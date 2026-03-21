# CoreVM Documentation

This directory contains the full developer and user documentation for the CoreVM project.

## Structure

```
docs/
├── libcorevm/                 Core VM engine library
│   ├── overview.md            Architecture & design
│   ├── devices.md             Emulated hardware reference
│   ├── backends.md            Execution backends (KVM, WHP, anyOS)
│   ├── ffi.md                 C FFI reference
│   └── memory.md              Memory subsystem
├── apps/
│   ├── vmm-server/            Web backend server
│   │   ├── user-guide.md      Installation, configuration, usage
│   │   └── developer-guide.md Architecture, API internals, extending
│   ├── vmm-cluster/           Cluster orchestration
│   │   ├── user-guide.md      Deployment, node management, DRS
│   │   └── developer-guide.md Architecture, engines, agent protocol
│   ├── vmm-ui/                Web frontend
│   │   ├── user-guide.md      UI walkthrough, features
│   │   └── developer-guide.md Build, stack, component structure
│   ├── vmmanager/             Desktop GUI
│   │   ├── user-guide.md      Installation, VM management
│   │   └── developer-guide.md Architecture, UI framework, platform code
│   └── vmctl/                 CLI tool
│       ├── user-guide.md      Commands, options, examples
│       └── developer-guide.md Architecture, extending
└── README.md                  This file
```

## Quick Links

| Topic | Link |
|-------|------|
| **Getting Started** | [vmm-server User Guide](apps/vmm-server/user-guide.md) |
| **Web UI** | [vmm-ui User Guide](apps/vmm-ui/user-guide.md) |
| **CLI Usage** | [vmctl User Guide](apps/vmctl/user-guide.md) |
| **Desktop App** | [vmmanager User Guide](apps/vmmanager/user-guide.md) |
| **Cluster Setup** | [vmm-cluster User Guide](apps/vmm-cluster/user-guide.md) |
| **VM Engine Internals** | [libcorevm Overview](libcorevm/overview.md) |
| **Hardware Devices** | [Device Reference](libcorevm/devices.md) |
| **C FFI** | [FFI Reference](libcorevm/ffi.md) |
