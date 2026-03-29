# CoreSAN Documentation

CoreSAN is a distributed, software-defined storage system that aggregates physical disks across multiple nodes into a shared, replicated storage pool. It provides chunk-based file storage with configurable fault tolerance, automatic replication, self-healing, and transparent cross-node reads. Files are split into fixed-size chunks (default 64 MB) that are the unit of storage, replication, and repair.

## Documentation Index

| Document | Description |
|----------|-------------|
| [Architecture](architecture.md) | System design, components, data flow, replication model |
| [Installation & Setup](installation.md) | Prerequisites, installation, initial configuration |
| [Configuration Reference](configuration.md) | All configuration options with defaults and examples |
| [Administration Guide](administration.md) | Day-to-day operations: disks, volumes, backends, peers |
| [API Reference](api-reference.md) | Complete REST API documentation with request/response examples |
| [Cluster Integration](cluster-integration.md) | How CoreSAN integrates with vmm-cluster (proxy, witness, auto-discovery) |
| [Replication & Fault Tolerance](replication.md) | FTT, RAID modes, write leases, push replication, repair |
| [Monitoring & Benchmarking](monitoring.md) | Health checks, integrity verification, network benchmarks |
| [Troubleshooting](troubleshooting.md) | Common problems, diagnostics, recovery procedures |
| [Database Schema](database-schema.md) | Complete SQLite schema reference |
| [Testbed](testbed.md) | Automated test scenarios for validation |

## Quick Start

```bash
# 1. Start CoreSAN on a node
vmm-san --config /etc/vmm/vmm-san.toml

# 2. Claim a physical disk
curl -X POST https://localhost:7443/api/disks/claim \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'

# 3. Create a volume with 1 failure tolerance
curl -X POST https://localhost:7443/api/volumes \
  -H 'Content-Type: application/json' \
  -d '{"name": "my-volume", "ftt": 1, "local_raid": "stripe"}'

# 4. Write a file
curl -X PUT https://localhost:7443/api/volumes/my-volume-id/files/hello.txt \
  -d 'Hello, CoreSAN!'

# 5. Read it back (from any node in the cluster)
curl https://other-node:7443/api/volumes/my-volume-id/files/hello.txt
```

## Key Concepts

- **Node**: A server running the `vmm-san` daemon
- **Backend**: A mounted disk (claimed by CoreSAN) providing raw storage
- **Volume**: A logical storage pool spanning multiple backends across nodes
- **FTT (Failures To Tolerate)**: Number of node failures a volume can survive (0, 1, or 2)
- **Local RAID**: How data is distributed across disks within a single node (stripe, mirror, stripe_mirror)
- **Chunk**: A fixed-size block (default 64 MB) that is the unit of replication
- **Quorum**: Majority-based consensus to prevent split-brain
- **Witness**: External tie-breaker (vmm-cluster) for 2-node clusters
- **Leader**: Elected node responsible for repair operations (lowest node_id among online nodes)
