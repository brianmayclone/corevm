# CoreVM Documentation

This directory contains the full developer and user documentation for the CoreVM project.

## Structure

```
docs/
├── libcorevm/                 Core VM engine library
│   ├── overview.md            Architecture & design
│   ├── devices.md             Emulated hardware reference
│   ├── backends.md            Execution backends (KVM, anyOS)
│   ├── ffi.md                 C FFI reference
│   └── memory.md              Memory subsystem
├── apps/
│   ├── vmm-server/            Web backend server
│   │   ├── user-guide.md      Installation, configuration, usage
│   │   └── developer-guide.md Architecture, API internals, extending
│   ├── vmm-cluster/           Cluster orchestration
│   │   ├── user-guide.md      Deployment, DRS, HA, SDN, migration, notifications
│   │   └── developer-guide.md Engines, agent protocol, reconciler, SDN, notifier
│   ├── vmm-ui/                Web frontend
│   │   ├── user-guide.md      UI walkthrough, features
│   │   └── developer-guide.md Build, stack, component structure
│   ├── vmmanager/             Desktop GUI
│   │   ├── user-guide.md      Installation, VM management
│   │   └── developer-guide.md Architecture, UI framework, platform code
│   ├── vmctl/                 CLI tool (direct VM engine)
│   │   ├── user-guide.md      Commands, options, examples
│   │   └── developer-guide.md Architecture, extending
│   └── vmmctl/                Remote management CLI (REST API)
│       ├── user-guide.md      Setup, commands, scripting, TLS
│       └── developer-guide.md Architecture, adding commands
└── README.md                  This file
```

## Quick Links

| Topic | Link |
|-------|------|
| **Getting Started** | [vmm-server User Guide](apps/vmm-server/user-guide.md) |
| **Web UI** | [vmm-ui User Guide](apps/vmm-ui/user-guide.md) |
| **CLI (local)** | [vmctl User Guide](apps/vmctl/user-guide.md) |
| **CLI (remote)** | [vmmctl User Guide](apps/vmmctl/user-guide.md) |
| **Desktop App** | [vmmanager User Guide](apps/vmmanager/user-guide.md) |
| **Cluster Setup** | [vmm-cluster User Guide](apps/vmm-cluster/user-guide.md) |
| **SDN Networking** | [vmm-cluster User Guide — SDN](apps/vmm-cluster/user-guide.md#sdn-software-defined-networking) |
| **Storage Wizard** | [vmm-cluster User Guide — Storage Wizard](apps/vmm-cluster/user-guide.md#storage-wizard) |
| **Notifications** | [vmm-cluster User Guide — Notifications](apps/vmm-cluster/user-guide.md#notifications) |
| **LDAP / AD** | [vmm-cluster User Guide — LDAP](apps/vmm-cluster/user-guide.md#ldap--active-directory) |
| **VM Engine Internals** | [libcorevm Overview](libcorevm/overview.md) |
| **Hardware Devices** | [Device Reference](libcorevm/devices.md) |
| **C FFI** | [FFI Reference](libcorevm/ffi.md) |
