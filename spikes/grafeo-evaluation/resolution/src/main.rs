mod bounds;
mod query;
mod version;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("crash") {
        anyhow::ensure!(args.len() == 5, "crash needs fixture, carrier, phase");
        return version::crash_child(
            std::path::Path::new(&args[2]),
            args[3] == "native",
            args[4] == "after",
        );
    }
    if args.get(1).map(String::as_str) == Some("measure") {
        anyhow::ensure!(args.len() == 4, "measure needs fixture and carrier");
        println!(
            "{}",
            version::measure(std::path::Path::new(&args[2]), args[3] == "native")?
        );
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "query":query::probe()?, "bounded_executor":bounds::probe()?,
            "version_carriers":version::probe()?
        }))?
    );
    Ok(())
}
