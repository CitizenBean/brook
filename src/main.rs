use brook::processing::*;
use brook::*;
use serde_json::json;
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
fn terminal_setup(path: &Path, uppercase: bool) -> Result<(Store, TerminalClient, Graph)> {
    let mut store = open(path)?;
    store.configure_processing(ProcessingLimits::default())?;
    let identity = TerminalIdentity::local();
    let client = store.terminal_client(identity.clone())?;
    let binding = Version::new("stdout", 1);
    store.bind_terminal(&binding, &client)?;
    let mut graph = terminal_recipe(&identity, binding)?;
    if uppercase {
        graph.version = 2;
        insert_uppercase(&mut graph)?;
    }
    store.register_graph(&graph)?;
    Ok((store, client, graph))
}
fn processor_demo(path: &Path) -> Result<()> {
    if path.join("brook.sqlite3").exists() {
        return Err(Error::Invalid("demo requires a new store directory"));
    }
    let (mut store, client, graph) = terminal_setup(path, false)?;
    let id = store.submit_terminal(
        &client,
        &graph.pipeline,
        graph.version,
        "first",
        json!("hello from the terminal"),
    )?;
    println!("Durable input receipt: {}", id.0);
    run_terminal_recipe(&mut store, &client, &mut std::io::stdout().lock())?;
    drop(store);
    let (mut store, client, graph) = terminal_setup(path, true)?;
    assert_eq!(
        store.submit_terminal(
            &client,
            &graph.pipeline,
            1,
            "first",
            json!("hello from the terminal")
        )?,
        id
    );
    store.submit_terminal(
        &client,
        &graph.pipeline,
        graph.version,
        "second",
        json!("transform inserted after the same router"),
    )?;
    run_terminal_recipe(&mut store, &client, &mut std::io::stdout().lock())?;
    println!("PASS: durable receipt survives restart; graph v2 inserts uppercase without router changes.");
    Ok(())
}
fn command(args: &[String]) -> Result<()> {
    if args.len() == 3 && args[1] == "demo" {
        return demo(Path::new(&args[2]));
    }
    if args.len() == 3 && args[1] == "processor-demo" {
        return processor_demo(Path::new(&args[2]));
    }
    if args.len() == 5 && matches!(args[1].as_str(), "terminal" | "terminal-uppercase") {
        let (mut store, client, graph) =
            terminal_setup(Path::new(&args[2]), args[1] == "terminal-uppercase")?;
        let id = store.submit_terminal(
            &client,
            &graph.pipeline,
            graph.version,
            &args[3],
            json!(args[4]),
        )?;
        eprintln!(
            "receipt {} (reuse operation ID only for the same input and graph version)",
            id.0
        );
        run_terminal_recipe(&mut store, &client, &mut std::io::stdout().lock())?;
        return Ok(());
    }
    if args.len() == 3 && args[1] == "terminal-run" {
        let (mut store, client, _) = terminal_setup(Path::new(&args[2]), false)?;
        run_terminal_recipe(&mut store, &client, &mut std::io::stdout().lock())?;
        return Ok(());
    }
    if args.len() == 4 && args[1] == "inspect" {
        let store = open(Path::new(&args[2]))?;
        let id = args[3].parse().map_err(|_| Error::Invalid("delivery ID"))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&store.inspect_processing(DeliveryId(id))?)?
        );
        return Ok(());
    }
    if args.len() == 4 && args[1] == "graph" {
        let store = open(Path::new(&args[2]))?;
        let version = args[3]
            .parse()
            .map_err(|_| Error::Invalid("graph version"))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&store.effective_graph("local", "terminal", version)?)?
        );
        return Ok(());
    }
    Err(Error::Invalid("usage: brook processor-demo <new-dir> | terminal[-uppercase] <dir> <operation-id> <text> | terminal-run <dir> | inspect <dir> <delivery-id> | graph <dir> <version> | demo <new-dir>"))
}
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = command(&args);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
