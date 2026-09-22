//! Tree-sitter symbol and import extraction.

use tree_sitter::{Node, Parser};

use crate::types::{Lang, MAX_SIG_CHARS};
use crate::util::cap_chars;

pub(crate) struct RawSymbol {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) signature: String,
    pub(crate) start_line: i64,
    pub(crate) end_line: i64,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) parent_local: Option<usize>,
}

pub(crate) struct RawImport {
    pub(crate) target: String,
    pub(crate) line: i64,
}

pub(crate) fn extract(lang: Lang, src: &str) -> (Vec<RawSymbol>, Vec<RawImport>) {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let language = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE,
        Lang::Python => tree_sitter_python::LANGUAGE,
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE,
        Lang::Text => return (symbols, imports),
    };
    let mut parser = Parser::new();
    if parser.set_language(&language.into()).is_err() {
        return (symbols, imports);
    }
    let Some(tree) = parser.parse(src, None) else {
        return (symbols, imports);
    };
    walk(
        tree.root_node(),
        src.as_bytes(),
        lang,
        None,
        &mut symbols,
        &mut imports,
    );
    (symbols, imports)
}

fn walk(
    node: Node,
    bytes: &[u8],
    lang: Lang,
    parent_local: Option<usize>,
    symbols: &mut Vec<RawSymbol>,
    imports: &mut Vec<RawImport>,
) {
    let mut next_parent = parent_local;

    if let Some(mut kind) = symbol_kind(lang, node.kind()) {
        if let Some(name) = symbol_name(&node, kind, bytes) {
            // A function nested in a class/impl/trait is a method.
            if kind == "function" {
                if let Some(p) = parent_local {
                    if matches!(
                        symbols[p].kind.as_str(),
                        "impl" | "class" | "trait" | "interface"
                    ) {
                        kind = "method";
                    }
                }
            }
            let me = symbols.len();
            symbols.push(RawSymbol {
                kind: kind.to_string(),
                name,
                signature: first_line(&node, bytes),
                start_line: node.start_position().row as i64 + 1,
                end_line: node.end_position().row as i64 + 1,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                parent_local,
            });
            next_parent = Some(me);
        }
    }

    collect_import(lang, &node, bytes, imports);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, bytes, lang, next_parent, symbols, imports);
    }
}

fn symbol_kind(lang: Lang, node_kind: &str) -> Option<&'static str> {
    match lang {
        Lang::Rust => Some(match node_kind {
            "function_item" => "function",
            "struct_item" => "struct",
            "enum_item" => "enum",
            "union_item" => "union",
            "trait_item" => "trait",
            "impl_item" => "impl",
            "mod_item" => "module",
            "const_item" => "const",
            "static_item" => "static",
            "type_item" => "type",
            "macro_definition" => "macro",
            _ => return None,
        }),
        Lang::Python => Some(match node_kind {
            "function_definition" => "function",
            "class_definition" => "class",
            _ => return None,
        }),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => Some(match node_kind {
            "function_declaration" | "generator_function_declaration" => "function",
            "class_declaration" => "class",
            "method_definition" => "method",
            "interface_declaration" => "interface",
            "type_alias_declaration" => "type",
            "enum_declaration" => "enum",
            _ => return None,
        }),
        Lang::Text => None,
    }
}

fn node_text(node: &Node, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}

fn symbol_name(node: &Node, kind: &str, bytes: &[u8]) -> Option<String> {
    if kind == "impl" {
        // `impl Trait for Type` / `impl Type`.
        let ty = node
            .child_by_field_name("type")
            .map(|n| node_text(&n, bytes));
        let tr = node
            .child_by_field_name("trait")
            .map(|n| node_text(&n, bytes));
        return Some(match (tr, ty) {
            (Some(t), Some(y)) => format!("{t} for {y}"),
            (None, Some(y)) => y,
            _ => "impl".to_string(),
        });
    }
    node.child_by_field_name("name")
        .map(|n| node_text(&n, bytes))
}

fn first_line(node: &Node, bytes: &[u8]) -> String {
    let text = node_text(node, bytes);
    let line = text.lines().next().unwrap_or("").trim();
    cap_chars(line, MAX_SIG_CHARS)
}

fn collect_import(lang: Lang, node: &Node, bytes: &[u8], imports: &mut Vec<RawImport>) {
    let is_import = match lang {
        Lang::Rust => node.kind() == "use_declaration",
        Lang::Python => matches!(node.kind(), "import_statement" | "import_from_statement"),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            matches!(node.kind(), "import_statement" | "import_clause")
        }
        Lang::Text => false,
    };
    if !is_import {
        return;
    }
    let target = node_text(node, bytes)
        .trim()
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim()
        .to_string();
    imports.push(RawImport {
        target,
        line: node.start_position().row as i64 + 1,
    });
}
