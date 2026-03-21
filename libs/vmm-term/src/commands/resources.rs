use crate::registry::*;

// ── rg-list ──────────────────────────────────────────────────────────────

pub struct RgListCommand;

impl Command for RgListCommand {
    fn name(&self) -> &str { "rg-list" }
    fn description(&self) -> &str { "List resource groups" }
    fn usage(&self) -> &str { "rg-list" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        ctx_table(ctx, "__rg_list")
    }
}

// ── rg-create ────────────────────────────────────────────────────────────

pub struct RgCreateCommand;

impl Command for RgCreateCommand {
    fn name(&self) -> &str { "rg-create" }
    fn description(&self) -> &str { "Create a new resource group" }
    fn usage(&self) -> &str { "rg-create <name> [description]" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: rg-create <name> [description]")]);
        }
        ctx_result(ctx, "__rg_create")
    }
}

// ── rg-delete ────────────────────────────────────────────────────────────

pub struct RgDeleteCommand;

impl Command for RgDeleteCommand {
    fn name(&self) -> &str { "rg-delete" }
    fn description(&self) -> &str { "Delete a resource group (VMs move to default)" }
    fn usage(&self) -> &str { "rg-delete <rg-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: rg-delete <rg-id>")]);
        }
        ctx_result(ctx, "__rg_delete")
    }
}

// ── rg-info ──────────────────────────────────────────────────────────────

pub struct RgInfoCommand;

impl Command for RgInfoCommand {
    fn name(&self) -> &str { "rg-info" }
    fn description(&self) -> &str { "Show resource group details and permissions" }
    fn usage(&self) -> &str { "rg-info <rg-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: rg-info <rg-id>")]);
        }
        ctx_output(ctx, "__rg_info")
    }
}

// ── rg-assign ────────────────────────────────────────────────────────────

pub struct RgAssignCommand;

impl Command for RgAssignCommand {
    fn name(&self) -> &str { "rg-assign" }
    fn description(&self) -> &str { "Assign a VM to a resource group" }
    fn usage(&self) -> &str { "rg-assign <vm-id|vm-name> <rg-id>" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.len() < 2 {
            return Err(vec![OutputLine::stderr("Usage: rg-assign <vm-id|vm-name> <rg-id>")]);
        }
        ctx_result(ctx, "__rg_assign")
    }
}

// ── rg-perms ─────────────────────────────────────────────────────────────

pub struct RgPermsCommand;

impl Command for RgPermsCommand {
    fn name(&self) -> &str { "rg-perms" }
    fn description(&self) -> &str { "Show or modify resource group permissions" }
    fn usage(&self) -> &str { "rg-perms <rg-id> [set <group-id> <perms...>]" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            return Err(vec![OutputLine::stderr("Usage: rg-perms <rg-id> [set <group-id> <perms...>]")]);
        }
        // If just rg-id, show permissions; if "set", modify
        if args.len() == 1 {
            ctx_output(ctx, "__rg_perms")
        } else {
            ctx_result(ctx, "__rg_perms_set")
        }
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
