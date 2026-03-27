# CoreSAN Testbed

The testbed (`san-testbed`) is an automated testing tool that spawns multiple vmm-san instances on localhost and runs predefined scenarios to validate cluster behavior.

## Building

```bash
cargo build -p san-testbed
# or
cargo build -p san-testbed --release
```

## Usage

```bash
# Run a specific scenario
san-testbed --scenario quorum-degraded

# Run ALL scenarios
san-testbed --scenario all

# Interactive mode with N nodes (no automated test)
san-testbed --nodes 3

# Show help
san-testbed --help
```

## How It Works

1. Creates a temporary directory with per-node config files and data directories
2. Starts N vmm-san instances on ports 7443, 7444, 7445, ...
3. Starts a witness mock on port 9443 (configurable behavior)
4. Pre-initializes each node's database (volumes, backends, peer registrations)
5. Cross-registers backends so push replication works between nodes
6. Waits for all nodes to become healthy
7. Runs the scenario logic
8. Reports PASS/FAIL with timing

## Test Scenarios

### Quorum Tests (3 nodes)

#### quorum-degraded
**Tests:** Quorum survives loss of one node in a 3-node cluster.

1. Kill node 3
2. Wait for nodes 1 & 2 to detect failure
3. Verify: quorum = `degraded` (not fenced)
4. Verify: exactly one node is leader

#### quorum-fenced
**Tests:** Single node gets fenced when it loses all peers and witness denies.

1. Set witness to deny all requests
2. Kill nodes 2 and 3
3. Wait for node 1 to detect failures
4. Verify: node 1 quorum = `fenced`
5. Verify: node 1 is NOT leader

#### quorum-recovery
**Tests:** Fenced node recovers when peers come back.

1. Fence node 1 (kill peers, deny witness)
2. Verify: node 1 is fenced
3. Restart node 2
4. Wait for recovery
5. Verify: node 1 quorum = `degraded` or `active`

### Fencing Tests (3 nodes)

#### fenced-write-denied
**Tests:** Fenced nodes reject writes.

1. Fence node 1
2. Attempt to write a file on node 1
3. Verify: HTTP 503 (Service Unavailable)

#### fenced-read-allowed
**Tests:** Fenced nodes can still read existing data.

1. Write a file on node 1 while healthy
2. Fence node 1
3. Read the file on node 1
4. Verify: HTTP 200 with correct data

### Failover Tests (3 nodes)

#### leader-failover
**Tests:** New leader is elected when current leader crashes.

1. Identify current leader
2. Kill the leader node
3. Wait for remaining nodes to detect failure
4. Verify: a new leader is elected among remaining nodes

### Partition Tests

#### partition-majority
**Tests:** Network partition isolates minority.

1. Deny witness for all requests
2. Partition: nodes 1+2 can reach each other, node 3 isolated
3. Verify: nodes 1+2 are `degraded` (majority partition)
4. Verify: node 3 is `fenced` (minority partition)

#### partition-witness-2node
**Tests:** Witness tie-breaking in 2-node cluster.

1. Configure 2-node cluster
2. Partition: node 1 and node 2 cannot reach each other
3. Witness allows only the node with lower host_id
4. Verify: one node `degraded`, other node `fenced`

### Replication Tests (3 nodes)

#### replication-basic
**Tests:** Files replicate across nodes.

1. Write a file on node 1
2. Wait for push replication
3. Read file from node 2
4. Verify: data matches

#### replication-verify
**Tests:** Bidirectional replication and cross-reads.

1. Write file A on node 1
2. Write file B on node 2
3. Wait for replication
4. Read file A from all 3 nodes — verify content matches
5. Read file B from all 3 nodes — verify content matches

### Repair Tests (3 nodes)

#### repair-leader-only
**Tests:** Only the leader runs repair operations.

1. Create under-replicated data
2. Verify: leader node repairs chunks
3. Verify: non-leader nodes skip repair

### File Transfer Tests (3 nodes)

#### transfer-small
**Tests:** Small file write/read with timing.

1. Write 1 KB file on node 1, measure time
2. Read back from node 1, measure time
3. Report: write latency, read latency, throughput

#### transfer-large
**Tests:** Large file integrity verification.

1. Generate 512 KB deterministic byte pattern
2. Write on node 1
3. Read back and compare byte-by-byte
4. Verify: SHA256 matches, no corruption

#### transfer-throughput
**Tests:** Aggregate throughput measurement.

1. Write 10 × 64 KB files sequentially, measure total time
2. Read all 10 files back, measure total time
3. Report: aggregate write MB/s, aggregate read MB/s
4. Thresholds: write > 0.1 MB/s, read > 0.5 MB/s (debug builds)

### Cross-Node Tests (3 nodes)

#### cross-node-read
**Tests:** Reading files that only exist on another node.

1. Write 4 KB file on node 1
2. Wait until node 2 can read it (poll with retry)
3. Wait until node 3 can read it
4. Verify: content matches on all nodes
5. Report: time until each node could read

## Witness Mock

The testbed includes a built-in witness server on port 9443 that supports configurable behavior:

| Mode | Behavior |
|------|----------|
| `AllowAll` | Always grants quorum to requesting node |
| `DenyAll` | Always denies quorum |
| `AllowNode(id)` | Grants quorum only to specified node |

Scenarios switch witness mode dynamically to simulate different failure conditions.

## Test Output

```
CoreSAN Testbed
===============
[INFO] ━━━ Scenario: quorum-degraded (3 nodes) ━━━
[INFO] Using vmm-san binary: /path/to/vmm-san
[INFO] Started node 1 (port 7443)
[INFO] Started node 2 (port 7444)
[INFO] Started node 3 (port 7445)
[DEBUG] All nodes healthy, running scenario logic...
[INFO] Killed node 3
[DEBUG] Waiting for quorum change...
[INFO] Node 1: quorum=degraded, leader=true
[INFO] Node 2: quorum=degraded, leader=false
[DEBUG] Scenario logic completed successfully

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ quorum-degraded      PASS  (3.2s)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Troubleshooting Testbed

### Port Already in Use

```
Cannot bind witness to 127.0.0.1:9443: Address already in use
```

A previous testbed run left processes behind:
```bash
pkill -f "vmm-san|san-testbed"
# Wait a few seconds for ports to release
```

### Binary Not Found

The testbed searches for `vmm-san` in these locations (in order):
1. `/tmp/corevm-target/debug/vmm-san`
2. `target/debug/vmm-san`
3. `../../target/debug/vmm-san`
4. `/tmp/cargo-build-san/debug/vmm-san`

If not found, it attempts `cargo build -p vmm-san` automatically.

### Slow Tests in Debug Mode

Debug builds are significantly slower. Transfer throughput tests use relaxed thresholds for debug builds. For production-like performance testing, use release builds:

```bash
cargo build -p vmm-san -p san-testbed --release
```
