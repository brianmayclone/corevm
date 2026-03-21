//! Cluster management commands for the vmm-cluster terminal.

use crate::registry::*;

// ── Helper: read pre-computed table or result from context ──────────────

fn ctx_table(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(data) => Ok(data.lines().map(|l| {
            if l.starts_with("──") || l.contains("NAME") && l.contains("STATUS") || l.starts_with("ID ") {
                OutputLine::table_header(l)
            } else {
                OutputLine::table_row(l)
            }
        }).collect()),
        None => Err(vec![OutputLine::stderr("No data available")]),
    }
}

fn ctx_result(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
        Some(msg) => Err(vec![OutputLine::stderr(msg)]),
        None => Err(vec![OutputLine::stderr("Operation failed")]),
    }
}

fn ctx_output(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(data) => Ok(data.lines().map(|l| OutputLine::stdout(l)).collect()),
        None => Err(vec![OutputLine::stderr("No data available")]),
    }
}

// ── cluster-status ──────────────────────────────────────────────────────

pub struct ClusterStatusCommand;
impl Command for ClusterStatusCommand {
    fn name(&self) -> &str { "cluster-status" }
    fn description(&self) -> &str { "Show cluster overview (hosts, VMs, resources)" }
    fn usage(&self) -> &str { "cluster-status" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_output(ctx, "__cluster_status") }
}

// ── cluster-list ────────────────────────────────────────────────────────

pub struct ClusterListCommand;
impl Command for ClusterListCommand {
    fn name(&self) -> &str { "cluster-list" }
    fn description(&self) -> &str { "List all clusters" }
    fn usage(&self) -> &str { "cluster-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__cluster_list") }
}

// ── cluster-create ──────────────────────────────────────────────────────

pub struct ClusterCreateCommand;
impl Command for ClusterCreateCommand {
    fn name(&self) -> &str { "cluster-create" }
    fn description(&self) -> &str { "Create a new cluster" }
    fn usage(&self) -> &str { "cluster-create <name> [description]" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: cluster-create <name> [description]")]); }
        ctx_result(ctx, &format!("__cluster_create_{}", args[0]))
    }
}

// ── cluster-delete ──────────────────────────────────────────────────────

pub struct ClusterDeleteCommand;
impl Command for ClusterDeleteCommand {
    fn name(&self) -> &str { "cluster-delete" }
    fn description(&self) -> &str { "Delete a cluster" }
    fn usage(&self) -> &str { "cluster-delete <cluster-id|name>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: cluster-delete <cluster-id|name>")]); }
        ctx_result(ctx, &format!("__cluster_delete_{}", args[0]))
    }
}

// ── cluster-info ────────────────────────────────────────────────────────

pub struct ClusterInfoCommand;
impl Command for ClusterInfoCommand {
    fn name(&self) -> &str { "cluster-info" }
    fn description(&self) -> &str { "Show details of a cluster" }
    fn usage(&self) -> &str { "cluster-info <cluster-id|name>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: cluster-info <cluster-id|name>")]); }
        ctx_output(ctx, &format!("__cluster_info_{}", args[0]))
    }
}

// ── host-list ───────────────────────────────────────────────────────────

pub struct HostListCommand;
impl Command for HostListCommand {
    fn name(&self) -> &str { "host-list" }
    fn description(&self) -> &str { "List all registered hosts" }
    fn usage(&self) -> &str { "host-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__host_list") }
}

// ── host-info ───────────────────────────────────────────────────────────

pub struct HostInfoCommand;
impl Command for HostInfoCommand {
    fn name(&self) -> &str { "host-info" }
    fn description(&self) -> &str { "Show host details and resource usage" }
    fn usage(&self) -> &str { "host-info <host-id|hostname>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: host-info <host-id|hostname>")]); }
        ctx_output(ctx, &format!("__host_info_{}", args[0]))
    }
}

// ── host-maintenance ────────────────────────────────────────────────────

pub struct HostMaintenanceCommand;
impl Command for HostMaintenanceCommand {
    fn name(&self) -> &str { "host-maintenance" }
    fn description(&self) -> &str { "Enter or exit maintenance mode on a host" }
    fn usage(&self) -> &str { "host-maintenance <host-id|hostname> <enter|exit>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 2 { return Err(vec![OutputLine::stderr("Usage: host-maintenance <host> <enter|exit>")]); }
        ctx_result(ctx, &format!("__host_maintenance_{}_{}", args[0], args[1]))
    }
}

// ── host-remove ─────────────────────────────────────────────────────────

pub struct HostRemoveCommand;
impl Command for HostRemoveCommand {
    fn name(&self) -> &str { "host-remove" }
    fn description(&self) -> &str { "Remove (deregister) a host from the cluster" }
    fn usage(&self) -> &str { "host-remove <host-id|hostname>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: host-remove <host-id|hostname>")]); }
        ctx_result(ctx, &format!("__host_remove_{}", args[0]))
    }
}

// ── datastore-list ──────────────────────────────────────────────────────

pub struct DatastoreListCommand;
impl Command for DatastoreListCommand {
    fn name(&self) -> &str { "datastore-list" }
    fn description(&self) -> &str { "List all cluster-wide datastores" }
    fn usage(&self) -> &str { "datastore-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__datastore_list") }
}

// ── datastore-info ──────────────────────────────────────────────────────

pub struct DatastoreInfoCommand;
impl Command for DatastoreInfoCommand {
    fn name(&self) -> &str { "datastore-info" }
    fn description(&self) -> &str { "Show datastore details and mount status per host" }
    fn usage(&self) -> &str { "datastore-info <datastore-id|name>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: datastore-info <datastore-id|name>")]); }
        ctx_output(ctx, &format!("__datastore_info_{}", args[0]))
    }
}

// ── vm-migrate ──────────────────────────────────────────────────────────

pub struct VmMigrateCommand;
impl Command for VmMigrateCommand {
    fn name(&self) -> &str { "vm-migrate" }
    fn description(&self) -> &str { "Migrate a VM to another host (cold migration)" }
    fn usage(&self) -> &str { "vm-migrate <vm-id|name> <target-host-id|hostname>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 2 { return Err(vec![OutputLine::stderr("Usage: vm-migrate <vm> <target-host>")]); }
        ctx_result(ctx, &format!("__vm_migrate_{}_{}", args[0], args[1]))
    }
}

// ── drs-list ────────────────────────────────────────────────────────────

pub struct DrsListCommand;
impl Command for DrsListCommand {
    fn name(&self) -> &str { "drs-list" }
    fn description(&self) -> &str { "List pending DRS recommendations" }
    fn usage(&self) -> &str { "drs-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__drs_list") }
}

// ── drs-apply ───────────────────────────────────────────────────────────

pub struct DrsApplyCommand;
impl Command for DrsApplyCommand {
    fn name(&self) -> &str { "drs-apply" }
    fn description(&self) -> &str { "Apply a DRS recommendation (trigger migration)" }
    fn usage(&self) -> &str { "drs-apply <recommendation-id>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: drs-apply <id>")]); }
        ctx_result(ctx, &format!("__drs_apply_{}", args[0]))
    }
}

// ── drs-dismiss ─────────────────────────────────────────────────────────

pub struct DrsDismissCommand;
impl Command for DrsDismissCommand {
    fn name(&self) -> &str { "drs-dismiss" }
    fn description(&self) -> &str { "Dismiss a DRS recommendation" }
    fn usage(&self) -> &str { "drs-dismiss <recommendation-id>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: drs-dismiss <id>")]); }
        ctx_result(ctx, &format!("__drs_dismiss_{}", args[0]))
    }
}

// ── task-list ───────────────────────────────────────────────────────────

pub struct TaskListCommand;
impl Command for TaskListCommand {
    fn name(&self) -> &str { "task-list" }
    fn description(&self) -> &str { "List recent tasks (migrations, HA restarts, etc.)" }
    fn usage(&self) -> &str { "task-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__task_list") }
}

// ── event-list ──────────────────────────────────────────────────────────

pub struct EventListCommand;
impl Command for EventListCommand {
    fn name(&self) -> &str { "event-list" }
    fn description(&self) -> &str { "Show recent cluster events" }
    fn usage(&self) -> &str { "event-list [limit]" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__event_list") }
}

// ── alarm-list ──────────────────────────────────────────────────────────

pub struct AlarmListCommand;
impl Command for AlarmListCommand {
    fn name(&self) -> &str { "alarm-list" }
    fn description(&self) -> &str { "Show active alarms" }
    fn usage(&self) -> &str { "alarm-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__alarm_list") }
}

// ── alarm-ack ───────────────────────────────────────────────────────────

pub struct AlarmAckCommand;
impl Command for AlarmAckCommand {
    fn name(&self) -> &str { "alarm-ack" }
    fn description(&self) -> &str { "Acknowledge an alarm" }
    fn usage(&self) -> &str { "alarm-ack <alarm-id>" }
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() { return Err(vec![OutputLine::stderr("Usage: alarm-ack <id>")]); }
        ctx_result(ctx, &format!("__alarm_ack_{}", args[0]))
    }
}

// ── services ────────────────────────────────────────────────────────────

pub struct ServiceListCommand;
impl Command for ServiceListCommand {
    fn name(&self) -> &str { "service-list" }
    fn description(&self) -> &str { "List all cluster services and their status" }
    fn usage(&self) -> &str { "service-list" }
    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult { ctx_table(ctx, "__service_list") }
}

// ── Registration ────────────────────────────────────────────────────────

/// Register all cluster management commands.
/// Called by vmm-cluster's terminal handler (NOT by vmm-server).
pub fn register_cluster_commands(registry: &mut crate::registry::CommandRegistry) {
    registry.register(Box::new(ClusterStatusCommand));
    registry.register(Box::new(ClusterListCommand));
    registry.register(Box::new(ClusterCreateCommand));
    registry.register(Box::new(ClusterDeleteCommand));
    registry.register(Box::new(ClusterInfoCommand));
    registry.register(Box::new(HostListCommand));
    registry.register(Box::new(HostInfoCommand));
    registry.register(Box::new(HostMaintenanceCommand));
    registry.register(Box::new(HostRemoveCommand));
    registry.register(Box::new(DatastoreListCommand));
    registry.register(Box::new(DatastoreInfoCommand));
    registry.register(Box::new(VmMigrateCommand));
    registry.register(Box::new(DrsListCommand));
    registry.register(Box::new(DrsApplyCommand));
    registry.register(Box::new(DrsDismissCommand));
    registry.register(Box::new(TaskListCommand));
    registry.register(Box::new(EventListCommand));
    registry.register(Box::new(AlarmListCommand));
    registry.register(Box::new(AlarmAckCommand));
    registry.register(Box::new(ServiceListCommand));
}
