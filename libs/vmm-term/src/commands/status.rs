use crate::registry::*;

pub struct StatusCommand;

impl Command for StatusCommand {
    fn name(&self) -> &str { "status" }
    fn description(&self) -> &str { "Show server status and system information" }
    fn usage(&self) -> &str { "status" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        if let Some(data) = ctx.get("__status") {
            Ok(data.lines().map(|l| OutputLine::stdout(l)).collect())
        } else {
            Err(vec![OutputLine::stderr("Failed to retrieve server status")])
        }
    }
}

pub struct UptimeCommand;

impl Command for UptimeCommand {
    fn name(&self) -> &str { "uptime" }
    fn description(&self) -> &str { "Show server uptime" }
    fn usage(&self) -> &str { "uptime" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        if let Some(data) = ctx.get("__uptime") {
            Ok(vec![OutputLine::info(data)])
        } else {
            Err(vec![OutputLine::stderr("Failed to retrieve uptime")])
        }
    }
}

pub struct WhoamiCommand;

impl Command for WhoamiCommand {
    fn name(&self) -> &str { "whoami" }
    fn description(&self) -> &str { "Show current user information" }
    fn usage(&self) -> &str { "whoami" }

    fn run(&self, _args: &[&str], ctx: &CommandContext) -> CommandResult {
        let user = ctx.get("username").unwrap_or("unknown");
        let role = ctx.get("user_role").unwrap_or("unknown");
        Ok(vec![
            OutputLine::stdout(format!("User: {}", user)),
            OutputLine::stdout(format!("Role: {}", role)),
        ])
    }
}
