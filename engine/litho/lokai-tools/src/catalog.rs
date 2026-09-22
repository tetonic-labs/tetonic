//! Tool catalogue and operator-card prompt lines.

use crate::types::EditFileArgs;
use crate::types::{
    schema_of, FindDefinitionArgs, FindMentionsArgs, FinishArgs, GlobArgs, GrepArgs, ListDirArgs,
    OutlineArgs, ReadFileArgs, RecallArgs, RunShellArgs, SearchCodeArgs, ToolDef, WriteFileArgs,
};

pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file",
            description: "Read a UTF-8 text file (optionally a line range) from the workspace.",
            parameters: schema_of::<ReadFileArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "list_dir",
            description: "List the entries of a directory in the workspace.",
            parameters: schema_of::<ListDirArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "grep",
            description: "Search file contents by regex (gitignore-aware) and return matching lines.",
            parameters: schema_of::<GrepArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "glob",
            description: "Find files whose path matches a glob like `src/**/*.rs`.",
            parameters: schema_of::<GlobArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "edit_file",
            description: "Replace an exact, unique substring in a file. Fails if old_string is missing or not unique.",
            parameters: schema_of::<EditFileArgs>(),
            mutating: true,
        },
        ToolDef {
            name: "write_file",
            description: "Create or overwrite a file with the given full contents.",
            parameters: schema_of::<WriteFileArgs>(),
            mutating: true,
        },
        ToolDef {
            name: "run_shell",
            description: "Run a shell command in the workspace. Requires user approval.",
            parameters: schema_of::<RunShellArgs>(),
            mutating: true,
        },
        ToolDef {
            name: "finish",
            description: "Signal the task is complete. For questions, explanations, or plans, provide the complete, detailed answer in `summary`. For code edits, summarize the changes.",
            parameters: schema_of::<FinishArgs>(),
            mutating: false,
        },
    ]
}

pub fn index_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "find_definition",
            description: "Find where a symbol (function/struct/class/...) is defined, by exact name. Faster and more precise than grep.",
            parameters: schema_of::<FindDefinitionArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "search_code",
            description: "Keyword search over the code index (BM25 ranked), returning line-ranged snippets. Use to locate relevant code by terms/identifiers.",
            parameters: schema_of::<SearchCodeArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "outline",
            description: "List the symbols (and nesting) defined in a file, without reading the whole thing.",
            parameters: schema_of::<OutlineArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "find_mentions",
            description: "Find indexed code locations that mention a symbol name (FTS keyword search — approximate, not call-graph references). Prefer lsp_find_references when LSP is available.",
            parameters: schema_of::<FindMentionsArgs>(),
            mutating: false,
        },
        ToolDef {
            name: "find_references",
            description: "Deprecated alias for find_mentions (keyword mentions, not precise references).",
            parameters: schema_of::<FindMentionsArgs>(),
            mutating: false,
        },
    ]
}

pub fn memory_tool_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "recall",
        description: "Search prior sessions in this workspace for relevant messages and tool outcomes. Use when you need decisions or context from earlier work.",
        parameters: schema_of::<RecallArgs>(),
        mutating: false,
    }]
}

pub fn operator_card(has_index: bool, has_memory: bool, has_lsp: bool) -> String {
    let mut lines = vec![
        "You are Lokai — a local-only coding agent. Hard constraints:".to_string(),
        "- All work stays on this machine; default-deny egress (no cloud APIs).".to_string(),
        "- run_shell requires explicit user approval when enabled.".to_string(),
        "- Project memory holds intent/decisions; the code index holds current code truth — do not confuse them.".to_string(),
    ];
    if has_memory {
        lines.push(
            "- Use `recall` to pull snippets from prior sessions when continuity matters.".into(),
        );
    }
    if has_index {
        lines.push(
            "- Prefer find_definition / search_code / outline over brute grep for navigation."
                .into(),
        );
    }
    if has_lsp {
        lines.push(
            "- Run lsp_diagnostics on edited files before finish when LSP is available.".into(),
        );
    }
    lines.join("\n")
}

pub fn catalog_prompt_lines(has_index: bool, has_memory: bool, has_lsp: bool) -> String {
    let mut out = String::new();
    if has_index {
        out.push_str(
            "- A code index is available: use find_definition to jump to a symbol, search_code \
for keyword search, outline to list a file's symbols, and find_mentions to locate textual uses \
(keyword/FTS — prefer lsp_find_references for precise refs). \
Prefer these over grep for locating code.\n",
        );
    }
    if has_memory {
        out.push_str(
            "- Prior sessions are searchable with the `recall` tool — use it for decisions and context from earlier work.\n",
        );
    }
    if has_lsp {
        out.push_str(
            "- Language servers are available: lsp_goto_definition and lsp_find_references for \
typed navigation; lsp_diagnostics before finish to catch compile/type errors. \
Index tools still work if LSP fails.\n",
        );
    }
    out.push_str(
        "- Python: after edit/write, syntax must be valid (py_compile). Fix SyntaxError/IndentationError before calling finish.\n",
    );
    out
}

pub fn validate_tool_args(name: &str, args: &serde_json::Value) -> Result<(), String> {
    let args = crate::types::coerce_args(args).map_err(|e| e.to_string())?;
    match name {
        "read_file" => serde_json::from_value::<ReadFileArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "list_dir" => serde_json::from_value::<ListDirArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "grep" => serde_json::from_value::<GrepArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "glob" => serde_json::from_value::<GlobArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "edit_file" => serde_json::from_value::<EditFileArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "write_file" => serde_json::from_value::<WriteFileArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "run_shell" => serde_json::from_value::<RunShellArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "finish" => serde_json::from_value::<FinishArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "find_definition" => serde_json::from_value::<FindDefinitionArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "search_code" => serde_json::from_value::<SearchCodeArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "outline" => serde_json::from_value::<OutlineArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "find_mentions" | "find_references" => serde_json::from_value::<FindMentionsArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "lsp_goto_definition" => serde_json::from_value::<crate::lsp::LspPositionArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "lsp_find_references" => serde_json::from_value::<crate::lsp::LspPositionArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "lsp_diagnostics" => serde_json::from_value::<crate::lsp::LspPathArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "spawn_agent" => crate::orchestration::parse_spawn_agent_args(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "recall" => serde_json::from_value::<RecallArgs>(args)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        other => Err(format!("unknown tool '{other}'")),
    }
}
