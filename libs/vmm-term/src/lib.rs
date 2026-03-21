//! vmm-term — Command registry and execution engine for the CoreVM terminal.
//!
//! Commands implement the `Command` trait and are registered in a `CommandRegistry`.
//! The terminal WebSocket handler passes input lines to `Registry::execute()`,
//! which parses the command name + args and dispatches to the matching handler.

pub mod registry;
pub mod parser;
pub mod commands;

pub use registry::{CommandRegistry, Command, CommandContext, CommandResult, OutputLine, OutputKind};
pub use parser::parse_line;
