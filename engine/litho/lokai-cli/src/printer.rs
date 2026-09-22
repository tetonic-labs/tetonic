//! Renders the agent step stream to stdout.

use std::io::Write;

/// Renders the agent's event stream. Tracks whether we're mid-stream of an
/// assistant message so live tokens print inline and other events break cleanly.
#[derive(Default)]
pub struct Printer {
    step_no: usize,
    streaming: bool,
}

impl Printer {
    pub fn on_token(&mut self, token: &str) {
        if !self.streaming {
            print!("\n[assistant] ");
            self.streaming = true;
        }
        print!("{token}");
        let _ = std::io::stdout().flush();
    }

    pub fn on_thought(&mut self, token: &str) {
        if !self.streaming {
            print!("\n[thought] ");
            self.streaming = true;
        }
        print!("{token}");
        let _ = std::io::stdout().flush();
    }

    pub fn on_answer(&mut self, text: &str) {
        self.end_line();
        println!("\n--- answer ---\n{text}\n--------------");
    }

    pub fn on_tool_call(&mut self, name: &str, args: &serde_json::Value) {
        self.end_line();
        self.step_no += 1;
        let preview = compact_args(args);
        println!("\n[{:>2}] -> {name}({preview})", self.step_no);
    }

    pub fn on_tool_result(&mut self, ok: bool, summary: &str) {
        self.end_line();
        let mark = if ok { "ok" } else { "ERR" };
        println!("     <- [{mark}] {summary}");
    }

    /// Close out an in-progress streamed line before printing a structured event.
    pub fn end_line(&mut self) {
        if self.streaming {
            println!();
            self.streaming = false;
        }
    }
}

fn compact_args(args: &serde_json::Value) -> String {
    let s = args.to_string();
    if s.len() > 120 {
        format!(
            "{}…",
            &s[..s.char_indices().nth(120).map(|(i, _)| i).unwrap_or(s.len())]
        )
    } else {
        s
    }
}
