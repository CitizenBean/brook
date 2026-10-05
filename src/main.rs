use brook::*;
use std::{path::Path, sync::Arc};

fn open(path: &Path) -> Result<Store> {
    Store::open(path, Limits::default(), Arc::new(MonotonicClock::default()))
}
fn run_job(
    store: &mut Store,
    lease: &Lease,
    receipt: Receipt,
    harness: &dyn Harness,
) -> Result<()> {
    let context = store.build_context(receipt)?;
    println!(
        "{} / {}: request {}, current revision {}, {} events",
        context.agent.role,
        harness.name(),
        receipt.request,
        context.revision,
        context.history.len()
    );
    store.admit_resume(lease, &context)?;
    let attempt = store.claim_job(lease, receipt)?;
    match harness.run(attempt.context(), attempt.output_budget()) {
        Ok(output) => store.complete_job(&attempt, &output),
        Err(failure) => {
            store.fail_job(&attempt, failure)?;
            Err(failure.into())
        }
    }
}
fn demo(path: &Path) -> Result<()> {
    // Require a fresh demo directory: never append another demo into user state.
    if path.join("brook.sqlite3").exists() {
        return Err(Error::Invalid("demo requires a new store directory"));
    }
    let mut s = open(path)?;
    let a = s.resolve_session("demo", "fake-client", "session-a")?;
    let b = s.resolve_session("demo", "fake-client", "session-b")?;
    let lease = s.claim(a, "worker-a", 60000)?;
    let other = s.claim(b, "worker-b", 60000)?;
    let initial = s.message(a, "Compare the two results.")?;
    s.message(b, "This session has independent history.")?;
    let target = Destination {
        sink: "fake".into(),
        account: "demo-account".into(),
        recipient: "demo-recipient".into(),
    };
    let version = s.grant("demo-grant", a, &target, "demo work", 60000)?;
    let input = Submission {
        agent: AgentConfig::default(),
        operation: "request-a".into(),
        grant: "demo-grant".into(),
        grant_version: version,
        destination: target,
        instruction: "Gather result A.".into(),
        payload: "demo work".into(),
        expected_peer: "fake-peer".into(),
        causal_events: vec![initial],
        lifetime_ms: 60000,
    };
    let r_a = s.admit(&lease, &input)?;
    let mut second = input.clone();
    second.operation = "request-b".into();
    second.instruction = "Gather result B.".into();
    second.agent.role = "reviewer".into();
    second.agent.id = "reviewer".into();
    second.agent.instructions = "Review the supplied evidence.".into();
    let r_b = s.admit(&lease, &second)?;
    let sink = FakeSink {
        outcome: SendOutcome::Applied,
    };
    for r in [r_a, r_b] {
        let send = s.begin_send(&lease, r)?;
        s.finish_send(&send, sink.send(&send))?;
    }
    s.message(a, "Newer activity must survive both replies.")?;
    s.accept_reply(r_b, "fake-peer", "B completed first")?;
    run_job(&mut s, &lease, r_b, &EchoHarness)?;
    s.release(&other)?;
    drop(s);
    println!("Restarting local authority; A remains pending.");
    let mut s = open(path)?;
    let lease = s.claim(a, "worker-after-restart", 60000)?;
    assert_eq!(s.admit(&lease, &input)?, r_a);
    s.accept_reply(r_a, "fake-peer", "A completed after restart")?;
    run_job(&mut s, &lease, r_a, &SummaryHarness)?;
    let history = s.history(a)?;
    assert!(history
        .iter()
        .any(|e| e.text == "Newer activity must survive both replies."));
    assert_eq!(s.history(b)?.len(), 1);
    assert_eq!(history.iter().filter(|e| e.kind == "result").count(), 2);
    println!("PASS: stable receipt, two isolated sessions, B before A, both results retained.");
    Ok(())
}
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = if args.len() == 3 && args[1] == "demo" {
        demo(Path::new(&args[2]))
    } else {
        Err(Error::Invalid("usage: brook demo <new-store-directory>"))
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
