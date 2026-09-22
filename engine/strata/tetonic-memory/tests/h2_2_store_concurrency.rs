//! H2-2: writer actor stays off the analytical-read critical path; WAL recovers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tetonic_memory::SharedStore;

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("lokai.db");
    (dir, path)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_recall_does_not_head_of_line_block_appends() {
    let (_dir, path) = temp_db();
    let store = SharedStore::open(&path, 2).expect("open shared store");

    // Seed FTS corpus in a prior session so recall has work to do.
    let ws = path.parent().unwrap().to_path_buf();
    let seed_sid = store
        .write({
            let ws = ws.clone();
            move |db| {
                let sid = db
                    .start_session(ws.to_str().unwrap(), "single-agent", "mock")
                    .unwrap();
                for i in 0..40 {
                    let body = format!(
                        "alpha beta gamma recall corpus line {i} {}",
                        "x".repeat(2_000)
                    );
                    db.append_message(&sid, "user", "", &body, None).unwrap();
                }
                db.end_session(&sid, "ok", None).unwrap();
                sid
            }
        })
        .await
        .unwrap();

    let live_sid = store
        .write({
            let ws = ws.clone();
            move |db| {
                db.start_session(ws.to_str().unwrap(), "single-agent", "mock")
                    .unwrap()
            }
        })
        .await
        .unwrap();

    let max_append_ms = Arc::new(AtomicU64::new(0));
    let store_r = store.clone();
    let ws_r = ws.clone();
    let seed = seed_sid.clone();
    let readers = tokio::spawn(async move {
        for _ in 0..8 {
            let ws = ws_r.clone();
            let seed = seed.clone();
            let _ = store_r
                .read(move |db| {
                    // Hold the reader while doing repeated FTS — simulates
                    // capacity/briefing analytical load.
                    for _ in 0..6 {
                        let _ = db.recall_history(&ws, "alpha beta gamma", 20, Some(&seed));
                    }
                    std::thread::sleep(Duration::from_millis(40));
                })
                .await;
        }
    });

    let mut latencies = Vec::new();
    for i in 0..24 {
        let sid = live_sid.clone();
        let started = Instant::now();
        store
            .write(move |db| {
                db.append_message(&sid, "user", "", &format!("live append {i}"), None)
                    .unwrap();
            })
            .await
            .unwrap();
        let ms = started.elapsed().as_millis() as u64;
        latencies.push(ms);
        max_append_ms.fetch_max(ms, Ordering::Relaxed);
    }

    readers.await.expect("reader task");

    let max = *latencies.iter().max().unwrap();
    let p50 = {
        let mut s = latencies.clone();
        s.sort_unstable();
        s[s.len() / 2]
    };
    assert!(
        max < 750,
        "append max latency {max}ms (p50={p50}ms) — slow FTS must not HOL-block the writer; latencies={latencies:?}"
    );
    assert!(
        p50 < 250,
        "append p50 {p50}ms too high under concurrent recall; latencies={latencies:?}"
    );
}

#[test]
fn wal_survives_abrupt_shared_store_drop() {
    let (_dir, path) = temp_db();
    let sid = {
        let store = SharedStore::open(&path, 1).expect("open");
        store
            .write_sync(|db| {
                let sid = db
                    .start_session("/tmp/wal-crash", "single-agent", "mock")
                    .unwrap();
                db.append_message(&sid, "user", "", "committed before crash", None)
                    .unwrap();
                sid
            })
            .unwrap()
    };
    // Dropping SharedStore closes the writer channel; WAL must still reopen cleanly.
    let store = SharedStore::open(&path, 1).expect("reopen after abrupt drop");
    let content = store
        .read_sync(move |db| {
            let rows = db.list_messages_for_resume(&sid, 10).unwrap();
            rows.into_iter().map(|r| r.content).collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(content, vec!["committed before crash".to_string()]);

    let mode = store.read_sync(|db| db.journal_mode().unwrap()).unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");
}

#[tokio::test]
async fn write_ordering_is_fifo_on_shared_store() {
    let (_dir, path) = temp_db();
    let store = SharedStore::open(&path, 1).unwrap();
    let sid = store
        .write(|db| {
            db.start_session("/tmp/order", "single-agent", "mock")
                .unwrap()
        })
        .await
        .unwrap();

    let mut handles = Vec::new();
    for i in 0..20 {
        let store = store.clone();
        let sid = sid.clone();
        handles.push(tokio::spawn(async move {
            store
                .write(move |db| {
                    db.append_message(&sid, "user", "", &format!("msg-{i:02}"), None)
                        .unwrap();
                })
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let texts = store
        .read({
            let sid = sid.clone();
            move |db| {
                db.list_messages_for_resume(&sid, 50)
                    .unwrap()
                    .into_iter()
                    .map(|m| m.content)
                    .collect::<Vec<_>>()
            }
        })
        .await
        .unwrap();
    let expected: Vec<_> = (0..20).map(|i| format!("msg-{i:02}")).collect();
    assert_eq!(texts, expected);
}
