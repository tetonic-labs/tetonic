//! CLI and in-session help text.

use clap::CommandFactory;

use crate::Args;

/// Short hint printed under the banner on agent startup.
pub fn print_startup_hint() {
    println!();
    println!("  Type a task or question to begin · /help for commands · Ctrl+Q or /exit to quit");
    println!("  Ctrl+C cancels an in-flight turn (does not quit)");
    println!("  One-shot: tetonic \"fix the bug in main.rs\" --workspace .");
    println!("  Explain:  tetonic --orchestrate auto --workspace .  (routes read-only to planner)");
    println!("  Full CLI: tetonic help   (or  tetonic --help)");
    println!();
}

/// Full CLI reference (`tetonic help`, or `tetonic --help`).
pub fn print_cli_help() -> anyhow::Result<()> {
    Args::command().print_long_help()?;
    Ok(())
}

/// Help shown when the user types `help` in interactive chat.
#[allow(dead_code)]
pub fn print_chat_help() {
    println!(
        "\
Chat commands:
  <message>   send a task or question to the agent
  help        show this message
  exit        quit (also: quit, :q, Ctrl+Q in the TUI)
  Ctrl+C      cancel the in-flight turn (TUI; does not quit)

CLI (no model needed for most):
  tetonic help              full flag and command reference
  tetonic --workspace .     interactive chat in a project
  tetonic \"<task>\"          one-shot task in the workspace
  tetonic --sessions        list past sessions
  tetonic --index           build the code index
  tetonic --def NAME        find a symbol definition
  tetonic --search QUERY    keyword search the index
  tetonic --checkpoints     list workspace checkpoints
  tetonic --undo            step workspace back one checkpoint

Run tetonic help for the complete list."
    );
}

pub const CLI_EXAMPLES: &str = r#"
EXAMPLES:
  Interactive chat (default when no task is given)
    tetonic --workspace .

  One-shot task
    tetonic "add tests for parse_config" --workspace .

  Code index and search (no model)
    tetonic --index --workspace .
    tetonic --def MyStruct --workspace .
    tetonic --search "error handling" --workspace .
    tetonic --outline src/main.rs --workspace .

  Session history
    tetonic --sessions
    tetonic --session <id>

  Workspace time travel
    tetonic --checkpoint "before-refactor" --workspace .
    tetonic --checkpoints --workspace .
    tetonic --restore before-refactor --workspace .
    tetonic --undo --workspace .

  Project memory
    tetonic --project-status --workspace .
    tetonic --project-note "uses pytest" --workspace .

  Orchestration
    tetonic --orchestrate auto --llm-router --workspace .

  Fleet enrollment
    tetonic estate enroll --help

  Local organization/team administration (no model)
    tetonic control --help
"#;
