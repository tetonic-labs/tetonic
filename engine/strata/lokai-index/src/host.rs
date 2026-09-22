//! [`lokai_domain::CodeIndex`] adapter. Product pack owns the concrete index.

use std::path::Path;

use lokai_domain::{
    CodeDefinition, CodeIndex, CodeIndexOpen, CodeIndexStatus, CodeOutlineRow, CodeSearchHit,
    TextSkeleton,
};

use crate::types::Lang;
use crate::{skeletonize, workspace_storage_key, Index};

impl CodeIndex for Index {
    fn workspace_key(&self, root: &Path) -> String {
        workspace_storage_key(root)
    }

    fn status(&self, ws_key: &str) -> Result<CodeIndexStatus, String> {
        let st = Index::status(self, ws_key).map_err(|e| e.to_string())?;
        let langs = st
            .by_lang
            .iter()
            .map(|(l, n)| format!("{l}:{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = format!(
            "{} files, {} symbols, {} chunks{}",
            st.files,
            st.symbols,
            st.chunks,
            if langs.is_empty() {
                String::new()
            } else {
                format!(" ({langs})")
            }
        );
        Ok(CodeIndexStatus { summary })
    }

    fn find_definition_in(
        &self,
        ws_key: &str,
        name: &str,
        path: Option<&str>,
    ) -> Result<Vec<CodeDefinition>, String> {
        Index::find_definition_in(self, ws_key, name, path)
            .map_err(|e| e.to_string())
            .map(|rows| {
                rows.into_iter()
                    .map(|d| CodeDefinition {
                        kind: d.kind,
                        rel: d.rel,
                        start_line: d.start_line as u32,
                        signature: d.signature,
                    })
                    .collect()
            })
    }

    fn search(&self, ws_key: &str, query: &str, limit: u32) -> Result<Vec<CodeSearchHit>, String> {
        Index::search(self, ws_key, query, limit)
            .map_err(|e| e.to_string())
            .map(|hits| hits.into_iter().map(hit_to_domain).collect())
    }

    fn outline(&self, ws_key: &str, rel: &str) -> Result<Vec<CodeOutlineRow>, String> {
        Index::outline(self, ws_key, rel)
            .map_err(|e| e.to_string())
            .map(|rows| {
                rows.into_iter()
                    .map(|r| CodeOutlineRow {
                        depth: r.depth,
                        kind: r.kind,
                        name: r.name,
                        start_line: r.start_line as u32,
                    })
                    .collect()
            })
    }

    fn find_mentions(
        &self,
        ws_key: &str,
        name: &str,
        limit: u32,
    ) -> Result<Vec<CodeSearchHit>, String> {
        Index::find_mentions(self, ws_key, name, limit)
            .map_err(|e| e.to_string())
            .map(|hits| hits.into_iter().map(hit_to_domain).collect())
    }
}

fn hit_to_domain(h: crate::types::Hit) -> CodeSearchHit {
    CodeSearchHit {
        rel: h.rel,
        start_line: h.start_line as u32,
        end_line: h.end_line as u32,
        symbol_name: h.symbol_name,
        preview: h.preview,
        score: h.score as f32,
    }
}

/// Opens [`Index`] from a filesystem path. Held by the product pack / app.
#[derive(Debug, Default, Clone, Copy)]
pub struct FilesystemCodeIndex;

impl CodeIndexOpen for FilesystemCodeIndex {
    fn open(&self, path: &Path) -> Result<Box<dyn CodeIndex>, String> {
        Index::open(path)
            .map(|idx| Box::new(idx) as Box<dyn CodeIndex>)
            .map_err(|e| e.to_string())
    }
}

/// Tree-sitter skeletonizer used at retrieve time (not a ReadAuthority).
#[derive(Debug, Default, Clone, Copy)]
pub struct IndexTextSkeleton;

impl TextSkeleton for IndexTextSkeleton {
    fn skeletonize(&self, path: &str, text: &str) -> String {
        let lang = Lang::from_path(Path::new(path));
        if lang == Lang::Text {
            return text.to_string();
        }
        let skeleton = skeletonize(lang, text);
        if skeleton.len() < text.len() {
            skeleton
        } else {
            text.to_string()
        }
    }
}
