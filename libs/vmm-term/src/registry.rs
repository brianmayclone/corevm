//! Command registry — stores registered commands and dispatches execution.

use std::collections::HashMap;
use serde::Serialize;

/// Output line from a command — sent back to the terminal client.
#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub kind: OutputKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    /// Normal output text.
    Stdout,
    /// Error output.
    Stderr,
    /// Success message (green).
    Success,
    /// Warning (yellow).
    Warning,
    /// Informational (dimmed).
    Info,
    /// Table header row.
    TableHeader,
    /// Table data row.
    TableRow,
}

impl OutputLine {
    pub fn stdout(text: impl Into<String>) -> Self { Self { kind: OutputKind::Stdout, text: text.into() } }
    pub fn stderr(text: impl Into<String>) -> Self { Self { kind: OutputKind::Stderr, text: text.into() } }
    pub fn success(text: impl Into<String>) -> Self { Self { kind: OutputKind::Success, text: text.into() } }
    pub fn warning(text: impl Into<String>) -> Self { Self { kind: OutputKind::Warning, text: text.into() } }
    pub fn info(text: impl Into<String>) -> Self { Self { kind: OutputKind::Info, text: text.into() } }
    pub fn table_header(text: impl Into<String>) -> Self { Self { kind: OutputKind::TableHeader, text: text.into() } }
    pub fn table_row(text: impl Into<String>) -> Self { Self { kind: OutputKind::TableRow, text: text.into() } }
}

/// Result of executing a command.
pub type CommandResult = Result<Vec<OutputLine>, Vec<OutputLine>>;

/// Context passed to every command — provides access to shared state.
/// The terminal handler constructs this with references to AppState.
pub struct CommandContext {
    /// Arbitrary key-value data the server injects (e.g. "user_id", "user_role").
    pub env: HashMap<String, String>,
    /// Opaque pointer to application state — commands downcast this.
    pub app_state: *const (),
}

// Safety: CommandContext is only used on the server thread that owns AppState.
unsafe impl Send for CommandContext {}
unsafe impl Sync for CommandContext {}

impl CommandContext {
    pub fn new() -> Self {
        Self { env: HashMap::new(), app_state: std::ptr::null() }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(|s| s.as_str())
    }

    /// Get the app state as a typed reference.
    /// # Safety
    /// Caller must ensure T matches the actual type stored.
    pub unsafe fn state<T>(&self) -> &T {
        &*(self.app_state as *const T)
    }
}

/// Trait that all terminal commands must implement.
pub trait Command: Send + Sync {
    /// Command name (e.g. "vm-list", "vm-start").
    fn name(&self) -> &str;

    /// Short description shown in help.
    fn description(&self) -> &str;

    /// Usage string (e.g. "vm-start <vm-id>").
    fn usage(&self) -> &str;

    /// Execute the command with parsed arguments.
    fn run(&self, args: &[&str], ctx: &CommandContext) -> CommandResult;

    /// Tab-completion candidates for the given partial input.
    fn completions(&self, _partial: &str, _ctx: &CommandContext) -> Vec<String> {
        Vec::new()
    }
}

/// Registry of all available commands.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self { commands: HashMap::new() }
    }

    /// Register a command. Panics if name is already taken.
    pub fn register(&mut self, cmd: Box<dyn Command>) {
        let name = cmd.name().to_string();
        if self.commands.contains_key(&name) {
            panic!("Command '{}' already registered", name);
        }
        self.commands.insert(name, cmd);
    }

    /// Execute a parsed command line.
    pub fn execute(&self, name: &str, args: &[&str], ctx: &CommandContext) -> CommandResult {
        match self.commands.get(name) {
            Some(cmd) => cmd.run(args, ctx),
            None => Err(vec![
                OutputLine::stderr(format!("Unknown command: '{}'. Type 'help' for available commands.", name)),
            ]),
        }
    }

    /// Get all registered command names + descriptions (for help).
    pub fn list(&self) -> Vec<(&str, &str, &str)> {
        let mut cmds: Vec<_> = self.commands.values()
            .map(|c| (c.name(), c.description(), c.usage()))
            .collect();
        cmds.sort_by_key(|(name, _, _)| *name);
        cmds
    }

    /// Get completions for a partial command name.
    pub fn complete_command(&self, partial: &str) -> Vec<String> {
        self.commands.keys()
            .filter(|name| name.starts_with(partial))
            .cloned()
            .collect()
    }

    /// Get completions for arguments of a specific command.
    pub fn complete_args(&self, cmd_name: &str, partial: &str, ctx: &CommandContext) -> Vec<String> {
        match self.commands.get(cmd_name) {
            Some(cmd) => cmd.completions(partial, ctx),
            None => Vec::new(),
        }
    }
}
