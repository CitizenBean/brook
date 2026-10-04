#[path = "../tests/support/simulation.rs"]
mod simulation;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: cargo run --example simulate -- <seed|replay.json> <fresh-store-dir> <trace.json>".into());
    }
    let path = std::path::Path::new(&args[2]);
    if path.join("brook.sqlite3").exists() {
        return Err("simulation requires a fresh store directory".into());
    }
    let plan = match args[1].parse::<u64>() {
        Ok(seed) => simulation::plan(seed),
        Err(_) => serde_json::from_slice::<simulation::Trace>(&std::fs::read(&args[1])?)?.plan,
    };
    let trace = simulation::run(path, plan, Some(std::path::Path::new(&args[3])))?;
    if let Some(failure) = trace.failure {
        return Err(format!("seed {}: {failure}; replay {}", trace.plan.seed, args[3]).into());
    }
    println!(
        "PASS host simulation seed {}: {} actions; trace {}",
        trace.plan.seed,
        trace.frames.len(),
        args[3]
    );
    Ok(())
}
