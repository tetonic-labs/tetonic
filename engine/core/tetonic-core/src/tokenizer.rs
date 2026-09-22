//! Token counting for context budgeting.

/// Token counting behind a trait so the budgeting logic is independent of how
/// tokens are counted (heuristic estimate vs. an exact per-model tokenizer).
pub trait Tokenizer: Send + Sync {
    fn count(&self, text: &str) -> usize;
    /// True if `count` is an estimate rather than an exact tokenization. Surfaced
    /// in the [`ContextReport`] so the user knows whether budgets are precise.
    fn estimated(&self) -> bool {
        true
    }
}

/// A heuristic estimator — **not** a real BPE tokenizer. Roughly ~3.7 chars/token,
/// which is a reasonable average for code + prose. Good enough to budget context
/// when no model tokenizer file is available.
pub struct HeuristicTokenizer;

impl Tokenizer for HeuristicTokenizer {
    fn count(&self, text: &str) -> usize {
        let chars = text.chars().count();
        ((chars as f32) / 3.7).ceil() as usize
    }
}

/// An exact tokenizer backed by a HuggingFace fast-tokenizer file (`tokenizer.json`),
/// loaded from **local disk only** (no download). Use the file that ships with
/// the model you're running so context budgets and compaction thresholds are
/// precise. Falls back to the heuristic for any text that fails to encode.
pub struct ExactTokenizer {
    inner: tokenizers::Tokenizer,
}

impl ExactTokenizer {
    pub fn from_file(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let inner = tokenizers::Tokenizer::from_file(path.as_ref())
            .map_err(|e| anyhow::anyhow!("loading tokenizer: {e}"))?;
        Ok(Self { inner })
    }
}

impl Tokenizer for ExactTokenizer {
    fn count(&self, text: &str) -> usize {
        match self.inner.encode(text, false) {
            Ok(enc) => enc.len(),
            Err(_) => HeuristicTokenizer.count(text),
        }
    }
    fn estimated(&self) -> bool {
        false
    }
}
