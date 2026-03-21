use crate::registry::*;

pub struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &str { "help" }
    fn description(&self) -> &str { "Show available commands" }
    fn usage(&self) -> &str { "help [command]" }

    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult {
        // help is special — it needs access to the registry, which we pass via env
        // The terminal handler injects "help_text" into ctx.env before dispatching
        if let Some(help_text) = ctx.get("__help_text") {
            Ok(help_text.lines().map(|l| OutputLine::stdout(l)).collect())
        } else {
            Ok(vec![OutputLine::info("Type 'help' to see available commands.")])
        }
    }
}
