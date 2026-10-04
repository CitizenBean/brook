#[path = "support/simulation.rs"]
#[allow(dead_code)]
mod simulation;
use brook::*;
use simulation::{Action, Driver, ResultProcessor, Work};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
fn pause(path: &Path) -> ! {
    std::fs::write(path.join("paused"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}
fn invocation(path: &Path) {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.join("physical-invocations"))
        .unwrap();
    f.write_all(b"invoked\n").unwrap();
    f.sync_all().unwrap();
}
fn open(path: &Path) -> Store {
    Store::open(path, Limits::default(), Arc::new(ManualClock::default())).unwrap()
}
fn assert_fixture_graph(store: &Store, work: &Work) {
    let mut id = work.root;
    let mut parent = None;
    let mut pending = Vec::new();
    for node in ["filter", "route", "uppercase", "work", "output"] {
        let record = store.inspect_processing(id).unwrap();
        assert_eq!(record.scope.node, node);
        assert_eq!(record.parent, parent);
        assert_eq!(record.graph_version, 1);
        assert_eq!(record.scope.pipeline, "terminal");
        assert_eq!(record.scope.namespace, work.client.namespace);
        assert_eq!(
            record.scope.key,
            format!("{}:{}", work.client.client, work.client.conversation)
        );
        if record.status == "pending" {
            pending.push(id);
        }
        if record.status == "done" {
            assert_eq!(record.children.len(), 1);
            parent = Some(id);
            id = record.children[0];
        } else {
            assert!(record.children.is_empty());
            assert!(matches!(node, "work" | "output"));
            break;
        }
    }
    assert_eq!(
        store.pending_processing(64).unwrap(),
        pending,
        "complete pending set differs from fixture closure"
    );
}

#[test]
#[ignore]
fn simulation_child() {
    let dir = std::env::var("BROOK_SIM_CRASH_DIR").unwrap();
    let phase = std::env::var("BROOK_SIM_CRASH_PHASE").unwrap();
    let path = Path::new(&dir);
    let mut host = Driver::new(path).unwrap();
    for a in [
        Action::Admit(0),
        Action::Send(0),
        Action::Activity(0),
        Action::Reply(0),
    ] {
        host.action(&a).unwrap();
    }
    let work = host.works[0].clone();
    let mut f = std::fs::File::create(path.join("host-fixture.json")).unwrap();
    f.write_all(&serde_json::to_vec(&work).unwrap()).unwrap();
    f.sync_all().unwrap();
    drop(host);
    let mut store = open(path);
    let lease = store.claim(work.session, "worker", 10000).unwrap();
    let context = store.build_context(work.receipt).unwrap();
    store.admit_resume(&lease, &context).unwrap();
    let attempt = store.claim_job(&lease, work.receipt).unwrap();
    let result = EchoHarness
        .run(attempt.context(), attempt.output_budget())
        .unwrap();
    invocation(path);
    if phase == "PhysicalHarnessReturned" {
        pause(path);
    }
    store.complete_job(&attempt, &result).unwrap();
    if phase == "LogicalResultCommitted" {
        pause(path);
    }
    let wanted = phase.clone();
    let dir = path.to_owned();
    store.set_boundary_hook(move |b| {
        if format!("{b:?}") == wanted {
            pause(&dir)
        }
    });
    let lease = store
        .claim_processing(work.delivery, "publish", 10000)
        .unwrap();
    let a = store.prepare_processing(&lease, work.delivery).unwrap();
    let proposal = store
        .execute_processor(
            &a,
            &ResultProcessor {
                delivery: work.delivery,
                result,
            },
        )
        .unwrap();
    let done = store.commit_processing(&a, &proposal).unwrap();
    store.release_processing(&lease).unwrap();
    let id = done.deliveries[0];
    let c = store.terminal_client(work.client).unwrap();
    let lease = store.claim_processing(id, "print", 10000).unwrap();
    let a = store.begin_terminal(&lease, id, &c).unwrap();
    let mut journal = std::fs::File::create(path.join("external-output")).unwrap();
    store.print_terminal(&a, &mut journal).unwrap();
    panic!("boundary missed");
}
#[test]
fn kill_across_host_composition_boundaries_then_recover_through_public_apis() {
    for phase in [
        "PhysicalHarnessReturned",
        "LogicalResultCommitted",
        "ProcessorAfterFirstChild",
        "ProcessorAfterCommit",
        "TerminalWriteAfterCommit",
        "TerminalWritten",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "simulation_child", "--nocapture"])
            .env("BROOK_SIM_CRASH_DIR", dir.path())
            .env("BROOK_SIM_CRASH_PHASE", phase)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !dir.path().join("paused").exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited {phase}: {status}");
            }
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("timeout {phase}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.kill().unwrap();
        assert!(
            !child.wait().unwrap().success(),
            "killed child unexpectedly succeeded"
        );
        let work: Work =
            serde_json::from_slice(&std::fs::read(dir.path().join("host-fixture.json")).unwrap())
                .unwrap();
        let mut s = open(dir.path());
        assert_fixture_graph(&s, &work);
        let lease = s.claim(work.session, "recover", 10000).unwrap();
        assert_eq!(s.admit(&lease, &work.input).unwrap(), work.receipt);
        if s.request_state(work.receipt).unwrap() != "done" {
            let a = s.claim_job(&lease, work.receipt).unwrap();
            let text = EchoHarness.run(a.context(), a.output_budget()).unwrap();
            invocation(dir.path());
            s.complete_job(&a, &text).unwrap();
        }
        let results: Vec<_> = s
            .history(work.session)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "result" && e.request == Some(work.receipt.request))
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, work.expected_result);
        let done = if let Some(done) = s.committed_processing(work.delivery).unwrap() {
            done
        } else {
            let lease = s
                .claim_processing(work.delivery, "recover-publish", 10000)
                .unwrap();
            let a = s.prepare_processing(&lease, work.delivery).unwrap();
            let proposal = s
                .execute_processor(
                    &a,
                    &ResultProcessor {
                        delivery: work.delivery,
                        result: results[0].text.clone(),
                    },
                )
                .unwrap();
            let done = s.commit_processing(&a, &proposal).unwrap();
            s.release_processing(&lease).unwrap();
            done
        };
        assert_eq!(done.deliveries.len(), 1);
        assert_fixture_graph(&s, &work);
        let id = done.deliveries[0];
        let c = s.terminal_client(work.client.clone()).unwrap();
        let lease = s.claim_processing(id, "recover-print", 10000).unwrap();
        if ["TerminalWriteAfterCommit", "TerminalWritten"].contains(&phase) {
            assert_eq!(s.inspect_processing(id).unwrap().status, "unknown");
            assert!(matches!(
                s.begin_terminal(&lease, id, &c),
                Err(Error::NotReady)
            ));
            let bytes = std::fs::read(dir.path().join("external-output")).unwrap();
            if phase == "TerminalWritten" {
                assert_eq!(bytes, format!("{}\n", work.expected_result).as_bytes());
            } else {
                assert!(bytes.is_empty());
            }
        } else {
            let a = s.begin_terminal(&lease, id, &c).unwrap();
            let mut out = std::fs::File::create(dir.path().join("external-output")).unwrap();
            s.print_terminal(&a, &mut out).unwrap();
            assert_eq!(
                std::fs::read_to_string(dir.path().join("external-output")).unwrap(),
                format!("{}\n", work.expected_result)
            );
        }
        assert_fixture_graph(&s, &work);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("physical-invocations"))
                .unwrap()
                .lines()
                .count(),
            if phase == "PhysicalHarnessReturned" {
                2
            } else {
                1
            }
        );
    }
}
