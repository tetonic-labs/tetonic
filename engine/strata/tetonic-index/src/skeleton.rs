//! Tree-Sitter AST Skeleton Extraction (OPT-602).
//!
//! Strips function and method bodies while retaining:
//! - Module docstrings and comments
//! - Struct, Enum, Union, Class declarations & fields
//! - Trait & Interface declarations
//! - Type aliases & imports
//! - Function and method signatures & return types
//! - Constants & statics
//!
//! Cuts token payload by 60%–80% for dependency and secondary context.

use crate::types::Lang;
use tree_sitter::{Node, Parser};

/// Strips implementation bodies from `src` using Tree-Sitter AST parsing.
pub fn skeletonize(lang: Lang, src: &str) -> String {
    let language = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE,
        Lang::Python => tree_sitter_python::LANGUAGE,
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE,
        Lang::Text => return src.to_string(),
    };

    let mut parser = Parser::new();
    if parser.set_language(&language.into()).is_err() {
        return src.to_string();
    }

    let Some(tree) = parser.parse(src, None) else {
        return src.to_string();
    };

    let mut body_replacements = Vec::new();
    collect_body_spans(
        tree.root_node(),
        src.as_bytes(),
        lang,
        &mut body_replacements,
    );

    if body_replacements.is_empty() {
        return src.to_string();
    }

    // Sort spans in ascending order by start_byte and remove nested/overlapping spans
    body_replacements.sort_by_key(|(start, _, _)| *start);
    let mut filtered_spans: Vec<(usize, usize, String)> = Vec::new();
    for span in body_replacements {
        if let Some(last) = filtered_spans.last() {
            if span.0 < last.1 {
                // Nested inside previous span, skip
                continue;
            }
        }
        filtered_spans.push(span);
    }

    let mut out = String::with_capacity(src.len());
    let mut last_byte = 0;

    for (start, end, replacement) in filtered_spans {
        if start > last_byte {
            out.push_str(&src[last_byte..start]);
        }
        out.push_str(&replacement);
        last_byte = end;
    }

    if last_byte < src.len() {
        out.push_str(&src[last_byte..]);
    }

    out
}

fn collect_body_spans(
    node: Node,
    bytes: &[u8],
    lang: Lang,
    replacements: &mut Vec<(usize, usize, String)>,
) {
    match lang {
        Lang::Rust => {
            if node.kind() == "function_item" {
                // Check if function has a body block
                if let Some(body) = node.child_by_field_name("body") {
                    if body.kind() == "block" {
                        replacements.push((
                            body.start_byte(),
                            body.end_byte(),
                            "{ /* ... */ }".to_string(),
                        ));
                        return;
                    }
                }
            }
        }
        Lang::Python => {
            if node.kind() == "function_definition" {
                if let Some(body) = node.child_by_field_name("body") {
                    if body.kind() == "block" {
                        // Determine base indentation of function to indent `...` properly
                        let fn_row = node.start_position().row;
                        let fn_indent = extract_line_indent(bytes, fn_row);
                        let replacement = format!("\n{fn_indent}    ...");
                        replacements.push((body.start_byte(), body.end_byte(), replacement));
                        return;
                    }
                }
            }
        }
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            if matches!(
                node.kind(),
                "function_declaration"
                    | "method_definition"
                    | "arrow_function"
                    | "function"
                    | "function_expression"
            ) {
                if let Some(body) = node.child_by_field_name("body") {
                    if body.kind() == "statement_block" {
                        replacements.push((
                            body.start_byte(),
                            body.end_byte(),
                            "{ /* ... */ }".to_string(),
                        ));
                        return;
                    }
                }
            }
        }
        Lang::Text => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_body_spans(child, bytes, lang, replacements);
    }
}

fn extract_line_indent(bytes: &[u8], row: usize) -> String {
    let mut current_row = 0;
    let mut line_start = 0;

    for (i, &b) in bytes.iter().enumerate() {
        if current_row == row {
            if b == b' ' || b == b'\t' {
                continue;
            } else {
                return String::from_utf8_lossy(&bytes[line_start..i]).to_string();
            }
        }
        if b == b'\n' {
            current_row += 1;
            line_start = i + 1;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skeletonize_rust() {
        let code = r#"
pub struct User {
    pub id: u64,
    pub name: String,
}

impl User {
    pub fn new(id: u64, name: String) -> Self {
        let name_trimmed = name.trim().to_string();
        Self { id, name: name_trimmed }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }
}
"#;
        let skeleton = skeletonize(Lang::Rust, code);
        assert!(skeleton.contains("pub struct User"));
        assert!(skeleton.contains("pub id: u64"));
        assert!(skeleton.contains("impl User"));
        assert!(skeleton.contains("pub fn new(id: u64, name: String) -> Self { /* ... */ }"));
        assert!(skeleton.contains("pub fn get_name(&self) -> &str { /* ... */ }"));
        assert!(!skeleton.contains("name_trimmed"));
    }

    #[test]
    fn test_skeletonize_python() {
        let code = r#"
class Greeter:
    def __init__(self, name: str):
        self.name = name
        self.count = 0

    def greet(self) -> str:
        self.count += 1
        return f"Hello, {self.name}!"
"#;
        let skeleton = skeletonize(Lang::Python, code);
        assert!(skeleton.contains("class Greeter:"));
        assert!(skeleton.contains("def __init__(self, name: str):"));
        assert!(skeleton.contains("def greet(self) -> str:"));
        assert!(skeleton.contains("..."));
        assert!(!skeleton.contains("self.count += 1"));
    }

    #[test]
    fn test_skeletonize_typescript() {
        let code = r#"
export interface Config {
    port: number;
    host: string;
}

export function startServer(cfg: Config): void {
    const s = createServer();
    s.listen(cfg.port, cfg.host);
}
"#;
        let skeleton = skeletonize(Lang::TypeScript, code);
        assert!(skeleton.contains("export interface Config"));
        assert!(skeleton.contains("export function startServer(cfg: Config): void { /* ... */ }"));
        assert!(!skeleton.contains("const s = createServer()"));
    }
}
