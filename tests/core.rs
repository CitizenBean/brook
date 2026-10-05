mod common;
use brook::*;
use common::*;
use std::sync::{Arc, Mutex};

#[test]
fn stable_receipt_conflicts_and_namespace_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (a, lease, input) = setup(&mut s);
    let receipt = s.admit(&lease, &input).unwrap();
    assert_eq!(s.admit(&lease, &input).unwrap(), receipt);
    for changed in [
        {
            let mut x = input.clone();
            x.payload = "different".into();
            x
        },
        {
            let mut x = input.clone();
            x.destination.recipient = "other".into();
            x
        },
        {
            let mut x = input.clone();
            x.causal_events.clear();
            x
        },
        {
            let mut x = input.clone();
            x.agent.role = "reviewer".into();
            x
        },
    ] {
        assert!(matches!(s.admit(&lease, &changed), Err(Error::Conflict)));
    }
    let b = s
        .resolve_session("namespace-b", "client-a", "conversation")
        .unwrap();
    let c = s
        .resolve_session("namespace-a", "client-b", "conversation")
        .unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        s.resolve_session("namespace-a", "client-a", "conversation")
            .unwrap(),
        a
    );
    let other_lease = s.claim(b, "worker", 10000).unwrap();
    let mut other = input.clone();
    assert!(matches!(
        s.admit(&other_lease, &other),
        Err(Error::Unauthorized)
    ));
    other.grant = "grant-b".into();
    other.grant_version = s.grant("grant-b", b, &target(), "work", 10000).unwrap();
    assert!(matches!(
        s.admit(&other_lease, &other),
        Err(Error::Unauthorized)
    ));
    other.causal_events.clear();
    let different = s.admit(&other_lease, &other).unwrap();
    assert_ne!(receipt, different);
    drop(s);
    let mut s = store(dir.path());
    let lease = s.claim(a, "worker", 10000).unwrap();
    assert_eq!(s.admit(&lease, &input).unwrap(), receipt);
}
#[test]
fn concurrent_duplicate_admission_and_claims_are_serialized() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let s = Arc::new(Mutex::new(s));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let s = s.clone();
            let lease = lease.clone();
            let input = input.clone();
            std::thread::spawn(move || s.lock().unwrap().admit(&lease, &input).unwrap())
        })
        .collect();
    let receipts: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(receipts.iter().all(|r| *r == receipts[0]));
    let mut guard = s.lock().unwrap();
    guard.release(&lease).unwrap();
    drop(guard);
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let s = s.clone();
            std::thread::spawn(move || s.lock().unwrap().claim(session, "contender", 1000).is_ok())
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
}
#[test]
fn authority_lock_restart_and_cross_store_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, old, input) = setup(&mut s);
    let receipt = s.admit(&old, &input).unwrap();
    assert!(matches!(
        Store::open(
            dir.path(),
            Limits::default(),
            Arc::new(ManualClock::default())
        ),
        Err(Error::Locked)
    ));
    drop(s);
    let mut s = store(dir.path());
    assert!(matches!(s.heartbeat(&old, 100), Err(Error::Fenced)));
    let lease = s.claim(session, "worker", 1000).unwrap();
    assert!(lease.generation() > old.generation());
    assert_eq!(s.admit(&lease, &input).unwrap(), receipt);
    let other = tempfile::tempdir().unwrap();
    let mut other = store(other.path());
    let (_, other_lease, _) = setup(&mut other);
    assert!(matches!(s.heartbeat(&other_lease, 100), Err(Error::Fenced)));
}
#[test]
fn lease_deadline_takeover_and_stale_result_fencing() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(dir.path(), Limits::default(), clock.clone()).unwrap();
    let (session, old, input) = setup(&mut s);
    let receipt = ready(&mut s, &old, &input);
    let attempt = s.claim_job(&old, receipt).unwrap();
    clock.set(100);
    s.heartbeat(&old, 100).unwrap();
    clock.set(200);
    assert!(matches!(s.heartbeat(&old, 100), Err(Error::Fenced)));
    assert!(matches!(
        s.complete_job(&attempt, "old"),
        Err(Error::Fenced)
    ));
    let new = s.claim(session, "new-worker", 1000).unwrap();
    let next = s.claim_job(&new, receipt).unwrap();
    assert!(matches!(
        s.complete_job(&attempt, "old"),
        Err(Error::Fenced)
    ));
    s.complete_job(&next, "new").unwrap();
    assert_eq!(s.request_state(receipt).unwrap(), "done");
    s.release(&new).unwrap();
    let reacquired = s.claim(session, "new-worker", 100).unwrap();
    assert!(reacquired.generation() > new.generation());
    assert!(matches!(s.heartbeat(&new, 100), Err(Error::Fenced)));
    clock.set(199);
    assert!(matches!(s.heartbeat(&reacquired, 100), Err(Error::Fenced)));
}
#[test]
fn out_of_order_replies_newer_context_and_swappable_harnesses() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, a) = setup(&mut s);
    let a_receipt = sent(&mut s, &lease, &a);
    let mut b = a.clone();
    b.operation = "op-b".into();
    b.agent.role = "reviewer".into();
    let b_receipt = sent(&mut s, &lease, &b);
    let newer = s.message(session, "newer activity").unwrap();
    let other = s
        .resolve_session("other", "client", "conversation")
        .unwrap();
    s.message(other, "other session secret").unwrap();
    s.accept_reply(b_receipt, PEER, "B first").unwrap();
    let context = s.build_context(b_receipt).unwrap();
    assert!(context.history.iter().any(|e| e.id == newer));
    assert!(!context.history.iter().any(|e| e.text.contains("secret")));
    let same_harness: &dyn Harness = &EchoHarness;
    let output = same_harness
        .run(&context, s.limits().payload_bytes)
        .unwrap();
    assert!(output.contains("B first"));
    assert_eq!(context.agent.role, "reviewer");
    s.admit_resume(&lease, &context).unwrap();
    let attempt = s.claim_job(&lease, b_receipt).unwrap();
    s.complete_job(&attempt, &output).unwrap();
    s.accept_reply(a_receipt, PEER, "A second").unwrap();
    let context = s.build_context(a_receipt).unwrap();
    assert_eq!(context.agent.role, "assistant");
    assert!(same_harness
        .run(&context, s.limits().payload_bytes)
        .unwrap()
        .contains("A second"));
    assert!(context
        .history
        .iter()
        .any(|e| e.request == Some(b_receipt.request) && e.kind == "result"));
    s.admit_resume(&lease, &context).unwrap();
    let attempt = s.claim_job(&lease, a_receipt).unwrap();
    let output = SummaryHarness
        .run(attempt.context(), attempt.output_budget())
        .unwrap();
    s.message(session, "arrived during execution").unwrap();
    s.complete_job(&attempt, &output).unwrap();
    assert!(s
        .history(session)
        .unwrap()
        .iter()
        .any(|e| e.text == "arrived during execution"));
    assert_eq!(s.history(other).unwrap().len(), 1);
}
#[test]
fn revision_race_and_one_active_job_per_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, a) = setup(&mut s);
    let r = accepted(&mut s, &lease, &a);
    let context = s.build_context(r).unwrap();
    s.message(session, "racing event").unwrap();
    assert!(matches!(
        s.admit_resume(&lease, &context),
        Err(Error::RevisionChanged)
    ));
    let context = s.build_context(r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let mut b = a.clone();
    b.operation = "b".into();
    let b = ready(&mut s, &lease, &b);
    assert!(matches!(
        s.claim_job(&lease, r),
        Err(Error::RevisionChanged)
    ));
    let context = s.build_context(r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let attempt = s.claim_job(&lease, r).unwrap();
    assert!(matches!(s.claim_job(&lease, b), Err(Error::NotReady)));
    s.complete_job(&attempt, "done").unwrap();
    assert!(matches!(
        s.claim_job(&lease, b),
        Err(Error::RevisionChanged)
    ));
}
#[test]
fn forged_duplicate_and_conflicting_replies() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let r = sent(&mut s, &lease, &input);
    let before = s.history(session).unwrap();
    assert!(matches!(
        s.accept_reply(r, "forged-peer", "answer"),
        Err(Error::Unauthorized)
    ));
    assert_eq!(s.history(session).unwrap(), before);
    assert!(s.accept_reply(r, PEER, "answer").unwrap());
    assert!(!s.accept_reply(r, PEER, "answer").unwrap());
    assert!(matches!(
        s.accept_reply(r, PEER, "changed"),
        Err(Error::Conflict)
    ));
    assert_eq!(s.pending_jobs(64).unwrap(), vec![r]);
    assert!(s
        .accept_reply(Receipt { request: 999 }, PEER, "answer")
        .is_err());
}
#[test]
fn cancellation_is_local_and_fences_acceptance_and_execution() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, a) = setup(&mut s);
    let r = sent(&mut s, &lease, &a);
    let mut b = a.clone();
    b.operation = "b".into();
    let b = accepted(&mut s, &lease, &b);
    s.cancel_request(&lease, r).unwrap();
    assert!(matches!(
        s.accept_reply(r, PEER, "late"),
        Err(Error::NotReady)
    ));
    let context = s.build_context(b).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let attempt = s.claim_job(&lease, b).unwrap();
    let other = s
        .resolve_session("other", "client", "conversation")
        .unwrap();
    s.message(other, "keep").unwrap();
    s.cancel_session(session).unwrap();
    assert!(matches!(
        s.complete_job(&attempt, "late"),
        Err(Error::NotReady)
    ));
    assert_eq!(s.history(other).unwrap()[0].text, "keep");
}
#[test]
fn authorization_revalidation_expiry_rotation_and_two_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(dir.path(), Limits::default(), clock.clone()).unwrap();
    let (session, lease, a) = setup(&mut s);
    let r = s.admit(&lease, &a).unwrap();
    s.revoke("grant-a").unwrap();
    assert!(matches!(s.begin_send(&lease, r), Err(Error::Unauthorized)));
    assert_eq!(s.outbox_state(r).unwrap(), "ready");
    let version = s
        .grant("grant-a", session, &target(), "work", 1000)
        .unwrap();
    assert!(version > a.grant_version);
    assert!(matches!(s.begin_send(&lease, r), Err(Error::Unauthorized)));
    let mut b = a.clone();
    b.operation = "b".into();
    b.grant_version = version;
    let b_receipt = s.admit(&lease, &b).unwrap();
    clock.set(1000);
    assert!(matches!(
        s.begin_send(&lease, b_receipt),
        Err(Error::Unauthorized)
    ));
    let mut target_b = target();
    target_b.recipient = "recipient-b".into();
    let v = s
        .grant("grant-b", session, &target_b, "work", 1000)
        .unwrap();
    let mut c = a.clone();
    c.operation = "c".into();
    c.destination = target_b.clone();
    c.grant = "grant-b".into();
    c.grant_version = v;
    let c = sent(&mut s, &lease, &c);
    assert_eq!(s.outbox_state(c).unwrap(), "sent");
    let v = s
        .grant("grant-c", session, &target(), "work", 1000)
        .unwrap();
    let mut d = a;
    d.operation = "d".into();
    d.grant = "grant-c".into();
    d.grant_version = v;
    let d = s.admit(&lease, &d).unwrap();
    let admitted = s.begin_send(&lease, d).unwrap();
    s.revoke("grant-c").unwrap();
    s.finish_send(&admitted, SendOutcome::Applied).unwrap();
    assert_eq!(s.outbox_state(d).unwrap(), "sent");
}
#[test]
fn unknown_is_quarantined_without_resend_and_notice_is_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let r = s.admit(&lease, &input).unwrap();
    let attempt = s.begin_send(&lease, r).unwrap();
    s.finish_send(&attempt, SendOutcome::Unknown).unwrap();
    assert!(matches!(s.begin_send(&lease, r), Err(Error::NotReady)));
    assert!(s.notify_recovery(r).unwrap());
    assert!(!s.notify_recovery(r).unwrap());
    assert_eq!(
        s.history(session)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "recovery")
            .count(),
        1
    );
    drop(s);
    let mut s = store(dir.path());
    let lease = s.claim(session, "worker", 1000).unwrap();
    assert!(matches!(s.begin_send(&lease, r), Err(Error::NotReady)));
    assert!(!s.notify_recovery(r).unwrap());
    assert_eq!(s.outbox_state(r).unwrap(), "unknown");
}
#[test]
fn quota_headroom_allows_completion_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        requests: 2,
        messages_per_session: 1,
        ..Limits::default()
    };
    let mut s = Store::open(dir.path(), limits.clone(), Arc::new(ManualClock::default())).unwrap();
    let (session, lease, a) = setup(&mut s);
    let a_r = sent(&mut s, &lease, &a);
    let mut b = a.clone();
    b.operation = "b".into();
    let b_r = s.admit(&lease, &b).unwrap();
    let send = s.begin_send(&lease, b_r).unwrap();
    s.finish_send(&send, SendOutcome::Unknown).unwrap();
    let mut c = a.clone();
    c.operation = "c".into();
    assert!(matches!(s.admit(&lease, &c), Err(Error::Capacity)));
    assert!(matches!(
        s.message(session, "over quota"),
        Err(Error::Capacity)
    ));
    s.accept_reply(a_r, PEER, "answer").unwrap();
    let context = s.build_context(a_r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let job = s.claim_job(&lease, a_r).unwrap();
    s.complete_job(&job, "completed at capacity").unwrap();
    s.notify_recovery(b_r).unwrap();
    assert_eq!(s.admit(&lease, &a).unwrap(), a_r);
    assert!(s.pending_jobs(65).is_err());
    drop(s);
    let mut s = Store::open(dir.path(), limits, Arc::new(ManualClock::default())).unwrap();
    let lease = s.claim(session, "worker", 1000).unwrap();
    assert_eq!(s.admit(&lease, &a).unwrap(), a_r);
    assert!(matches!(s.admit(&lease, &c), Err(Error::Capacity)));
}
#[test]
fn oversized_context_payload_and_unsupported_tools_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        context_bytes: 256,
        ..Limits::default()
    };
    let mut s = Store::open(dir.path(), limits, Arc::new(ManualClock::default())).unwrap();
    let (_, lease, input) = setup(&mut s);
    let r = accepted(&mut s, &lease, &input);
    assert!(matches!(s.build_context(r), Err(Error::ContextUnavailable)));
    assert_eq!(s.request_state(r).unwrap(), "accepted");
    s.fail_resume(&lease, r).unwrap();
    assert_eq!(s.request_state(r).unwrap(), "recovery_failed");
    s.notify_recovery(r).unwrap();
    let mut other = input.clone();
    other.operation = "other".into();
    other.payload = "x".repeat(4097);
    assert!(matches!(s.admit(&lease, &other), Err(Error::Invalid(_))));
    other.payload = "work".into();
    other.agent.allowed_tools.push("nested_dispatch".into());
    assert!(matches!(s.admit(&lease, &other), Err(Error::Invalid(_))));
}
#[test]
fn expiry_closes_work_and_preserves_unknown_effect() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(dir.path(), Limits::default(), clock.clone()).unwrap();
    let (_, lease, mut input) = setup(&mut s);
    input.lifetime_ms = 100;
    let r = s.admit(&lease, &input).unwrap();
    let _send = s.begin_send(&lease, r).unwrap();
    clock.set(100);
    assert_eq!(s.expire().unwrap(), 1);
    assert_eq!(s.request_state(r).unwrap(), "expired");
    assert_eq!(s.outbox_state(r).unwrap(), "unknown");
    assert!(matches!(
        s.accept_reply(r, PEER, "late"),
        Err(Error::NotReady)
    ));
    s.notify_recovery(r).unwrap();
}
#[test]
fn attempt_exhaustion_survives_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        job_attempts: 1,
        ..Limits::default()
    };
    let mut s = Store::open(dir.path(), limits.clone(), Arc::new(ManualClock::default())).unwrap();
    let (session, lease, input) = setup(&mut s);
    let r = ready(&mut s, &lease, &input);
    let _attempt = s.claim_job(&lease, r).unwrap();
    drop(s);
    let mut s = Store::open(dir.path(), limits, Arc::new(ManualClock::default())).unwrap();
    let lease = s.claim(session, "worker", 1000).unwrap();
    assert!(matches!(s.claim_job(&lease, r), Err(Error::Capacity)));
    assert_eq!(s.request_state(r).unwrap(), "recovery_failed");
    s.notify_recovery(r).unwrap();
}

#[test]
fn forged_context_and_cancel_before_resume_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let r = accepted(&mut s, &lease, &input);
    let context = s.build_context(r).unwrap();
    let mut forged = context.clone();
    forged.agent.instructions = "substituted".into();
    assert!(matches!(
        s.admit_resume(&lease, &forged),
        Err(Error::RevisionChanged)
    ));
    s.cancel_session(session).unwrap();
    assert!(s.admit_resume(&lease, &context).is_err());
    assert_eq!(s.job_state(r).unwrap(), "cancelled");
}
#[test]
fn pins_survive_accepted_work_and_uncertain_effect_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (_, lease, input) = setup(&mut s);
    let r = accepted(&mut s, &lease, &input);
    let inspect = rusqlite::Connection::open(dir.path().join("brook.sqlite3")).unwrap();
    let pins = |r: Receipt| {
        inspect
            .query_row(
                "SELECT count(*) FROM pins WHERE request=?",
                [r.request],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert!(pins(r) > 0);
    let context = s.build_context(r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let job = s.claim_job(&lease, r).unwrap();
    assert!(pins(r) > 0);
    s.complete_job(&job, "done").unwrap();
    assert_eq!(pins(r), 0);
    let mut next = input;
    next.operation = "unknown".into();
    let next = s.admit(&lease, &next).unwrap();
    let _send = s.begin_send(&lease, next).unwrap();
    s.cancel_request(&lease, next).unwrap();
    assert!(pins(next) > 0);
    assert_eq!(s.outbox_state(next).unwrap(), "unknown");
}
#[test]
fn takeover_quarantines_inflight_send_and_preserves_continuation() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::default());
    let mut s = Store::open(dir.path(), Limits::default(), clock.clone()).unwrap();
    let (session, old, input) = setup(&mut s);
    let r = s.admit(&old, &input).unwrap();
    let send = s.begin_send(&old, r).unwrap();
    s.heartbeat(&old, 100).unwrap();
    clock.set(100);
    let new = s.claim(session, "new", 1000).unwrap();
    assert_eq!(s.outbox_state(r).unwrap(), "unknown");
    assert!(matches!(
        s.finish_send(&send, SendOutcome::Applied),
        Err(Error::Fenced)
    ));
    assert!(matches!(s.begin_send(&new, r), Err(Error::NotReady)));
    s.accept_reply(r, PEER, "authenticated late outcome")
        .unwrap();
    let context = s.build_context(r).unwrap();
    s.admit_resume(&new, &context).unwrap();
}

#[test]
fn completed_reply_retains_pins_until_delivery_and_recovery_are_settled() {
    for outcome in [
        SendOutcome::Applied,
        SendOutcome::Unknown,
        SendOutcome::ConfirmedNotApplied,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let (_, lease, input) = setup(&mut s);
        let r = s.admit(&lease, &input).unwrap();
        let send = s.begin_send(&lease, r).unwrap();
        s.accept_reply(r, PEER, "reply before send result").unwrap();
        let context = s.build_context(r).unwrap();
        s.admit_resume(&lease, &context).unwrap();
        let job = s.claim_job(&lease, r).unwrap();
        s.complete_job(&job, "completed").unwrap();
        let inspect = rusqlite::Connection::open(dir.path().join("brook.sqlite3")).unwrap();
        let pins = || {
            inspect
                .query_row(
                    "SELECT count(*) FROM pins WHERE request=?",
                    [r.request],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(s.request_state(r).unwrap(), "done");
        assert_eq!(s.outbox_state(r).unwrap(), "in_flight");
        assert!(pins() > 0, "unresolved delivery must retain causal pins");
        s.finish_send(&send, outcome).unwrap();
        if matches!(outcome, SendOutcome::Applied) {
            assert_eq!(pins(), 0, "both dependencies are now settled");
        } else {
            assert!(pins() > 0, "recovery still depends on causal material");
            s.notify_recovery(r).unwrap();
            assert!(pins() > 0, "notification is not resolution");
        }
    }
}

#[test]
fn both_harnesses_complete_a_maximum_sized_valid_reply() {
    let harnesses: [&dyn Harness; 2] = [&EchoHarness, &SummaryHarness];
    for harness in harnesses {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let (_, lease, input) = setup(&mut s);
        let r = sent(&mut s, &lease, &input);
        let reply = "x".repeat(s.limits().payload_bytes);
        s.accept_reply(r, PEER, &reply).unwrap();
        let context = s.build_context(r).unwrap();
        s.admit_resume(&lease, &context).unwrap();
        let job = s.claim_job(&lease, r).unwrap();
        let result = harness.run(job.context(), job.output_budget()).unwrap();
        assert_eq!(
            result, reply,
            "optional prefix must not displace valid outcome bytes"
        );
        s.complete_job(&job, &result).unwrap();
        assert_eq!(s.job_state(r).unwrap(), "done");
    }
}

#[test]
fn running_harness_failure_releases_slot_and_rejects_late_results() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (_, lease, input) = setup(&mut s);
    let r = accepted(&mut s, &lease, &input);
    let mut next = input.clone();
    next.operation = "next".into();
    let next = accepted(&mut s, &lease, &next);
    let context = s.build_context(r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let job = s.claim_job(&lease, r).unwrap();
    let failure = EchoHarness.run(job.context(), 1).unwrap_err();
    assert_eq!(failure, HarnessFailure::OutputBudgetExceeded);
    s.fail_job(&job, failure).unwrap();
    assert_eq!(s.job_state(r).unwrap(), "failed");
    assert_eq!(s.request_state(r).unwrap(), "recovery_failed");
    assert!(matches!(
        s.complete_job(&job, "late success"),
        Err(Error::NotReady)
    ));
    assert!(matches!(s.fail_job(&job, failure), Err(Error::NotReady)));
    let context = s.build_context(next).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let next_job = s.claim_job(&lease, next).unwrap();
    // Same lease can immediately run the next job; no authority turnover needed.
    s.complete_job(&next_job, "next result").unwrap();
    s.notify_recovery(r).unwrap();
}

#[test]
fn stale_failure_cannot_close_a_newer_physical_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(dir.path());
    let (session, lease, input) = setup(&mut s);
    let r = ready(&mut s, &lease, &input);
    let old = s.claim_job(&lease, r).unwrap();
    s.release(&lease).unwrap();
    let new_lease = s.claim(session, "next-worker", 10000).unwrap();
    let new = s.claim_job(&new_lease, r).unwrap();
    assert!(matches!(
        s.fail_job(&old, HarnessFailure::ExecutionFailed),
        Err(Error::Fenced)
    ));
    assert_eq!(s.job_state(r).unwrap(), "running");
    // A misbehaving adapter is still checked by storage and can be settled safely.
    assert!(matches!(
        s.complete_job(&new, &"x".repeat(4097)),
        Err(Error::Invalid(_))
    ));
    s.fail_job(&new, HarnessFailure::OutputBudgetExceeded)
        .unwrap();
    assert_eq!(s.job_state(r).unwrap(), "failed");
}
