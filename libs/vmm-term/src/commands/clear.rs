use crate::registry::*;

pub struct ClearCommand;

impl Command for ClearCommand {
    fn name(&self) -> &str { "clear" }
    fn description(&self) -> &str { "Clear the terminal screen" }
    fn usage(&self) -> &str { "clear" }

    fn run(&self, _args: &[&str], _ctx: &CommandContext) -> CommandResult {
        // The frontend handles the actual clearing — we just send a special marker
        Ok(vec![OutputLine { kind: OutputKind::Stdout, text: "__CLEAR__".to_string() }])
    }
}
