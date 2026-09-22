//! `tetonic-bench`: a repeatable micro-benchmark of the CPU-bound hot paths that
//! govern *editor responsiveness* (indexing on save, retrieval latency, context
//! assembly, semantic search). It deliberately does NOT touch the model: the LLM
//! round-trip dominates end-to-end agent latency but is Ollama's cost, not ours.
//!
//! Usage: `tetonic-bench [num_files] [embed_dim]`  (defaults: 1500 files, 768 dims)
//!
//! Everything runs against a generated synthetic repo in a temp dir, so numbers
//! are comparable across runs and machines. Run under different profiles to
//! compare codegen settings, e.g.:
//!   cargo run -p tetonic-bench --release          # shipped profile (opt-level "z")
//!   cargo run -p tetonic-bench --profile perf     # opt-level 3

use std::path::{Path, PathBuf};
use std::time::Instant;

use lokai_core::{HeuristicTokenizer, Tokenizer};
use lokai_index::Index;
use lokai_tools::{Tools, Workspace};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let n_files: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    let dim: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(768);

    let root = std::env::temp_dir().join(format!("tetonic-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let db = std::env::temp_dir().join(format!("tetonic-bench-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);

    println!("tetonic-bench: synthetic repo: {n_files} files, embed dim {dim}");
    println!("{}", "=".repeat(64));

    // ---- corpus generation (not counted) -----------------------------------
    let total_bytes = gen_corpus(&root, n_files)?;
    println!(
        "corpus: {n_files} files, {:.1} MB on disk\n",
        total_bytes as f64 / 1e6
    );

    let ws = root.to_string_lossy().to_string();
    let idx = Index::open(&db)?;

    // ---- 1. cold index -----------------------------------------------------
    let (cold, st) = timed(|| idx.index_workspace(&root));
    let st = st?;
    row("index: cold (parse + SQL)", cold, n_files);
    println!(
        "          -> {} indexed, {} symbols, {} skipped",
        st.indexed, st.symbols, st.skipped
    );

    // ---- 2. warm index (mtime/size fast path) ------------------------------
    let (warm, st) = timed(|| idx.index_workspace(&root));
    let st = st?;
    row("index: warm (no-op, fast path)", warm, n_files);
    println!(
        "          -> {} unchanged, {} indexed",
        st.unchanged, st.indexed
    );

    // ---- 3. incremental: edit 1% of files ----------------------------------
    let edited = edit_fraction(&root, n_files, 100)?; // ~1%
    let (inc, st) = timed(|| idx.index_workspace(&root));
    let st = st?;
    row(
        &format!("index: incremental ({edited} edited)"),
        inc,
        edited.max(1),
    );
    println!(
        "          -> {} re-indexed, {} unchanged",
        st.indexed, st.unchanged
    );

    // ---- 4. retrieval: structural + keyword --------------------------------
    let tools = Tools::new(Workspace::new(&root)?, false);
    bench_many("retrieval: find_definition", 200, || {
        let _ = idx.find_definition(&ws, "process").unwrap();
    });
    bench_many("retrieval: search (FTS/BM25)", 200, || {
        let _ = idx.search(&ws, "process total", 10).unwrap();
    });
    bench_many("retrieval: outline (one file)", 200, || {
        let _ = idx.outline(&ws, "pkg0/mod_0.rs").unwrap();
    });

    // ---- 5. grep (parallel walk + regex over whole tree) -------------------
    let (g, _) = timed(|| {
        let out = tools.execute(
            "grep",
            &json!({ "pattern": "fn process|def process", "max_results": 100000 }),
        );
        Ok::<_, anyhow::Error>(out.content.lines().count())
    });
    row("grep: regex over whole tree", g, n_files);

    // ---- 6. semantic: store + brute-force search ---------------------------
    let pending = idx.pending_embeddings(&ws, "bench-embed", 1_000_000)?;
    let m = pending.len();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (store, _) = timed(|| {
        for p in &pending {
            let v: Vec<f32> = (0..dim).map(|_| rng.unit()).collect();
            idx.store_embedding(p.chunk_id, "bench-embed", &v)?;
        }
        Ok::<_, anyhow::Error>(())
    });
    row(
        &format!("embed: store {m} vectors (dim {dim})"),
        store,
        m.max(1),
    );

    let query: Vec<f32> = (0..dim).map(|_| rng.unit()).collect();
    bench_many("semantic: brute-force cosine search", 50, || {
        let _ = idx.semantic_search(&ws, "bench-embed", &query, 10).unwrap();
    });
    println!("          -> {m} vectors scanned per query");

    // ---- 7. tokenizer throughput (drives context budgeting) ----------------
    let blob = read_all(&root, 4_000_000);
    let tok = HeuristicTokenizer;
    let (tk, count) = timed(|| Ok::<_, anyhow::Error>(tok.count(&blob)));
    let count = count?;
    let mbps = (blob.len() as f64 / 1e6) / (tk.as_secs_f64().max(1e-9));
    println!(
        "tokenizer: heuristic count {:.2} ms  ({:.0} MB/s, {} tokens over {:.1} MB)",
        tk.as_secs_f64() * 1e3,
        mbps,
        count,
        blob.len() as f64 / 1e6
    );

    // ---- footprint ---------------------------------------------------------
    let db_size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    let db_with_vecs = db_size as f64 / 1e6;
    println!(
        "\nindex.db footprint: {:.1} MB ({} files + {} vectors)",
        db_with_vecs, n_files, m
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(db.with_extension("db-wal"));
    let _ = std::fs::remove_file(db.with_extension("db-shm"));
    Ok(())
}

// ---- helpers ---------------------------------------------------------------

fn timed<T>(f: impl FnOnce() -> T) -> (std::time::Duration, T) {
    let t = Instant::now();
    let r = f();
    (t.elapsed(), r)
}

/// Print one timing row with a per-unit derived rate.
fn row(label: &str, d: std::time::Duration, units: usize) {
    let ms = d.as_secs_f64() * 1e3;
    let per = d.as_secs_f64() / units as f64;
    let rate = if per > 0.0 { 1.0 / per } else { 0.0 };
    println!("{label:<38} {ms:>9.2} ms   ({rate:>9.0}/s)");
}

/// Run `f` `iters` times, report total + per-call latency.
fn bench_many(label: &str, iters: usize, mut f: impl FnMut()) {
    // one warmup
    f();
    let t = Instant::now();
    for _ in 0..iters {
        f();
    }
    let d = t.elapsed();
    let per_us = d.as_secs_f64() * 1e6 / iters as f64;
    println!("{label:<38} {per_us:>9.1} µs/call ({iters} calls)");
}

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// A value in [-1, 1).
    fn unit(&mut self) -> f32 {
        let u = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32; // [0,1)
        u * 2.0 - 1.0
    }
}

fn gen_corpus(root: &Path, n: usize) -> std::io::Result<u64> {
    let mut total = 0u64;
    for i in 0..n {
        let dir = root.join(format!("pkg{}", i % 50));
        std::fs::create_dir_all(&dir)?;
        let (name, body) = if i % 2 == 0 {
            (format!("mod_{i}.rs"), rust_file(i))
        } else {
            (format!("mod_{i}.py"), python_file(i))
        };
        total += body.len() as u64;
        std::fs::write(dir.join(name), body)?;
    }
    Ok(total)
}

fn rust_file(i: usize) -> String {
    format!(
        "use std::collections::HashMap;\nuse std::fmt;\n\n\
         pub struct Widget{i} {{\n    pub id: u64,\n    pub name: String,\n    pub tags: Vec<String>,\n}}\n\n\
         impl Widget{i} {{\n    \
         pub fn new(id: u64) -> Self {{ Self {{ id, name: String::new(), tags: Vec::new() }} }}\n    \
         pub fn render(&self) -> String {{ format!(\"widget {{}}\", self.id) }}\n    \
         pub fn process(&mut self, n: usize) -> usize {{\n        \
         let mut total = 0usize;\n        \
         for k in 0..n {{ total = total.wrapping_add(k * self.id as usize); }}\n        \
         total\n    }}\n}}\n\n\
         pub fn helper_{i}(x: i32) -> i32 {{ x * 2 + {i} }}\n\n\
         pub const SEED_{i}: u64 = {i};\n"
    )
}

fn python_file(i: usize) -> String {
    format!(
        "import os\nimport sys\n\n\
         class Widget{i}:\n    \
         def __init__(self, id):\n        self.id = id\n        self.name = \"\"\n        self.tags = []\n\n    \
         def render(self):\n        return f\"widget {{self.id}}\"\n\n    \
         def process(self, n):\n        total = 0\n        for k in range(n):\n            total += k * self.id\n        return total\n\n\n\
         def helper_{i}(x):\n    return x * 2 + {i}\n\n\
         SEED_{i} = {i}\n"
    )
}

/// Re-write every `step`-th file (changing content so the hash differs).
fn edit_fraction(root: &Path, n: usize, step: usize) -> std::io::Result<usize> {
    let mut count = 0;
    for i in (0..n).step_by(step) {
        let dir = root.join(format!("pkg{}", i % 50));
        let (name, body) = if i % 2 == 0 {
            (
                format!("mod_{i}.rs"),
                format!("{}\n// edit nonce {}\n", rust_file(i), i * 7 + 1),
            )
        } else {
            (
                format!("mod_{i}.py"),
                format!("{}\n# edit nonce {}\n", python_file(i), i * 7 + 1),
            )
        };
        std::fs::write(dir.join(name), body)?;
        count += 1;
    }
    Ok(count)
}

/// Concatenate file contents up to `cap` bytes (for tokenizer throughput).
fn read_all(root: &Path, cap: usize) -> String {
    let mut buf = String::new();
    for entry in ignore_walk(root) {
        if buf.len() >= cap {
            break;
        }
        if let Ok(s) = std::fs::read_to_string(&entry) {
            buf.push_str(&s);
        }
    }
    buf
}

fn ignore_walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
    }
    out
}
