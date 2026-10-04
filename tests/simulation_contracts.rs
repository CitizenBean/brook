mod common;
use brook::{processing::*, *};
use common::*;
use serde_json::json;
use std::sync::Arc;

#[test]
fn cancellation_at_each_continuation_phase_rejects_late_work_and_releases_slot() {
    for phase in 0..4 {
        let d = tempfile::tempdir().unwrap();
        let mut s = store(d.path());
        let (session, lease, input) = setup(&mut s);
        let r = sent(&mut s, &lease, &input);
        if phase > 0 {
            s.accept_reply(r, PEER, "result").unwrap();
        }
        if phase > 1 {
            let c = s.build_context(r).unwrap();
            s.admit_resume(&lease, &c).unwrap();
        }
        let running = if phase == 3 {
            Some(s.claim_job(&lease, r).unwrap())
        } else {
            None
        };
        s.cancel_request(&lease, r).unwrap();
        assert!(s.accept_reply(r, PEER, "result").is_err());
        assert!(s.claim_job(&lease, r).is_err());
        if let Some(a) = running {
            assert!(s.complete_job(&a, "late").is_err());
            assert!(s.fail_job(&a, HarnessFailure::ExecutionFailed).is_err());
        }
        let mut next = input.clone();
        next.operation = "next".into();
        let next = ready(&mut s, &lease, &next);
        let a = s.claim_job(&lease, next).unwrap();
        s.complete_job(&a, "next succeeds").unwrap();
        let history = s.history(session).unwrap();
        assert!(!history
            .iter()
            .any(|e| e.request == Some(r.request) && e.kind == "result"));
        assert_eq!(history.iter().filter(|e| e.kind == "result").count(), 1);
    }
}
#[test]
fn revision_races_at_manifest_and_claim_rebuild_from_current_evidence() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let (session, lease, input) = setup(&mut s);
    let r = accepted(&mut s, &lease, &input);
    let old = s.build_context(r).unwrap();
    s.message(session, "between build and admission").unwrap();
    assert!(matches!(
        s.admit_resume(&lease, &old),
        Err(Error::RevisionChanged)
    ));
    let c = s.build_context(r).unwrap();
    s.admit_resume(&lease, &c).unwrap();
    s.message(session, "between admission and claim").unwrap();
    assert!(matches!(
        s.claim_job(&lease, r),
        Err(Error::RevisionChanged)
    ));
    let c = s.build_context(r).unwrap();
    s.admit_resume(&lease, &c).unwrap();
    let a = s.claim_job(&lease, r).unwrap();
    assert!(a
        .context()
        .history
        .iter()
        .any(|e| e.text == "between admission and claim"));
    let result = SummaryHarness.run(a.context(), a.output_budget()).unwrap();
    s.complete_job(&a, &result).unwrap();
    assert!(s
        .history(session)
        .unwrap()
        .iter()
        .any(|e| e.text == "between build and admission"));
}
struct Oversized;
impl Harness for Oversized {
    fn name(&self) -> &'static str {
        "bad-oversized"
    }
    fn run(&self, _: &Context, budget: usize) -> std::result::Result<String, HarnessFailure> {
        Ok("🦀".repeat(budget))
    }
}
#[test]
fn misbehaving_harness_rejected_then_explicit_failure_frees_slot() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let (_, lease, input) = setup(&mut s);
    let r = ready(&mut s, &lease, &input);
    let a = s.claim_job(&lease, r).unwrap();
    let bad = Oversized.run(a.context(), a.output_budget()).unwrap();
    assert!(s.complete_job(&a, &bad).is_err());
    s.fail_job(&a, HarnessFailure::OutputBudgetExceeded)
        .unwrap();
    assert_eq!(s.request_state(r).unwrap(), "recovery_failed");
    let mut next = input.clone();
    next.operation = "next".into();
    let next = ready(&mut s, &lease, &next);
    let a = s.claim_job(&lease, next).unwrap();
    s.complete_job(&a, "bounded").unwrap();
}
#[test]
fn physical_retry_budget_does_not_reset_at_reopen_and_stale_failure_is_fenced() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let (session, lease, input) = setup(&mut s);
    let r = ready(&mut s, &lease, &input);
    let old = s.claim_job(&lease, r).unwrap();
    drop(s);
    for attempt in 2..=3 {
        let mut s = store(d.path());
        let lease = s.claim(session, "new", 10000).unwrap();
        assert!(matches!(
            s.fail_job(&old, HarnessFailure::ExecutionFailed),
            Err(Error::Fenced)
        ));
        let a = s.claim_job(&lease, r).unwrap();
        assert_eq!(a.context().request, r.request);
        assert!(attempt <= 3);
        drop(s);
    }
    let mut s = store(d.path());
    let lease = s.claim(session, "last", 10000).unwrap();
    assert!(matches!(s.claim_job(&lease, r), Err(Error::Capacity)));
    assert_eq!(s.request_state(r).unwrap(), "recovery_failed");
}
struct Routes(Vec<String>);
impl Processor for Routes {
    fn code(&self) -> Version {
        Version::new("routes", 1)
    }
    fn process(&self, a: &ProcessingAttempt) -> Result<Proposal> {
        Ok(Proposal {
            reason: "selected branch fixture".into(),
            state: json!(a.state().as_u64().unwrap_or(0) + 1),
            outputs: vec![a.event().payload.clone()],
            routing: Routing::SelectedBranches(self.0.clone()),
        })
    }
}
fn graph_store() -> (tempfile::TempDir, Store, TerminalClient, Graph) {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    s.configure_processing(ProcessingLimits {
        deliveries: 4,
        ..Default::default()
    })
    .unwrap();
    let identity = TerminalIdentity::local();
    let c = s.terminal_client(identity.clone()).unwrap();
    let binding = Version::new("stdout", 1);
    s.bind_terminal(&binding, &c).unwrap();
    let mut g = terminal_recipe(&identity, binding).unwrap();
    g.entry = "route".into();
    g.nodes.get_mut("route").unwrap().code = Version::new("routes", 1);
    g.nodes.insert("second".into(), g.nodes["output"].clone());
    let r = g.nodes.get_mut("route").unwrap();
    r.branches.clear();
    r.branches
        .insert("a".into(), vec!["output".into(), "second".into()]);
    r.branches.insert("b".into(), vec!["second".into()]);
    s.register_graph(&g).unwrap();
    (d, s, c, g)
}
#[test]
fn graph_pin_branch_order_dedup_invalid_routes_and_capacity_are_atomic() {
    let mut observed = Vec::new();
    for order in [vec!["a".into(), "b".into()], vec!["b".into(), "a".into()]] {
        let (_, mut s, c, mut g) = graph_store();
        let first = s
            .submit_terminal(&c, "terminal", 1, "one", json!("text"))
            .unwrap();
        let second = s
            .submit_terminal(&c, "terminal", 1, "two", json!("text"))
            .unwrap();
        g.version = 2;
        g.nodes.get_mut("route").unwrap().branches.remove("a");
        s.register_graph(&g).unwrap();
        let lease = s.claim_processing(first, "owner", 10000).unwrap();
        let a = s.prepare_processing(&lease, first).unwrap();
        let b = s.prepare_processing(&lease, second).unwrap();
        let bad = s
            .execute_processor(&a, &Routes(vec!["missing".into()]))
            .unwrap();
        assert!(s.commit_processing(&a, &bad).is_err());
        assert!(s.committed_processing(first).unwrap().is_none());
        assert!(s.pending_processing(64).unwrap().is_empty());
        let mut wrong = s.execute_processor(&a, &Routes(order.clone())).unwrap();
        wrong.outputs = vec![json!(42)];
        assert!(s.commit_processing(&a, &wrong).is_err());
        let p = s.execute_processor(&a, &Routes(order)).unwrap();
        let done = s.commit_processing(&a, &p).unwrap();
        assert_eq!(done.deliveries.len(), 2);
        assert_eq!(s.commit_processing(&a, &p).unwrap(), done);
        assert!(matches!(
            s.commit_processing(&b, &p),
            Err(Error::RevisionChanged)
        ));
        let b = s.prepare_processing(&lease, second).unwrap();
        assert_eq!(b.state(), &json!(1));
        let p = s.execute_processor(&b, &Routes(vec!["a".into()])).unwrap();
        assert!(matches!(s.commit_processing(&b, &p), Err(Error::Capacity)));
        s.fail_processing(&b, "fanout quota", false).unwrap();
        assert_eq!(s.inspect_processing(second).unwrap().status, "failed");
        let targets: Vec<_> = done
            .deliveries
            .iter()
            .map(|id| {
                let v = s.inspect_processing(*id).unwrap();
                assert_eq!(v.graph_version, 1);
                v.scope.node
            })
            .collect();
        observed.push(targets);
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0], vec!["output", "second"]);
}
#[test]
fn terminal_binding_revocation_before_and_after_admission_has_distinct_semantics() {
    for after in [false, true] {
        let (_, mut s, c, mut g) = graph_store();
        g.version = 2;
        g.entry = "output".into();
        s.register_graph(&g).unwrap();
        let id = s
            .submit_terminal(
                &c,
                "terminal",
                2,
                "one",
                json!("\u{1b}]52;c;bad\u{7} café\n\tline"),
            )
            .unwrap();
        let lease = s.claim_processing(id, "sink", 10000).unwrap();
        let a = if after {
            Some(s.begin_terminal(&lease, id, &c).unwrap())
        } else {
            None
        };
        s.revoke_terminal(&Version::new("stdout", 1)).unwrap();
        let mut out = Vec::new();
        if let Some(a) = a {
            s.print_terminal(&a, &mut out).unwrap();
            assert_eq!(
                String::from_utf8(out).unwrap(),
                "\\u{1b}]52;c;bad\\u{7} café\n\tline\n"
            );
        } else {
            assert!(matches!(
                s.begin_terminal(&lease, id, &c),
                Err(Error::Unauthorized)
            ));
            assert!(out.is_empty());
        }
    }
}
#[test]
fn independent_session_interleavings_are_equivalent_after_normalizing_global_ids() {
    fn run(order: &[usize], namespace: &str) -> Vec<Vec<(String, String)>> {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(
            d.path(),
            Limits::default(),
            Arc::new(ManualClock::default()),
        )
        .unwrap();
        let mut sessions = Vec::new();
        let mut leases = Vec::new();
        let mut inputs = Vec::new();
        for n in 0..2 {
            let sid = s
                .resolve_session(namespace, &format!("client-{n}"), "same-id")
                .unwrap();
            let lease = s.claim(sid, "owner", 10000).unwrap();
            let event = s.message(sid, &format!("input-{n}")).unwrap();
            let dest = target();
            let grant = format!("g-{n}");
            let version = s.grant(&grant, sid, &dest, "work", 10000).unwrap();
            inputs.push(Submission {
                operation: "same-op".into(),
                agent: AgentConfig::default(),
                grant,
                grant_version: version,
                destination: dest,
                instruction: "request".into(),
                payload: "work".into(),
                expected_peer: PEER.into(),
                causal_events: vec![event],
                lifetime_ms: 9000,
            });
            sessions.push(sid);
            leases.push(lease);
        }
        for &n in order {
            let r = ready(&mut s, &leases[n], &inputs[n]);
            let a = s.claim_job(&leases[n], r).unwrap();
            s.complete_job(&a, "same semantic result").unwrap();
        }
        sessions
            .into_iter()
            .map(|sid| {
                s.history(sid)
                    .unwrap()
                    .into_iter()
                    .map(|e| (e.kind, e.text))
                    .collect()
            })
            .collect()
    }
    assert_eq!(run(&[0, 1], "original"), run(&[1, 0], "renamed"));
}
