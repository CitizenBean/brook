mod common;
use brook::*;
use common::*;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

fn pause(path: &Path) -> ! {
    let mut file = std::fs::File::create(path.join("boundary-reached")).unwrap();
    file.write_all(b"ready").unwrap();
    file.sync_all().unwrap();
    loop {
        std::thread::park();
    }
}
struct PausingHarness<'a>(&'a Path);
impl Harness for PausingHarness<'_> {
    fn name(&self) -> &'static str {
        "pausing-test-harness"
    }
    fn run(
        &self,
        _context: &Context,
        _budget: usize,
    ) -> std::result::Result<String, HarnessFailure> {
        pause(self.0)
    }
}
/// Launched only by the parent test, then killed without Rust destructors.
#[test]
#[ignore]
fn crash_child() {
    let path = std::env::var("BROOK_CRASH_DIRECTORY").unwrap();
    let phase = std::env::var("BROOK_CRASH_PHASE").unwrap();
    let path = Path::new(&path);
    let mut s = store(path);
    let (_, lease, input) = setup(&mut s);
    let hook_phase = phase.clone();
    let hook_path = path.to_owned();
    s.set_boundary_hook(move |boundary| {
        if format!("{boundary:?}") == hook_phase {
            pause(&hook_path);
        }
    });
    let r = s.admit(&lease, &input).unwrap();
    let send = s.begin_send(&lease, r).unwrap();
    if phase == "EffectApplied" {
        // A separately persisted fake provider effect survives the authority's SIGKILL.
        let mut effect = std::fs::File::create(path.join("fake-provider-applied")).unwrap();
        effect.write_all(b"effect-1").unwrap();
        effect.sync_all().unwrap();
        pause(path);
    }
    if phase.starts_with("Quarantine") || phase.starts_with("Recovery") {
        s.finish_send(&send, SendOutcome::Unknown).unwrap();
        s.notify_recovery(r).unwrap();
        panic!("recovery boundary not reached");
    }
    if phase != "CompletedWithSendInFlight" {
        s.finish_send(&send, SendOutcome::Applied).unwrap();
    }
    s.accept_reply(r, PEER, "answer").unwrap();
    let context = s.build_context(r).unwrap();
    s.admit_resume(&lease, &context).unwrap();
    let attempt = s.claim_job(&lease, r).unwrap();
    if phase == "JobClaimed" {
        pause(path);
    }
    if phase == "InHarness" {
        PausingHarness(path)
            .run(attempt.context(), attempt.output_budget())
            .unwrap();
    }
    s.complete_job(&attempt, "result").unwrap();
    if phase == "CompletedWithSendInFlight" {
        pause(path);
    }
    panic!("boundary not reached: {phase}");
}

#[test]
fn kill_and_reopen_at_every_durable_boundary() {
    let phases = [
        "AdmissionBeforeCommit",
        "AdmissionAfterCommit",
        "SendBeforeCommit",
        "SendAfterCommit",
        "EffectApplied",
        "ReplyBeforeCommit",
        "ReplyAfterCommit",
        "JobBeforeCommit",
        "JobAfterCommit",
        "JobClaimed",
        "InHarness",
        "CompletedWithSendInFlight",
        "ResultBeforeCommit",
        "ResultAfterCommit",
        "QuarantineBeforeCommit",
        "QuarantineAfterCommit",
        "RecoveryBeforeCommit",
        "RecoveryAfterCommit",
    ];
    for phase in phases {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "crash_child", "--nocapture"])
            .env("BROOK_CRASH_DIRECTORY", dir.path())
            .env("BROOK_CRASH_PHASE", phase)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !dir.path().join("boundary-reached").exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited before {phase}: {status}");
            }
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child timed out at {phase}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // A second process cannot acquire the authority even at a paused transaction.
        assert!(
            matches!(
                Store::open(
                    dir.path(),
                    Limits::default(),
                    Arc::new(ManualClock::default())
                ),
                Err(Error::Locked)
            ),
            "{phase}"
        );
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let mut s = store(dir.path());
        let session = s
            .resolve_session("namespace-a", "client-a", "conversation")
            .unwrap();
        let lease = s.claim(session, "restarted", 10000).unwrap();
        let r = Receipt { request: 1 };
        if phase == "AdmissionBeforeCommit" {
            assert!(s.request_state(r).is_err());
            assert!(s.pending_outbox(64).unwrap().is_empty());
            assert_eq!(s.history(session).unwrap().len(), 1);
            continue;
        }
        assert_eq!(
            s.history(session)
                .unwrap()
                .iter()
                .filter(|e| e.kind == "call")
                .count(),
            1,
            "{phase}"
        );
        match phase {
            "AdmissionAfterCommit" | "SendBeforeCommit" => {
                assert_eq!(s.outbox_state(r).unwrap(), "ready");
                assert_eq!(s.pending_outbox(64).unwrap(), vec![r]);
            }
            "SendAfterCommit"
            | "EffectApplied"
            | "QuarantineBeforeCommit"
            | "QuarantineAfterCommit"
            | "RecoveryBeforeCommit" => {
                assert_eq!(s.outbox_state(r).unwrap(), "unknown");
                assert!(s.pending_outbox(64).unwrap().is_empty());
                assert!(matches!(s.begin_send(&lease, r), Err(Error::NotReady)));
                assert!(s.notify_recovery(r).unwrap());
                assert!(!s.notify_recovery(r).unwrap());
                if phase == "EffectApplied" {
                    assert_eq!(
                        std::fs::read(dir.path().join("fake-provider-applied")).unwrap(),
                        b"effect-1"
                    );
                }
            }
            "CompletedWithSendInFlight" => {
                assert_eq!(s.request_state(r).unwrap(), "done");
                assert_eq!(s.outbox_state(r).unwrap(), "unknown");
                let inspect = rusqlite::Connection::open(dir.path().join("brook.sqlite3")).unwrap();
                let pins: i64 = inspect
                    .query_row(
                        "SELECT count(*) FROM pins WHERE request=?",
                        [r.request],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert!(
                    pins > 0,
                    "restart recovery must retain causal evidence after early job completion"
                );
                assert!(s.notify_recovery(r).unwrap());
                assert!(matches!(s.begin_send(&lease, r), Err(Error::NotReady)));
            }
            "RecoveryAfterCommit" => {
                assert!(!s.notify_recovery(r).unwrap());
                assert_eq!(
                    s.history(session)
                        .unwrap()
                        .iter()
                        .filter(|e| e.kind == "recovery")
                        .count(),
                    1
                );
            }
            "ReplyBeforeCommit" => {
                assert_eq!(s.request_state(r).unwrap(), "pending");
                assert!(s.pending_jobs(64).unwrap().is_empty());
                assert!(!s
                    .history(session)
                    .unwrap()
                    .iter()
                    .any(|e| e.kind == "outcome"));
            }
            "ReplyAfterCommit" | "JobBeforeCommit" => {
                assert_eq!(s.request_state(r).unwrap(), "accepted");
                assert_eq!(s.job_state(r).unwrap(), "pending");
                assert_eq!(s.pending_jobs(64).unwrap(), vec![r]);
            }
            "JobAfterCommit" | "JobClaimed" | "InHarness" | "ResultBeforeCommit" => {
                assert_eq!(s.request_state(r).unwrap(), "accepted");
                assert_eq!(s.job_state(r).unwrap(), "ready");
                let attempt = s.claim_job(&lease, r).unwrap();
                s.complete_job(&attempt, "recovered result").unwrap();
                assert_eq!(
                    s.history(session)
                        .unwrap()
                        .iter()
                        .filter(|e| e.kind == "result")
                        .count(),
                    1
                );
            }
            "ResultAfterCommit" => {
                assert_eq!(s.request_state(r).unwrap(), "done");
                assert_eq!(s.job_state(r).unwrap(), "done");
                assert!(s.pending_jobs(64).unwrap().is_empty());
                assert_eq!(
                    s.history(session)
                        .unwrap()
                        .iter()
                        .filter(|e| e.kind == "result")
                        .count(),
                    1
                );
            }
            _ => unreachable!(),
        }
    }
}
