//! CoreSAN Testbed — multi-node testing and chaos simulation.

mod cluster;
mod context;
mod db_init;
mod witness;
mod cli;
mod partition;
mod scenarios;

use context::TestContext;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("san_testbed=debug"))
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    // Check for --help
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("CoreSAN Testbed — multi-node testing and chaos simulation");
        println!();
        println!("Usage:");
        println!("  san-testbed [--nodes N]               Interactive mode with N nodes (default: 3)");
        println!("  san-testbed --scenario <name|all>     Run automated test scenario(s)");
        println!();
        println!("Scenarios:");
        println!("  Quorum:    quorum-degraded, quorum-fenced, quorum-recovery");
        println!("  Fencing:   fenced-write-denied, fenced-read-allowed");
        println!("  Failover:  leader-failover");
        println!("  Partition: partition-majority, partition-witness-2node");
        println!("  Repl:      replication-basic, replication-verify");
        println!("  Transfer:  transfer-small, transfer-large, transfer-throughput");
        println!("  Cross:     cross-node-read");
        println!("  Other:     repair-leader-only, all");
        return;
    }

    let num_nodes = args.iter()
        .position(|a| a == "--nodes")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3);

    let scenario = args.iter()
        .position(|a| a == "--scenario")
        .and_then(|i| args.get(i + 1))
        .cloned();

    println!("CoreSAN Testbed");
    println!("===============");

    if let Some(scenario_name) = scenario {
        // Run automated scenario(s)
        if scenario_name == "all" {
            let results = scenarios::run_all().await;
            scenarios::print_results(&results);
            let failed = results.iter().any(|r| !r.passed);
            std::process::exit(if failed { 1 } else { 0 });
        } else {
            match scenarios::run_single(&scenario_name).await {
                Some(result) => {
                    scenarios::print_results(&[result.clone()]);
                    std::process::exit(if result.passed { 0 } else { 1 });
                }
                None => {
                    eprintln!("Unknown scenario: {}", scenario_name);
                    eprintln!("Run with --help for available scenarios.");
                    std::process::exit(1);
                }
            }
        }
    } else {
        // Interactive mode
        println!("Starting {} nodes...\n", num_nodes);
        let mut ctx = match TestContext::new(num_nodes).await {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("Failed to start testbed: {}", e);
                std::process::exit(1);
            }
        };

        if let Err(e) = ctx.wait_all_healthy().await {
            eprintln!("Nodes not healthy: {}", e);
            std::process::exit(1);
        }

        cli::run(&mut ctx).await;
        ctx.shutdown();
    }
}
