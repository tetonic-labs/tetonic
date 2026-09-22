use super::*;

fn fixture(count: usize) -> Index {
    let idx = Index {
        conn: Connection::open_in_memory().unwrap(),
        path: PathBuf::from(":memory:"),
        ann: std::cell::RefCell::new(HashMap::new()),
        ann_order: std::cell::RefCell::new(Vec::new()),
    };
    idx.conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    idx.migrate().unwrap();
    let tx = idx.conn.unchecked_transaction().unwrap();
    for i in 0..count {
        index_one_file(
            &tx,
            "repo",
            &format!("repo/{i}.rs"),
            &format!("{i}.rs"),
            Lang::Rust,
            "pub fn alpha() {}\npub fn beta() {}\npub fn gamma() {}",
            "hash",
            Some(1),
            60,
            false,
        )
        .unwrap();
    }
    tx.commit().unwrap();
    idx
}

fn rows(idx: &Index) -> Vec<(i64, String)> {
    idx.conn
        .prepare("SELECT rowid, content FROM fts_chunks ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn lookup_migration_and_purge_preserve_exact_remaining_content() {
    let idx = fixture(20);
    idx.conn
        .execute_batch("DROP TABLE fts_file_rows; DELETE FROM schema_versions WHERE version=4;")
        .unwrap();
    idx.migrate().unwrap();
    let before = rows(&idx);
    let expected = {
        let tx = idx.conn.unchecked_transaction().unwrap();
        tx.execute("DELETE FROM fts_chunks WHERE path=?1", ["repo/7.rs"])
            .unwrap();
        let expected = rows(&idx);
        tx.rollback().unwrap();
        expected
    };
    purge_file_at_path(&idx.conn, "repo/7.rs").unwrap();
    assert_eq!(rows(&idx), expected);
    assert!(rows(&idx).len() < before.len());
    let dangling: i64 = idx
        .conn
        .query_row(
            "SELECT COUNT(*) FROM fts_file_rows WHERE file_path='repo/7.rs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0);
}

#[test]
#[ignore = "repeatable SQL update-path benchmark; run explicitly with --nocapture"]
fn benchmark_index_update_lookup() {
    let idx = fixture(5000);
    let before = rows(&idx);
    let mut legacy = 0.0;
    let mut indexed = 0.0;
    for _ in 0..3 {
        for fast in [false, true] {
            let tx = idx.conn.unchecked_transaction().unwrap();
            let started = std::time::Instant::now();
            for i in 0..80 {
                let path = format!("repo/{i}.rs");
                if fast {
                    purge_file_at_path(&tx, &path).unwrap();
                } else {
                    let [a, b] = path_lookup_keys(&path);
                    tx.execute(
                        "DELETE FROM fts_chunks WHERE path=?1 OR path=?2",
                        params![a, b],
                    )
                    .unwrap();
                    tx.execute("DELETE FROM files WHERE path=?1 OR path=?2", params![a, b])
                        .unwrap();
                }
            }
            let elapsed = started.elapsed().as_secs_f64();
            if fast {
                indexed += elapsed;
            } else {
                legacy += elapsed;
            }
            tx.rollback().unwrap();
            assert_eq!(rows(&idx), before);
        }
    }
    println!("index replacement lookup: legacy_ms={:.2} indexed_ms={:.2} speedup={:.2}x (5000 files, 80 deletions x 3; SQL path only)", legacy*1000.0, indexed*1000.0, legacy/indexed);
}
