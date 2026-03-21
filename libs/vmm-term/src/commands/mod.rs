//! Built-in commands that ship with vmm-term.

pub mod help;
pub mod clear;
pub mod echo;
pub mod vm;
pub mod status;
pub mod storage;
pub mod resources;
pub mod cluster;

use crate::registry::CommandRegistry;

/// Register all built-in commands.
pub fn register_builtins(registry: &mut CommandRegistry) {
    // Core
    registry.register(Box::new(help::HelpCommand));
    registry.register(Box::new(clear::ClearCommand));
    registry.register(Box::new(echo::EchoCommand));

    // VM management
    registry.register(Box::new(vm::VmListCommand));
    registry.register(Box::new(vm::VmStartCommand));
    registry.register(Box::new(vm::VmStopCommand));
    registry.register(Box::new(vm::VmForceStopCommand));
    registry.register(Box::new(vm::VmRestartCommand));
    registry.register(Box::new(vm::VmInfoCommand));
    registry.register(Box::new(vm::VmDeleteCommand));

    // Server status
    registry.register(Box::new(status::StatusCommand));
    registry.register(Box::new(status::UptimeCommand));
    registry.register(Box::new(status::WhoamiCommand));

    // Storage
    registry.register(Box::new(storage::PoolListCommand));
    registry.register(Box::new(storage::PoolCreateCommand));
    registry.register(Box::new(storage::PoolDeleteCommand));
    registry.register(Box::new(storage::PoolInfoCommand));
    registry.register(Box::new(storage::DiskListCommand));
    registry.register(Box::new(storage::DiskCreateCommand));
    registry.register(Box::new(storage::DiskDeleteCommand));
    registry.register(Box::new(storage::DiskResizeCommand));
    registry.register(Box::new(storage::IsoListCommand));

    // Resource groups
    registry.register(Box::new(resources::RgListCommand));
    registry.register(Box::new(resources::RgCreateCommand));
    registry.register(Box::new(resources::RgDeleteCommand));
    registry.register(Box::new(resources::RgInfoCommand));
    registry.register(Box::new(resources::RgAssignCommand));
    registry.register(Box::new(resources::RgPermsCommand));
}
