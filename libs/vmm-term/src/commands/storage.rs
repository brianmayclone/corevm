use crate::registry::*;

// ── pool-list ────────────────────────────────────────────────────────────

pub struct PoolListCommand;

impl Command for PoolListCommand {
    fn name(&self) -> &str { "pool-list" }
    fn description(&self) -> &str { "List storage pools" }
    fn usage(&self) -> &str { "pool-list" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        ctx_table(ctx, "__pool_list")
    }
}

// ── pool-create ──────────────────────────────────────────────────────────

pub struct PoolCreateCommand;

impl Command for PoolCreateCommand {
    fn name(&self) -> &str { "pool-create" }
    fn description(&self) -> &str { "Create a new storage pool" }
    fn usage(&self) -> &str { "pool-create <name> <path> [--type local|nfs|gluster|ceph] [--shared]" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 2 {
            return Err(vec![OutputLine::stderr("Usage: pool-create <name> <path> [--type local|nfs|gluster|ceph] [--shared]")]);
        }
        ctx_result(ctx, "__pool_create")
    }
}

// ── pool-delete ──────────────────────────────────────────────────────────

pub struct PoolDeleteCommand;

impl Command for PoolDeleteCommand {
    fn name(&self) -> &str { "pool-delete" }
    fn description(&self) -> &str { "Delete a storage pool" }
    fn usage(&self) -> &str { "pool-delete <pool-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: pool-delete <pool-id>")]);
        }
        ctx_result(ctx, "__pool_delete")
    }
}

// ── pool-info ────────────────────────────────────────────────────────────

pub struct PoolInfoCommand;

impl Command for PoolInfoCommand {
    fn name(&self) -> &str { "pool-info" }
    fn description(&self) -> &str { "Show detailed information about a storage pool" }
    fn usage(&self) -> &str { "pool-info <pool-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: pool-info <pool-id>")]);
        }
        ctx_output(ctx, "__pool_info")
    }
}

// ── disk-list ────────────────────────────────────────────────────────────

pub struct DiskListCommand;

impl Command for DiskListCommand {
    fn name(&self) -> &str { "disk-list" }
    fn description(&self) -> &str { "List disk images" }
    fn usage(&self) -> &str { "disk-list [pool-id]" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        ctx_table(ctx, "__disk_list")
    }
}

// ── disk-create ──────────────────────────────────────────────────────────

pub struct DiskCreateCommand;

impl Command for DiskCreateCommand {
    fn name(&self) -> &str { "disk-create" }
    fn description(&self) -> &str { "Create a new disk image" }
    fn usage(&self) -> &str { "disk-create <name> <size-gb> <pool-id> [--format raw|qcow2]" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 3 {
            return Err(vec![OutputLine::stderr("Usage: disk-create <name> <size-gb> <pool-id> [--format raw|qcow2]")]);
        }
        ctx_result(ctx, "__disk_create")
    }
}

// ── disk-delete ──────────────────────────────────────────────────────────

pub struct DiskDeleteCommand;

impl Command for DiskDeleteCommand {
    fn name(&self) -> &str { "disk-delete" }
    fn description(&self) -> &str { "Delete a disk image" }
    fn usage(&self) -> &str { "disk-delete <disk-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: disk-delete <disk-id>")]);
        }
        ctx_result(ctx, "__disk_delete")
    }
}

// ── disk-resize ──────────────────────────────────────────────────────────

pub struct DiskResizeCommand;

impl Command for DiskResizeCommand {
    fn name(&self) -> &str { "disk-resize" }
    fn description(&self) -> &str { "Resize a disk image" }
    fn usage(&self) -> &str { "disk-resize <disk-id> <new-size-gb>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 2 {
            return Err(vec![OutputLine::stderr("Usage: disk-resize <disk-id> <new-size-gb>")]);
        }
        ctx_result(ctx, "__disk_resize")
    }
}

// ── iso-list ─────────────────────────────────────────────────────────────

pub struct IsoListCommand;

impl Command for IsoListCommand {
    fn name(&self) -> &str { "iso-list" }
    fn description(&self) -> &str { "List ISO images" }
    fn usage(&self) -> &str { "iso-list" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        ctx_table(ctx, "__iso_list")
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn ctx_table(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(data) => Ok(data.lines().map(|l| {
            if l.starts_with("ID ") || l.starts_with("──") || l.starts_with("NAME ") {
                OutputLine::table_header(l)
            } else {
                OutputLine::table_row(l)
            }
        }).collect()),
        None => Err(vec![OutputLine::stderr("Failed to retrieve data")]),
    }
}

fn ctx_result(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
        Some(msg) => Err(vec![OutputLine::stderr(msg)]),
        None => Err(vec![OutputLine::stderr("Operation failed — no response from server")]),
    }
}

fn ctx_output(ctx: &CommandContext, key: &str) -> CommandResult {
    match ctx.get(key) {
        Some(data) => Ok(data.lines().map(|l| OutputLine::stdout(l)).collect()),
        None => Err(vec![OutputLine::stderr("Not found")]),
    }
}
