//! Interactive CLI command loop.

use crate::context::TestContext;
use crate::witness::WitnessMode;
use rustyline::DefaultEditor;

pub async fn run(ctx: &mut TestContext) {
    let mut rl = DefaultEditor::new().unwrap();
    let mut running = 0;
    for n in &mut ctx.nodes { if n.is_running() { running += 1; } }
    println!("\nCoreSAN Testbed — {} nodes running. Type 'help' for commands.\n", running);

    loop {
        let line = match rl.readline("san-testbed> ") {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() { continue; }
        rl.add_history_entry(line).ok();

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "help" => print_help(),
            "status" => cmd_status(ctx).await,
            "kill" if parts.len() >= 2 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    ctx.kill_node(n).await;
                    println!("Node {} killed", n);
                }
            }
            "start" if parts.len() >= 2 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    match ctx.start_node(n).await {
                        Ok(_) => println!("Node {} started", n),
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "partition" => {
                if let Some((a, b)) = parse_partition(&parts[1..]) {
                    match ctx.partition(&a, &b).await {
                        Ok(_) => println!("Partition applied: {:?} vs {:?}", a, b),
                        Err(e) => println!("Error: {}", e),
                    }
                } else {
                    println!("Usage: partition 1,2 vs 3");
                }
            }
            "heal" => {
                match ctx.heal().await {
                    Ok(_) => println!("All partitions healed"),
                    Err(e) => println!("Error: {}", e),
                }
            }
            "write" if parts.len() >= 5 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    let content = parts[4..].join(" ");
                    match ctx.write_file(n, parts[2], parts[3], content.as_bytes()).await {
                        Ok(status) => println!("HTTP {}", status),
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "read" if parts.len() >= 4 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    match ctx.read_file(n, parts[2], parts[3]).await {
                        Ok((status, body)) => {
                            println!("HTTP {} — {} bytes", status, body.len());
                            if let Ok(s) = String::from_utf8(body) {
                                println!("{}", s);
                            }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "volumes" => {
                let mut vol_url = None;
                for node in &mut ctx.nodes {
                    if node.is_running() {
                        vol_url = Some(format!("{}/api/volumes", node.address()));
                        break;
                    }
                }
                if let Some(url) = vol_url {
                    match ctx.http.get(&url).send().await {
                        Ok(resp) => {
                            if let Ok(body) = resp.text().await {
                                println!("{}", body);
                            }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                } else {
                    println!("No running nodes");
                }
            }
            "witness" if parts.len() >= 2 => {
                match parts[1] {
                    "allow-all" => { ctx.set_witness_mode(WitnessMode::AllowAll); println!("Witness: allow-all"); }
                    "deny-all" => { ctx.set_witness_mode(WitnessMode::DenyAll); println!("Witness: deny-all"); }
                    "smart" => { ctx.set_witness_mode(WitnessMode::Smart); println!("Witness: smart"); }
                    "off" => { ctx.set_witness_mode(WitnessMode::Off); println!("Witness: off"); }
                    _ => println!("Usage: witness allow-all|deny-all|smart|off"),
                }
            }
            "wait" if parts.len() >= 2 => {
                if let Ok(secs) = parts[1].parse::<u64>() {
                    println!("Waiting {}s...", secs);
                    ctx.wait_secs(secs).await;
                }
            }
            "exit" | "quit" => break,
            _ => println!("Unknown command. Type 'help'."),
        }
    }
}

fn print_help() {
    println!("Commands:");
    println!("  status                        Show all node status");
    println!("  kill <n>                      Stop node N");
    println!("  start <n>                     Start node N");
    println!("  partition <a,b> vs <c,d>      Simulate network partition");
    println!("  heal                          Remove all partitions");
    println!("  write <n> <vol> <path> <data> Write file to node");
    println!("  read <n> <vol> <path>         Read file from node");
    println!("  volumes                       List volumes");
    println!("  witness allow-all|deny-all|smart|off  Control mock witness");
    println!("  wait <secs>                   Wait N seconds");
    println!("  exit                          Shutdown and exit");
}

fn parse_partition(parts: &[&str]) -> Option<(Vec<usize>, Vec<usize>)> {
    let joined = parts.join(" ");
    let sides: Vec<&str> = joined.split(" vs ").collect();
    if sides.len() != 2 { return None; }

    let a: Vec<usize> = sides[0].split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let b: Vec<usize> = sides[1].split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if a.is_empty() || b.is_empty() { return None; }
    Some((a, b))
}

async fn cmd_status(ctx: &mut TestContext) {
    // Collect node info first to avoid borrow conflicts
    let mut node_info: Vec<(usize, u16, bool)> = Vec::new();
    for node in &mut ctx.nodes {
        let running = node.is_running();
        node_info.push((node.index, node.port, running));
    }
    for (idx, port, running) in &node_info {
        let state_str = if *running { "RUNNING" } else { "STOPPED" };
        if !running {
            println!("  Node {} (port {}): {}", idx, port, state_str);
            continue;
        }
        match ctx.get_status(*idx).await {
            Ok(s) => {
                let leader = if s.is_leader { " LEADER" } else { "" };
                println!("  Node {} (port {}): {}  quorum={}  peers={}{}",
                    idx, port, state_str, s.quorum_status, s.peer_count, leader);
            }
            Err(e) => {
                println!("  Node {} (port {}): {}  (error: {})", idx, port, state_str, e);
            }
        }
    }
    let wmode = ctx.witness.mode.read().unwrap();
    println!("  Witness (port 9443): {:?}", *wmode);
}
