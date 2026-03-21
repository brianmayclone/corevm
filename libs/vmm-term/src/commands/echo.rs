use crate::registry::*;

pub struct EchoCommand;

impl Command for EchoCommand {
    fn name(&self) -> &str { "echo" }
    fn description(&self) -> &str { "Print text to the terminal" }
    fn usage(&self) -> &str { "echo <text...>" }

    fn run(&self, args: &[&str], _ctx: &CommandContext) -> CommandResult {
        Ok(vec![OutputLine::stdout(args.join(" "))])
    }
}
