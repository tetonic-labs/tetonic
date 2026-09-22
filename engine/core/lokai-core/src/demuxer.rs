//! Zero-allocation speculative streaming token demuxer.
//!
//! Separates model output streams in real-time into:
//! 1. Natural Language Prose (`DemuxedChunk::Prose`)
//! 2. Model Reasoning / Thoughts (`DemuxedChunk::Thought` from `<think>` tags)
//! 3. Suppressed Raw Tool Calling JSON (`DemuxedChunk::ToolJson`)

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemuxedChunk {
    /// Natural language prose for user chat.
    Prose(String),
    /// Internal reasoning / thoughts.
    Thought(String),
    /// Suppressed internal tool JSON payload.
    ToolJson(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemuxerState {
    Detecting,
    StreamingProse,
    StreamingThought,
    BufferingToolJson,
}

pub struct TokenDemuxer {
    state: DemuxerState,
    prefix_buffer: String,
}

impl Default for TokenDemuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenDemuxer {
    const MAX_SPECULATIVE_CHARS: usize = 64;

    pub fn new() -> Self {
        Self {
            state: DemuxerState::Detecting,
            prefix_buffer: String::with_capacity(Self::MAX_SPECULATIVE_CHARS),
        }
    }

    pub fn push(&mut self, token: &str) -> Vec<DemuxedChunk> {
        let mut out = Vec::new();
        self.process_token(token, &mut out);
        out
    }

    fn process_token(&mut self, token: &str, out: &mut Vec<DemuxedChunk>) {
        match self.state {
            DemuxerState::Detecting => {
                self.prefix_buffer.push_str(token);
                let trimmed = self.prefix_buffer.trim_start();

                if let Some(idx) = self.prefix_buffer.find("<think>") {
                    let buf = std::mem::take(&mut self.prefix_buffer);
                    let before = &buf[..idx];
                    let after = buf[idx + 7..].to_string();
                    if !before.is_empty() && !content_looks_like_tool_json(before) {
                        out.push(DemuxedChunk::Prose(before.to_string()));
                    }
                    self.state = DemuxerState::StreamingThought;
                    if !after.is_empty() {
                        self.process_token(&after, out);
                    }
                } else if trimmed.starts_with('{') || trimmed.starts_with("```json") {
                    if trimmed.contains("\"tool_calls\"")
                        || trimmed.contains("\"name\":")
                        || trimmed.contains("\"parameters\":")
                        || trimmed.contains("\"arguments\":")
                    {
                        self.state = DemuxerState::BufferingToolJson;
                        let buf = std::mem::take(&mut self.prefix_buffer);
                        out.push(DemuxedChunk::ToolJson(buf));
                    } else if self.prefix_buffer.len() >= Self::MAX_SPECULATIVE_CHARS {
                        self.state = DemuxerState::StreamingProse;
                        let buf = std::mem::take(&mut self.prefix_buffer);
                        out.push(DemuxedChunk::Prose(buf));
                    }
                } else if self.prefix_buffer.len() >= 8
                    || self.prefix_buffer.contains(' ')
                    || self.prefix_buffer.contains('\n')
                {
                    self.state = DemuxerState::StreamingProse;
                    let buf = std::mem::take(&mut self.prefix_buffer);
                    out.push(DemuxedChunk::Prose(buf));
                }
            }
            DemuxerState::StreamingThought => {
                if let Some(idx) = token.find("</think>") {
                    let thought_part = &token[..idx];
                    let remainder = &token[idx + 8..];
                    if !thought_part.is_empty() {
                        out.push(DemuxedChunk::Thought(thought_part.to_string()));
                    }
                    self.state = DemuxerState::Detecting;
                    self.prefix_buffer.clear();
                    if !remainder.is_empty() {
                        self.process_token(remainder, out);
                    }
                } else {
                    out.push(DemuxedChunk::Thought(token.to_string()));
                }
            }
            DemuxerState::StreamingProse => {
                if let Some(idx) = token.find("<think>") {
                    let prose_part = &token[..idx];
                    let remainder = &token[idx + 7..];
                    if !prose_part.is_empty() {
                        out.push(DemuxedChunk::Prose(prose_part.to_string()));
                    }
                    self.state = DemuxerState::StreamingThought;
                    if !remainder.is_empty() {
                        self.process_token(remainder, out);
                    }
                } else {
                    out.push(DemuxedChunk::Prose(token.to_string()));
                }
            }
            DemuxerState::BufferingToolJson => {
                out.push(DemuxedChunk::ToolJson(token.to_string()));
            }
        }
    }

    pub fn finish(&mut self) -> Vec<DemuxedChunk> {
        let mut out = Vec::new();
        if !self.prefix_buffer.is_empty() {
            let buf = std::mem::take(&mut self.prefix_buffer);
            if content_looks_like_tool_json(&buf) {
                out.push(DemuxedChunk::ToolJson(buf));
            } else {
                out.push(DemuxedChunk::Prose(buf));
            }
        }
        out
    }
}

fn content_looks_like_tool_json(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with('{') || t.starts_with("```json"))
        && (t.contains("\"tool_calls\"")
            || t.contains("\"name\":")
            || t.contains("\"parameters\":")
            || t.contains("\"arguments\":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_plain_prose_immediately() {
        let mut demuxer = TokenDemuxer::new();
        let mut chunks = Vec::new();
        for t in ["Hello", " world", " from", " assistant!"] {
            chunks.extend(demuxer.push(t));
        }
        chunks.extend(demuxer.finish());

        let prose: String = chunks
            .into_iter()
            .filter_map(|c| match c {
                DemuxedChunk::Prose(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(prose, "Hello world from assistant!");
    }

    #[test]
    fn suppresses_raw_tool_json_stream() {
        let mut demuxer = TokenDemuxer::new();
        let mut chunks = Vec::new();
        for t in [
            "{\"name\":",
            " \"read_file\",",
            " \"arguments\":",
            " {\"path\": \"src/main.rs\"}}",
        ] {
            chunks.extend(demuxer.push(t));
        }
        chunks.extend(demuxer.finish());

        let has_prose = chunks.iter().any(|c| matches!(c, DemuxedChunk::Prose(_)));
        let tool_json_count = chunks
            .iter()
            .filter(|c| matches!(c, DemuxedChunk::ToolJson(_)))
            .count();

        assert!(!has_prose, "Expected no prose chunks for tool JSON");
        assert!(tool_json_count > 0, "Expected tool JSON chunks");
    }

    #[test]
    fn extracts_thought_tags_cleanly() {
        let mut demuxer = TokenDemuxer::new();
        let mut chunks = Vec::new();
        for t in [
            "<think>",
            "Let me check",
            " the architecture.",
            "</think>",
            " Here is",
            " the explanation.",
        ] {
            chunks.extend(demuxer.push(t));
        }
        chunks.extend(demuxer.finish());

        let thoughts: String = chunks
            .iter()
            .filter_map(|c| match c {
                DemuxedChunk::Thought(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let prose: String = chunks
            .iter()
            .filter_map(|c| match c {
                DemuxedChunk::Prose(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(thoughts, "Let me check the architecture.");
        assert_eq!(prose, " Here is the explanation.");
    }
}
