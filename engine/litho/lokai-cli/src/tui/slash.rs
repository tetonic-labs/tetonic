//! Slash palette: grouped commands and cycling Tab completion.

pub const PRIMARY: &[&str] = &[
    "/help",
    "/status",
    "/model",
    "/inference",
    "/recovery",
    "/doctor",
    "/debug",
    "/capacity",
    "/egress",
    "/ps",
    "/evict",
    "/exit",
];

pub const LEFTOVER: &[&str] = &["/optimize", "/lokai", "/kill-ollama"];

pub fn grouped_help() -> &'static str {
    "Session\n\
     /status      this session, resume state, and phase\n\
     /model       choose a model (F5 also opens the picker)\n\
     /model ID    advanced: change both model tiers while idle\n\
     /inference   inspect or select a registered inference profile\n\
     /recovery    inspect interrupted runs and explicit abandonment\n\
     /doctor      this chat vs the saved capacity profile\n\
     /debug       toggle detailed error traces and diagnostics\n\
     /capacity    capacity status for this session\n\
     /exit        quit (also Ctrl+Q)\n\n\
     Network\n\
     /egress      persist egress rules (estate store + live reload)\n\n\
     GPU\n\
     /ps          currently loaded Ollama models\n\
     /evict       unload Ollama models from VRAM\n\n\
     /help more   leftover commands not spawned from the inspector\n"
}

pub fn leftover_help() -> &'static str {
    "These commands are not spawned from the inspector.\n\n\
     /optimize     rebuild the default capacity profile from a terminal\n\
     /lokai        nested CLI is not spawned here\n\
     /kill-ollama  stop Ollama from a terminal if it is wedged\n"
}

pub fn matches(prefix: &str) -> Vec<&'static str> {
    if !prefix.starts_with('/') {
        return Vec::new();
    }
    let primary: Vec<_> = PRIMARY
        .iter()
        .copied()
        .filter(|c| c.starts_with(prefix))
        .collect();
    if !primary.is_empty() {
        return primary;
    }
    if prefix.len() < 4 {
        return Vec::new();
    }
    LEFTOVER
        .iter()
        .copied()
        .filter(|c| c.starts_with(prefix))
        .collect()
}

/// Fill the next prefix match. Returns the completed buffer and the next cycle index.
pub fn cycle(prefix: &str, index: usize) -> Option<(String, usize)> {
    let found = matches(prefix);
    if found.is_empty() {
        return None;
    }
    let i = index % found.len();
    let next = (i + 1) % found.len();
    Some((found[i].to_string(), next))
}

pub fn ghost(prefix: &str, index: usize) -> Option<&'static str> {
    let found = matches(prefix);
    if found.is_empty() {
        return None;
    }
    let suggestion = found[index % found.len()];
    if suggestion.len() > prefix.len() {
        Some(&suggestion[prefix.len()..])
    } else {
        None
    }
}

pub fn is_quit_line(line: &str) -> bool {
    matches!(line.trim(), "/exit" | "exit" | "quit" | ":q" | "/quit")
}

pub fn is_status_line(line: &str) -> bool {
    matches!(line.trim(), "/status" | "/status ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycles_prefix_matches() {
        let (a, i1) = cycle("/d", 0).unwrap();
        let (b, i2) = cycle("/d", i1).unwrap();
        assert_eq!(a, "/doctor");
        assert_eq!(b, "/debug");
        assert_ne!(i2, i1);
        let (c, i3) = cycle("/d", i2).unwrap();
        assert_eq!(c, "/doctor");
        assert_eq!(i3, i1);
        let found = matches("/");
        assert!(found.contains(&"/status"));
        assert!(found.contains(&"/doctor"));
        assert!(found.contains(&"/debug"));
        let (first, next) = cycle("/", 0).unwrap();
        let (second, _) = cycle("/", next).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn leftover_kill_is_not_first_k_complete() {
        assert!(matches("/k").is_empty());
        assert!(matches("/ki").is_empty());
        assert_eq!(matches("/kill"), vec!["/kill-ollama"]);
        assert!(matches("/o").is_empty());
        assert_eq!(matches("/opt"), vec!["/optimize"]);
    }

    #[test]
    fn ghost_shows_remainder_of_current_match() {
        assert_eq!(ghost("/do", 0), Some("ctor"));
        assert_eq!(ghost("/doctor", 0), None);
    }

    #[test]
    fn help_groups_park_leftovers() {
        assert!(grouped_help().contains("/status"));
        assert!(!grouped_help().contains("/kill-ollama"));
        assert!(leftover_help().contains("/kill-ollama"));
        assert!(leftover_help().contains("not spawned"));
    }
}
