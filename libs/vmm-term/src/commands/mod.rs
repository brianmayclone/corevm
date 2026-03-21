//! Built-in commands that ship with vmm-term.

pub mod help;
pub mod clear;
pub mod echo;

use crate::registry::CommandRegistry;

/// Register all built-in commands.
pub fn register_builtins(registry: &mut CommandRegistry) {
    registry.register(Box::new(help::HelpCommand));
    registry.register(Box::new(clear::ClearCommand));
    registry.register(Box::new(echo::EchoCommand));
}
