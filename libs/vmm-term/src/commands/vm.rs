use crate::registry::*;

// ── vm-list ──────────────────────────────────────────────────────────────

pub struct VmListCommand;

impl Command for VmListCommand {
    fn name(&self) -> &str { "vm-list" }
    fn description(&self) -> &str { "List all virtual machines" }
    fn usage(&self) -> &str { "vm-list" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        // The server injects __vm_list as pre-formatted text
        if let Some(data) = ctx.get("__vm_list") {
            Ok(data.lines().map(|l| {
                if l.starts_with("ID ") || l.starts_with("──") {
                    OutputLine::table_header(l)
                } else {
                    OutputLine::table_row(l)
                }
            }).collect())
        } else {
            Err(vec![OutputLine::stderr("Failed to retrieve VM list")])
        }
    }
}

// ── vm-start ─────────────────────────────────────────────────────────────

pub struct VmStartCommand;

impl Command for VmStartCommand {
    fn name(&self) -> &str { "vm-start" }
    fn description(&self) -> &str { "Start a virtual machine" }
    fn usage(&self) -> &str { "vm-start <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-start <vm-id|vm-name>")]);
        }
        // Server handles the actual start — we just pass the result
        let key = format!("__vm_start_{}", args[0]);
        match ctx.get(&key) {
            Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
            Some(msg) => Err(vec![OutputLine::stderr(msg)]),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}

// ── vm-stop ──────────────────────────────────────────────────────────────

pub struct VmStopCommand;

impl Command for VmStopCommand {
    fn name(&self) -> &str { "vm-stop" }
    fn description(&self) -> &str { "Stop a virtual machine (graceful shutdown)" }
    fn usage(&self) -> &str { "vm-stop <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-stop <vm-id|vm-name>")]);
        }
        let key = format!("__vm_stop_{}", args[0]);
        match ctx.get(&key) {
            Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
            Some(msg) => Err(vec![OutputLine::stderr(msg)]),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}

// ── vm-force-stop ────────────────────────────────────────────────────────

pub struct VmForceStopCommand;

impl Command for VmForceStopCommand {
    fn name(&self) -> &str { "vm-force-stop" }
    fn description(&self) -> &str { "Force stop a virtual machine (power off)" }
    fn usage(&self) -> &str { "vm-force-stop <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-force-stop <vm-id|vm-name>")]);
        }
        let key = format!("__vm_force_stop_{}", args[0]);
        match ctx.get(&key) {
            Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
            Some(msg) => Err(vec![OutputLine::stderr(msg)]),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}

// ── vm-restart ───────────────────────────────────────────────────────────

pub struct VmRestartCommand;

impl Command for VmRestartCommand {
    fn name(&self) -> &str { "vm-restart" }
    fn description(&self) -> &str { "Restart a virtual machine" }
    fn usage(&self) -> &str { "vm-restart <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-restart <vm-id|vm-name>")]);
        }
        let key = format!("__vm_restart_{}", args[0]);
        match ctx.get(&key) {
            Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
            Some(msg) => Err(vec![OutputLine::stderr(msg)]),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}

// ── vm-info ──────────────────────────────────────────────────────────────

pub struct VmInfoCommand;

impl Command for VmInfoCommand {
    fn name(&self) -> &str { "vm-info" }
    fn description(&self) -> &str { "Show detailed information about a VM" }
    fn usage(&self) -> &str { "vm-info <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-info <vm-id|vm-name>")]);
        }
        let key = format!("__vm_info_{}", args[0]);
        match ctx.get(&key) {
            Some(data) => Ok(data.lines().map(|l| OutputLine::stdout(l)).collect()),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}

// ── vm-delete ────────────────────────────────────────────────────────────

pub struct VmDeleteCommand;

impl Command for VmDeleteCommand {
    fn name(&self) -> &str { "vm-delete" }
    fn description(&self) -> &str { "Delete a virtual machine" }
    fn usage(&self) -> &str { "vm-delete <vm-id|vm-name>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: vm-delete <vm-id|vm-name>")]);
        }
        let key = format!("__vm_delete_{}", args[0]);
        match ctx.get(&key) {
            Some(msg) if msg.starts_with("OK:") => Ok(vec![OutputLine::success(&msg[3..])]),
            Some(msg) => Err(vec![OutputLine::stderr(msg)]),
            None => Err(vec![OutputLine::stderr(format!("VM '{}' not found", args[0]))]),
        }
    }
}
