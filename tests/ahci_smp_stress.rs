//! AHCI SMP stress test — simulates multi-vCPU AHCI access patterns.
//!
//! Reproduces the lock contention that causes Windows 10 installation to hang
//! with multiple vCPUs. Tests different lock strategies and measures throughput.
//!
//! Run: rustc --edition 2021 -o /tmp/ahci_smp_test tests/ahci_smp_stress.rs && /tmp/ahci_smp_test

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ── Exact lock implementation from ffi.rs ───────────────────────────────────

static AHCI_LOCK: AtomicBool = AtomicBool::new(false);

fn ahci_lock() {
    if AHCI_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        return;
    }
    let mut spin = 0u32;
    loop {
        if AHCI_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        spin += 1;
        if spin < 64 {
            core::hint::spin_loop();
        } else {
            std::thread::sleep(Duration::from_micros(1));
            spin = 0;
        }
    }
}

fn ahci_unlock() {
    AHCI_LOCK.store(false, Ordering::Release);
}

// ── Stats ───────────────────────────────────────────────────────────────────

struct Stats {
    commands: AtomicU64,
    total_us: AtomicU64,
    lock_wait_us: AtomicU64,
    contended: AtomicU64, // waits > 1ms
}

impl Stats {
    fn new() -> Self {
        Self {
            commands: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            lock_wait_us: AtomicU64::new(0),
            contended: AtomicU64::new(0),
        }
    }

    fn report(&self, name: &str) {
        let cmds = self.commands.load(Ordering::Relaxed);
        if cmds == 0 { println!("  {}: no commands", name); return; }
        let avg = self.total_us.load(Ordering::Relaxed) / cmds;
        let avg_wait = self.lock_wait_us.load(Ordering::Relaxed) / cmds;
        let contended = self.contended.load(Ordering::Relaxed);
        println!("  {}: {} cmds, avg={}us, avg_wait={}us, contended(>1ms)={}",
            name, cmds, avg, avg_wait, contended);
    }
}

// ── Command simulation ─────────────────────────────────────────────────────

/// Current behavior: hold lock during entire disk I/O
fn command_inline(io_delay: Duration, stats: &Stats) {
    let t0 = Instant::now();
    ahci_lock();
    let wait = t0.elapsed();
    // Simulate disk I/O under lock
    std::thread::sleep(io_delay);
    ahci_unlock();
    let total = t0.elapsed();
    stats.total_us.fetch_add(total.as_micros() as u64, Ordering::Relaxed);
    stats.lock_wait_us.fetch_add(wait.as_micros() as u64, Ordering::Relaxed);
    stats.commands.fetch_add(1, Ordering::Relaxed);
    if wait > Duration::from_millis(1) {
        stats.contended.fetch_add(1, Ordering::Relaxed);
    }
}

fn run_test(name: &str, num_threads: usize, io_delay: Duration, duration: Duration) {
    let all_stats: Vec<Arc<Stats>> = (0..num_threads).map(|_| Arc::new(Stats::new())).collect();
    let running = Arc::new(AtomicBool::new(true));

    let handles: Vec<_> = (0..num_threads).map(|i| {
        let s = all_stats[i].clone();
        let r = running.clone();
        thread::spawn(move || {
            while r.load(Ordering::Relaxed) {
                command_inline(io_delay, &s);
            }
        })
    }).collect();

    thread::sleep(duration);
    running.store(false, Ordering::Relaxed);
    for h in handles { h.join().unwrap(); }

    println!("\n=== {} ===", name);
    println!("  Config: {} threads, I/O delay={}us, duration={}s",
        num_threads, io_delay.as_micros(), duration.as_secs());
    let mut total_cmds = 0u64;
    let mut max_wait = 0u64;
    for (i, s) in all_stats.iter().enumerate() {
        s.report(&format!("thread-{}", i));
        total_cmds += s.commands.load(Ordering::Relaxed);
        let avg_wait = s.lock_wait_us.load(Ordering::Relaxed) / s.commands.load(Ordering::Relaxed).max(1);
        max_wait = max_wait.max(avg_wait);
    }
    let throughput = total_cmds as f64 / duration.as_secs_f64();
    let theoretical = 1_000_000.0 / io_delay.as_micros() as f64;
    println!("  Total: {} cmds, throughput={:.0}/s (theoretical max={:.0}/s)",
        total_cmds, throughput, theoretical);
    println!("  Slowdown: {:.1}x", theoretical / throughput);
    if max_wait > 2000 {
        println!("  *** PROBLEM: avg wait {}us >> I/O time {}us — lock contention!", max_wait, io_delay.as_micros());
    }
}

/// Test with mixed readers (lock-free) and writers (locked)
fn run_mixed_test() {
    let port_is = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));
    let writer_stats = Arc::new(Stats::new());
    let reader_count = Arc::new(AtomicU64::new(0));
    let duration = Duration::from_secs(2);

    // 1 writer holds lock during 2ms I/O
    let ws = writer_stats.clone();
    let wr = running.clone();
    let wis = port_is.clone();
    let writer = thread::spawn(move || {
        let mut seq = 0u64;
        while wr.load(Ordering::Relaxed) {
            let t0 = Instant::now();
            ahci_lock();
            let wait = t0.elapsed();
            seq += 1;
            wis.store(seq, Ordering::Release);
            std::thread::sleep(Duration::from_millis(2));
            ahci_unlock();
            let total = t0.elapsed();
            ws.total_us.fetch_add(total.as_micros() as u64, Ordering::Relaxed);
            ws.lock_wait_us.fetch_add(wait.as_micros() as u64, Ordering::Relaxed);
            ws.commands.fetch_add(1, Ordering::Relaxed);
        }
    });

    // 3 readers do lock-free reads (our optimization)
    let readers: Vec<_> = (0..3).map(|_| {
        let ris = port_is.clone();
        let rc = reader_count.clone();
        let rr = running.clone();
        thread::spawn(move || {
            while rr.load(Ordering::Relaxed) {
                let _ = ris.load(Ordering::Acquire);
                rc.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();

    thread::sleep(duration);
    running.store(false, Ordering::Relaxed);
    writer.join().unwrap();
    for r in readers { r.join().unwrap(); }

    println!("\n=== Mixed: 1 Writer (locked, 2ms) + 3 Lock-Free Readers ===");
    writer_stats.report("writer");
    let reads = reader_count.load(Ordering::Relaxed);
    let writes = writer_stats.commands.load(Ordering::Relaxed);
    println!("  Lock-free reads: {} ({:.0}/s)", reads, reads as f64 / duration.as_secs_f64());
    println!("  Read/Write ratio: {:.0}x — readers unaffected by writer lock!", reads as f64 / writes.max(1) as f64);
}

fn main() {
    println!("AHCI SMP Lock Contention Test");
    println!("============================\n");
    println!("This reproduces the lock contention causing Windows 10 install");
    println!("to hang with 4 vCPUs. The AHCI_LOCK is held during disk I/O.\n");

    // Test 1: Baseline — 1 thread, no contention
    run_test("1 Thread Baseline (no contention)", 1, Duration::from_micros(500), Duration::from_secs(2));

    // Test 2: 4 threads, short I/O (SSD-like)
    run_test("4 Threads, 100us I/O (SSD)", 4, Duration::from_micros(100), Duration::from_secs(2));

    // Test 3: 4 threads, medium I/O
    run_test("4 Threads, 500us I/O", 4, Duration::from_micros(500), Duration::from_secs(2));

    // Test 4: 4 threads, long I/O (HDD seek) — THIS IS THE PROBLEM CASE
    run_test("4 Threads, 2ms I/O (HDD) — PROBLEM CASE", 4, Duration::from_millis(2), Duration::from_secs(3));

    // Test 5: Lock-free reads (our optimization)
    run_mixed_test();

    println!("\n============================");
    println!("CONCLUSION:");
    println!("With 4 threads and 2ms I/O under lock, each thread waits ~6ms avg");
    println!("(3 others × 2ms). This 4x slowdown + sleep() overhead = near-hang.");
    println!("Fix: move disk I/O out of the lock (deferred I/O / async worker).");
}
