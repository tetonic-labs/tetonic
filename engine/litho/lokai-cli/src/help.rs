//! CLI and in-session help text.

use clap::CommandFactory;

use crate::Args;

/// Short hint printed under the banner on agent startup.
pub fn print_startup_hint() {
    println!();
    println!("  Type a task or question to begin · /help for commands · Ctrl+Q or /exit to quit");
    println!("  Ctrl+C cancels an in-flight turn (does not quit)");
    println!("  One-shot: lokai \"fix the bug in main.rs\" --workspace .");
    println!("  Explain:  lokai --orchestrate auto --workspace .  (routes read-only to planner)");
    println!("  Full CLI: lokai help   (or  lokai --help)");
    println!();
}

/// Full CLI reference (`lokai help`, or `lokai --help`).
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
  lokai help              full flag and command reference
  lokai --workspace .     interactive chat in a project
  lokai \"<task>\"          one-shot task in the workspace
  lokai --sessions        list past sessions
  lokai --index           build the code index
  lokai --def NAME        find a symbol definition
  lokai --search QUERY    keyword search the index
  lokai --checkpoints     list workspace checkpoints
  lokai --undo            step workspace back one checkpoint

Run lokai help for the complete list."
    );
}

pub const CLI_EXAMPLES: &str = r#"
EXAMPLES:
  Interactive chat (default when no task is given)
    lokai --workspace .

  One-shot task
    lokai "add tests for parse_config" --workspace .

  Code index and search (no model)
    lokai --index --workspace .
    lokai --def MyStruct --workspace .
    lokai --search "error handling" --workspace .
    lokai --outline src/main.rs --workspace .

  Session history
    lokai --sessions
    lokai --session <id>

  Workspace time travel
    lokai --checkpoint "before-refactor" --workspace .
    lokai --checkpoints --workspace .
    lokai --restore before-refactor --workspace .
    lokai --undo --workspace .

  Project memory
    lokai --project-status --workspace .
    lokai --project-note "uses pytest" --workspace .

  Orchestration
    lokai --orchestrate auto --llm-router --workspace .

  Fleet enrollment
    lokai estate enroll --help
"#;
