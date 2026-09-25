//! Read-only / offline commands (history, time travel, index, project memory) — infrastructure-only.

use std::io::Write;

use anyhow::Result;
use tetonic_app::Application;

use crate::args::Args;

pub async fn project_memory(args: &Args) -> Result<()> {
    let app = Application::bootstrap_offline(Some(&args.workspace)).await?;
    let ws = resolve_workspace(&app, args)?;
    if let Some(note) = &args.project_note {
        app.add_project_note(&ws, note, "user")?;
        println!("project note added for {ws}");
        return Ok(());
    }
    if args.project_status {
        match app.project_status(&ws)? {
            Some(st) => {
                println!("project: {}", st.name);
                println!("  id:          {}", st.id);
                println!("  root:        {}", st.root);
                println!("  digest:      {} chars", st.digest_chars);
                println!("  notes:       {}", st.note_count);
                println!("  last active: {}", st.last_active_at);
            }
            None => {
                let id = app.ensure_project(&ws)?;
                println!("no project memory yet for {ws}");
                println!("  created project id: {id}");
            }
        }
    }
    Ok(())
}

pub async fn show_history(session: Option<&str>) -> Result<()> {
    let app = Application::bootstrap_offline(None).await?;
    match session {
        Some(id) => {
            let rows = app.session_transcript(id)?;
            if rows.is_empty() {
                println!("no session '{id}' (or it has no messages)");
                return Ok(());
            }
            println!("transcript of {id} ({} messages):\n", rows.len());
            for (seq, role, content) in rows {
                let body = content.replace('\n', " ");
                let body: String = body.chars().take(100).collect();
                println!("  {seq:>3} {role:<9} {body}");
            }
        }
        None => {
            let rows = app.list_recent_sessions(20)?;
            if rows.is_empty() {
                println!("no sessions recorded yet");
                return Ok(());
            }
            println!("recent sessions (newest first)\n");
            for s in rows {
                println!(
                    "  {}  {:<8} {:>3} msgs  {:>2} tools  {:>2} file-changes  {}\n      {}",
                    s.id,
                    s.status,
                    s.messages,
                    s.tool_calls,
                    s.file_changes,
                    s.started_at.chars().take(19).collect::<String>(),
                    s.workspace_root,
                );
            }
        }
    }
    Ok(())
}

pub async fn time_travel(args: &Args) -> Result<()> {
    let app = Application::bootstrap_offline(Some(&args.workspace)).await?;
    let ws = resolve_workspace(&app, args)?;

    if let Some(label) = &args.checkpoint {
        let (id, mark) = app.create_checkpoint(&ws, label)?;
        println!("created checkpoint {id} \"{label}\" at mark {mark}");
        return Ok(());
    }

    if args.checkpoints {
        let report = app.list_checkpoints(&ws)?;
        println!("checkpoints for {ws}");
        match report.redo_target {
            Some(r) => println!(
                "  current head: mark {}  (redo available -> mark {r})\n",
                report.current_head
            ),
            None => println!("  current head: mark {}\n", report.current_head),
        }
        if report.checkpoints.is_empty() {
            println!(
                "  (none yet — create one with `tetonic --checkpoint \"label\" --workspace <dir>`)"
            );
            return Ok(());
        }
        for c in report.checkpoints {
            let here = if c.mark == report.current_head {
                "  <- current"
            } else {
                ""
            };
            let when = c.created_at.chars().take(19).collect::<String>();
            println!("  {}  mark {:>4}  {when}  {}{here}", c.id, c.mark, c.label);
        }
        return Ok(());
    }

    if let Some(target_ref) = &args.restore {
        let summary = app.restore_checkpoint(&ws, target_ref, args.dry_run)?;
        print_restore_summary(&summary, &ws);
        return Ok(());
    }

    if args.redo {
        let summary = app.redo(&ws, args.dry_run)?;
        print_restore_summary(&summary, &ws);
        return Ok(());
    }

    if args.undo {
        let summary = app.undo(&ws, args.dry_run)?;
        print_restore_summary(&summary, &ws);
        return Ok(());
    }

    Ok(())
}

fn print_restore_summary(summary: &tetonic_app::RestoreSummary, ws: &str) {
    println!(
        "{} {} file change(s): mark {} -> {}",
        if summary.dry_run {
            "would apply"
        } else {
            &summary.reason
        },
        summary.changes.len(),
        summary.from_mark,
        summary.to_mark
    );
    println!("  workspace: {ws}\n");
    for ch in &summary.changes {
        if let Some(ref err) = ch.error {
            eprintln!("  FAIL   {} ({err})", ch.path);
        } else {
            println!("  {} {}", ch.verb, ch.path);
        }
    }
    if summary.dry_run {
        println!(
            "\n[dry run] {} change(s) would be applied, {} skipped",
            summary.applied, summary.skipped
        );
    }
}

pub async fn code_index(args: &Args) -> Result<()> {
    let app = Application::bootstrap_offline(Some(&args.workspace)).await?;

    if args.gc_index {
        let n = app.prune_index_workspaces()?;
        println!("index GC: pruned {n} missing workspace(s)");
        return Ok(());
    }

    let ws = resolve_workspace(&app, args)?;

    if args.watch_index {
        println!("watching {ws} for file changes (Ctrl+C to stop)…");
        app.watch_index_blocking(&ws)?;
        return Ok(());
    }

    if args.index || args.embed {
        if args.index {
            let stats = app.index_workspace(&ws)?;
            println!("indexed {ws}");
            println!(
                "  {} changed, {} unchanged, {} skipped, {} removed — {} symbols in {} ms",
                stats.indexed,
                stats.unchanged,
                stats.skipped,
                stats.deleted,
                stats.symbols,
                stats.elapsed_ms
            );
        }
        if args.embed {
            let (embedded, emb, tot, requests) = app
                .embed_workspace(&ws, &args.embed_model, |cur, total| {
                    print!("\r  embedding... {cur}/{total}");
                    let _ = std::io::stdout().flush();
                })
                .await?;
            println!();
            println!(
                "  embedded {embedded} chunk(s); coverage {emb}/{tot} ('{}', {requests} request(s))",
                args.embed_model
            );
        }
        return Ok(());
    }

    if args.index_status {
        let s = app.index_status(&ws, &args.embed_model)?;
        println!("index status for {ws}");
        println!(
            "  files: {}   symbols: {}   chunks: {}",
            s.files, s.symbols, s.chunks
        );
        if let Some(t) = s.last_indexed {
            println!("  last indexed: {}", t.chars().take(19).collect::<String>());
        }
        if s.embedded_chunks > 0 {
            println!(
                "  embedded: {}/{} chunks ({})",
                s.embedded_chunks, s.total_chunks, args.embed_model
            );
        }
        for (lang, n) in s.by_lang {
            println!("  {lang:<8} {n}");
        }
        if s.files == 0 {
            println!("  (empty — run `tetonic --index --workspace <dir>`)");
        }
        return Ok(());
    }

    if let Some(name) = &args.def {
        let rows = app.find_definition(&ws, name)?;
        if rows.is_empty() {
            println!("no definition of '{name}' found (is the workspace indexed? `tetonic --index`)");
        }
        for r in rows {
            println!(
                "  {:<8} {}:{}   {}",
                r.kind, r.rel, r.start_line, r.signature
            );
        }
        return Ok(());
    }

    if let Some(name) = &args.refs {
        let hits = app.find_mentions(&ws, name, 50)?;
        if hits.is_empty() {
            println!("no references to '{name}' found");
        }
        for h in hits {
            println!("  {}:{}   {}", h.rel, h.start_line, h.preview);
        }
        return Ok(());
    }

    if let Some(path) = &args.outline {
        let rows = app.index_outline(&ws, path)?;
        if rows.is_empty() {
            println!("no outline for '{path}' (not indexed, or no symbols)");
        }
        for r in rows {
            let indent = "  ".repeat(r.depth + 1);
            println!("{indent}{:<8} {}   (L{})", r.kind, r.name, r.start_line);
        }
        return Ok(());
    }

    if let Some(q) = &args.search {
        if args.semantic {
            let (hits, reqs) = app
                .index_semantic_search(&ws, &args.embed_model, q, 25)
                .await?;
            println!("semantic search '{q}' ({reqs} request(s) to local runtime)");
            if hits.is_empty() {
                println!("no matches for '{q}'");
            }
            for h in hits {
                let sym = if h.symbol_name.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", h.symbol_name)
                };
                println!(
                    "  {:.3}  {}:{}-{}{}   {}",
                    h.score.unwrap_or(0.0),
                    h.rel,
                    h.start_line,
                    h.end_line,
                    sym,
                    h.preview
                );
            }
        } else {
            let hits = app.index_search(&ws, q, 25)?;
            if hits.is_empty() {
                println!("no matches for '{q}'");
            }
            for h in hits {
                let sym = if h.symbol_name.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", h.symbol_name)
                };
                println!(
                    "  {}:{}-{}{}   {}",
                    h.rel, h.start_line, h.end_line, sym, h.preview
                );
            }
        }
        return Ok(());
    }

    Ok(())
}

fn resolve_workspace(app: &Application, args: &Args) -> Result<String> {
    if args.workspace != "." {
        Ok(std::path::Path::new(&args.workspace)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&args.workspace))
            .display()
            .to_string())
    } else {
        match app.resolve_latest_workspace()? {
            Some(w) => Ok(w),
            None => Ok(std::env::current_dir()?.display().to_string()),
        }
    }
}
