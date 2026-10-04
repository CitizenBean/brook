use brook::{processing::*, *};
use serde_json::json;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
fn open(path: &Path) -> Store {
    Store::open(path, Limits::default(), Arc::new(ManualClock::default())).unwrap()
}
fn pause(path: &Path) -> ! {
    let mut f = std::fs::File::create(path.join("boundary")).unwrap();
    f.write_all(b"ready").unwrap();
    f.sync_all().unwrap();
    loop {
        std::thread::park();
    }
}
#[test]
#[ignore]
fn processor_crash_child() {
    let dir = std::env::var("BROOK_PROCESSOR_CRASH_DIR").unwrap();
    let phase = std::env::var("BROOK_PROCESSOR_CRASH_PHASE").unwrap();
    let path = Path::new(&dir);
    let mut s = open(path);
    s.configure_processing(Default::default()).unwrap();
    let i = TerminalIdentity::local();
    let c = s.terminal_client(i.clone()).unwrap();
    let binding = Version::new("stdout", 1);
    s.bind_terminal(&binding, &c).unwrap();
    let mut g = terminal_recipe(&i, binding).unwrap();
    g.entry = "route".into();
    g.nodes.insert("second".into(), g.nodes["output"].clone());
    g.nodes
        .get_mut("route")
        .unwrap()
        .branches
        .insert("display".into(), vec!["output".into(), "second".into()]);
    s.register_graph(&g).unwrap();
    let hp = path.to_owned();
    let wanted = phase.clone();
    s.set_boundary_hook(move |b| {
        if format!("{b:?}") == wanted {
            pause(&hp)
        }
    });
    let id = s
        .submit_terminal(&c, "terminal", 1, "one", json!("hello"))
        .unwrap();
    let l = s.claim_processing(id, "worker", 10000).unwrap();
    let a = s.prepare_processing(&l, id).unwrap();
    if phase == "ProcessorExecuting" {
        pause(path);
    }
    let p = s
        .execute_processor(&a, &RustAdapter(TextProcessor("router")))
        .unwrap();
    let done = s.commit_processing(&a, &p).unwrap();
    s.release_processing(&l).unwrap();
    let id = done.deliveries[0];
    let l = s.claim_processing(id, "writer", 10000).unwrap();
    let a = s.begin_terminal(&l, id, &c).unwrap();
    let mut f = std::fs::File::create(path.join("terminal-output")).unwrap();
    s.print_terminal(&a, &mut f).unwrap();
    panic!("missed {phase}");
}
#[test]
fn process_kills_preserve_atomic_fanout_and_terminal_uncertainty() {
    for phase in [
        "ProcessorIngressBeforeCommit",
        "ProcessorIngressAfterCommit",
        "ProcessorExecuting",
        "ProcessorAfterFirstChild",
        "ProcessorBeforeCommit",
        "ProcessorAfterCommit",
        "TerminalBeforeCommit",
        "TerminalAfterCommit",
        "TerminalWriteBeforeCommit",
        "TerminalWriteAfterCommit",
        "TerminalWritten",
        "TerminalFinishBeforeCommit",
        "TerminalFinishAfterCommit",
    ] {
        let d = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "processor_crash_child",
                "--nocapture",
            ])
            .env("BROOK_PROCESSOR_CRASH_DIR", d.path())
            .env("BROOK_PROCESSOR_CRASH_PHASE", phase)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !d.path().join("boundary").exists() {
            if let Some(code) = child.try_wait().unwrap() {
                panic!("early exit {phase}: {code}");
            }
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("timeout {phase}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let mut s = open(d.path());
        if phase == "ProcessorIngressBeforeCommit" {
            assert!(s.pending_processing(64).unwrap().is_empty());
            continue;
        }
        let c = s.terminal_client(TerminalIdentity::local()).unwrap();
        assert_eq!(
            s.submit_terminal(&c, "terminal", 1, "one", json!("hello"))
                .unwrap(),
            DeliveryId(1)
        );
        if [
            "ProcessorIngressAfterCommit",
            "ProcessorExecuting",
            "ProcessorAfterFirstChild",
            "ProcessorBeforeCommit",
        ]
        .contains(&phase)
        {
            assert!(s.committed_processing(DeliveryId(1)).unwrap().is_none());
            assert_eq!(s.pending_processing(64).unwrap(), vec![DeliveryId(1)]);
            let l = s
                .claim_processing(DeliveryId(1), "recovery", 10000)
                .unwrap();
            let a = s.prepare_processing(&l, DeliveryId(1)).unwrap();
            assert_eq!(a.state(), &serde_json::Value::Null);
            let p = s
                .execute_processor(&a, &RustAdapter(TextProcessor("router")))
                .unwrap();
            assert_eq!(s.commit_processing(&a, &p).unwrap().deliveries.len(), 2);
            continue;
        }
        let done = s.committed_processing(DeliveryId(1)).unwrap().unwrap();
        assert_eq!(done.deliveries.len(), 2);
        let id = done.deliveries[0];
        let status = s.inspect_processing(id).unwrap().status;
        match phase {
            "ProcessorAfterCommit" | "TerminalBeforeCommit" => assert_eq!(status, "pending"),
            "TerminalFinishAfterCommit" => assert_eq!(status, "printed"),
            _ => {
                assert_eq!(status, "unknown");
                let l = s.claim_processing(id, "recovery", 10000).unwrap();
                assert!(matches!(s.begin_terminal(&l, id, &c), Err(Error::NotReady)));
            }
        }
        if ["TerminalWriteBeforeCommit", "TerminalWriteAfterCommit"].contains(&phase) {
            assert!(std::fs::read(d.path().join("terminal-output"))
                .unwrap()
                .is_empty());
        }
        if [
            "TerminalWritten",
            "TerminalFinishBeforeCommit",
            "TerminalFinishAfterCommit",
        ]
        .contains(&phase)
        {
            assert_eq!(
                std::fs::read(d.path().join("terminal-output")).unwrap(),
                b"hello\n"
            );
        }
    }
}
