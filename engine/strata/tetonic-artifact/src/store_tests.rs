use super::*;
use tetonic_domain::artifact::{ArtifactKind, RetentionPolicy};
use tetonic_domain::classify::DataClass;
use tetonic_domain::ids::{AttemptId, RunId, TaskId};

fn test_declaration() -> ArtifactDeclaration {
    ArtifactDeclaration {
        kind: ArtifactKind::FileSnapshot,
        producer_run_id: RunId::new("run_1"),
        producer_task_id: TaskId::new("task_1"),
        producer_attempt_id: AttemptId::new("attempt_1"),
        worker_id: None,
        workspace_version: None,
        data_class: DataClass::Secret,
        retention_policy: RetentionPolicy::ProjectHistory,
    }
}

/// A scanner that finds nothing. Sealing now requires a policy, and this is
/// the neutral one; a test that wants a detection supplies its own.
fn clean_scan() -> ScanPolicy {
    ScanPolicy::Scan(Arc::new(|_| false))
}

#[tokio::test]
async fn acceptance_refuses_missing_or_corrupted_content_without_advancing_metadata() {
    for missing in [false, true] {
        let (_dir, store) = create_test_store().await;
        let mut writer = store.begin_write(test_declaration()).await.unwrap();
        writer.write_chunk(b"original").await.unwrap();
        let meta = writer.seal().await.unwrap();
        let object = store.object_path(&meta.artifact_id);
        if missing {
            std::fs::remove_file(&object).unwrap();
        } else {
            std::fs::write(&object, b"tampered").unwrap();
        }
        assert!(store.mark_accepted(&meta.artifact_id).await.is_err());
        assert_eq!(
            store
                .metadata(&meta.artifact_id)
                .await
                .unwrap()
                .lifecycle_state,
            ArtifactState::Sealed
        );
        std::fs::write(&object, b"original").unwrap();
        store.mark_accepted(&meta.artifact_id).await.unwrap();
        let reopened = LocalArtifactStore::new(store.root_dir(), clean_scan()).unwrap();
        assert_eq!(
            reopened
                .metadata(&meta.artifact_id)
                .await
                .unwrap()
                .lifecycle_state,
            ArtifactState::Accepted
        );
        reopened.open(&meta.artifact_id).await.unwrap();
    }
}

fn scan_for(needle: &'static str) -> ScanPolicy {
    ScanPolicy::Scan(Arc::new(move |text: &str| text.contains(needle)))
}

async fn create_test_store() -> (tempfile::TempDir, LocalArtifactStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalArtifactStore::new(dir.path(), clean_scan()).unwrap();
    (dir, store)
}

async fn store_with(scan: ScanPolicy) -> (tempfile::TempDir, LocalArtifactStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalArtifactStore::new(dir.path(), scan).unwrap();
    (dir, store)
}

#[tokio::test]
async fn test_interrupted_artifact_upload() {
    let (_dir, store) = create_test_store().await;
    let mut writer = store.begin_write(test_declaration()).await.unwrap();
    writer.write_chunk(b"hello").await.unwrap();

    let res = writer.abandon().await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_oversized_artifact_rejection() {
    let (_dir, store) = create_test_store().await;
    let mut writer = store.begin_write(test_declaration()).await.unwrap();

    let big_chunk = vec![0u8; 50 * 1024 * 1024];
    writer.write_chunk(&big_chunk).await.unwrap();
    writer.write_chunk(&big_chunk).await.unwrap();

    let res = writer.write_chunk(&big_chunk).await;
    assert!(matches!(res, Err(ArtifactError::SizeLimitExceeded(_))));
}

#[tokio::test]
async fn test_changing_artifact_content_changes_identity() {
    let (_dir, store) = create_test_store().await;

    let mut w1 = store.begin_write(test_declaration()).await.unwrap();
    w1.write_chunk(b"foo").await.unwrap();
    let meta1 = w1.seal().await.unwrap();

    let mut w2 = store.begin_write(test_declaration()).await.unwrap();
    w2.write_chunk(b"bar").await.unwrap();
    let meta2 = w2.seal().await.unwrap();

    assert_ne!(meta1.artifact_id, meta2.artifact_id);
    assert_ne!(meta1.content_digest, meta2.content_digest);
}

#[tokio::test]
async fn test_artifact_classification_propagation() {
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"secret").await.unwrap();
    let meta = w.seal().await.unwrap();
    assert_eq!(meta.data_class, DataClass::Secret);
}
#[tokio::test]
async fn test_failed_content_deletion() {
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"to delete").await.unwrap();
    let meta = w.seal().await.unwrap();

    // A directory at the object path causes remove_file to fail on all platforms.
    let obj_path = store.object_path(&meta.artifact_id);
    std::fs::remove_file(&obj_path).unwrap();
    std::fs::create_dir(&obj_path).unwrap(); // Now remove_file will fail on it because it's a directory

    let res = store.delete(&meta.artifact_id).await;
    assert!(res.is_err());
    assert!(store.deletion_path(&meta.artifact_id).exists());

    // Revoke readable metadata before deleting content; retain the failed object
    // for repair rather than exposing a potentially dangling live record.
    let meta_path = store.meta_path(&meta.artifact_id);
    assert!(!meta_path.exists());
    assert!(obj_path.exists());
    assert!(store.open(&meta.artifact_id).await.is_err());
    std::fs::remove_dir(&obj_path).unwrap();
    std::fs::write(&obj_path, b"to delete").unwrap();
    let reopened = LocalArtifactStore::new(store.root_dir(), clean_scan()).unwrap();
    let report = crate::gc::enforce_at_startup(&reopened, reopened.quota()).unwrap();
    assert!(report.deleted.contains(&meta.artifact_id.0));
    assert!(!obj_path.exists());
    assert!(!store.deletion_path(&meta.artifact_id).exists());
    store.delete(&meta.artifact_id).await.unwrap();
}

#[tokio::test]
async fn invalid_deletion_record_cannot_authorize_cleanup() {
    let (_dir, store) = create_test_store().await;
    let mut writer = store.begin_write(test_declaration()).await.unwrap();
    writer.write_chunk(b"keep").await.unwrap();
    let meta = writer.seal().await.unwrap();
    let marker = store.deletion_path(&meta.artifact_id);
    std::fs::write(&marker, b"unrecognized record").unwrap();
    assert!(crate::gc::enforce_at_startup(&store, store.quota()).is_err());
    assert!(store.object_path(&meta.artifact_id).exists());
    assert_eq!(
        store
            .metadata(&meta.artifact_id)
            .await
            .unwrap()
            .content_digest,
        meta.content_digest
    );
    assert!(store.open(&meta.artifact_id).await.is_err());
    assert!(store.mark_accepted(&meta.artifact_id).await.is_err());
    // Even after bytes/metadata disappear, an outstanding intent reserves the ID.
    std::fs::remove_file(store.object_path(&meta.artifact_id)).unwrap();
    std::fs::remove_file(store.meta_path(&meta.artifact_id)).unwrap();
    assert!(store
        .persist_quarantined_remote(meta, b"keep")
        .await
        .is_err());
}

/// INV-ART-001 — `open` must refuse content that does not match its seal
/// digest.
///
/// **This test was inverted in M6.** It previously opened the corrupted
/// artifact, hashed the bytes it received, and asserted the digest differed
/// from the metadata — that is, it asserted the store hands out corrupt
/// content and then proved the corruption itself. It passed, and it was
/// cited as coverage for an invariant it contradicted.
#[tokio::test]
async fn open_refuses_content_that_does_not_match_its_seal_digest() {
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"original data").await.unwrap();
    let meta = w.seal().await.unwrap();

    let obj_path = store.object_path(&meta.artifact_id);
    std::fs::write(&obj_path, b"corrupted data").unwrap();

    let Err(err) = store.open(&meta.artifact_id).await else {
        panic!("open must refuse corrupted content, not return a reader over it");
    };
    assert!(
        matches!(err, ArtifactError::DigestMismatch { .. }),
        "expected DigestMismatch, got {err:?}"
    );
}

#[tokio::test]
async fn open_succeeds_on_intact_content() {
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"original data").await.unwrap();
    let meta = w.seal().await.unwrap();

    let mut reader = store.open(&meta.artifact_id).await.unwrap();
    let mut buf = vec![0u8; 1024];
    let n = reader.read_chunk(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"original data");
}

/// S-7 — a store built without a scanner refuses to seal rather than sealing
/// unscanned content.
#[tokio::test]
async fn seal_refuses_when_no_scanner_was_supplied() {
    let (_dir, store) = store_with(ScanPolicy::Refuse).await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"anything").await.unwrap();
    let err = w.seal().await.unwrap_err();
    assert!(
        format!("{err}").contains("without a secret scanner"),
        "expected a scanner refusal, got {err}"
    );
}

/// S-8, non-vacuous case — a secret straddling a chunk boundary in an
/// artifact **larger than 1 MiB**.
///
/// Pre-M6 this sealed successfully: `seal` skipped the scan entirely above
/// 1 MiB. This is the case that proves the exemption was a hole, as opposed
/// to the sub-1-MiB straddling case below, which was already detected
/// because the old scan read the whole file at once.
#[tokio::test]
async fn oversized_artifact_with_a_straddling_secret_is_refused() {
    let (_dir, store) = store_with(scan_for("SECRET_TOKEN")).await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();

    w.write_chunk(&vec![b'a'; 1024 * 1024 + 512]).await.unwrap();
    w.write_chunk(b"SECRET_").await.unwrap();
    w.write_chunk(b"TOKEN").await.unwrap();

    let err = w.seal().await.unwrap_err();
    assert!(
        format!("{err}").contains("secrets"),
        "expected a secret refusal, got {err}"
    );
}

/// S-8, regression guard — the sub-1-MiB straddling case.
///
/// This passed before M6 too, because the old scan hashed the whole file in
/// one call. It is kept to guard the chunking that S-8 *introduced*, and it
/// is what pins `SCAN_OVERLAP_BYTES`: the split has to remain detectable
/// once the scanner only ever sees a window.
#[tokio::test]
async fn small_artifact_with_a_straddling_secret_is_still_refused() {
    let (_dir, store) = store_with(scan_for("SECRET_TOKEN")).await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"prefix SECRET_").await.unwrap();
    w.write_chunk(b"TOKEN suffix").await.unwrap();
    assert!(w.seal().await.is_err());
}

/// S-8 — non-UTF8 content is scanned, not exempted.
///
/// Pre-M6 `seal` did `str::from_utf8` and silently skipped the scan when it
/// failed, so a credential embedded in a binary artifact was never seen.
#[tokio::test]
async fn non_utf8_artifact_is_scanned_for_embedded_secrets() {
    let (_dir, store) = store_with(scan_for("SECRET_TOKEN")).await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    let mut data = vec![0xff, 0xfe, 0x00, 0x80];
    data.extend_from_slice(b"SECRET_TOKEN");
    data.extend_from_slice(&[0xff, 0x00]);
    w.write_chunk(&data).await.unwrap();
    assert!(
        w.seal().await.is_err(),
        "a secret inside non-UTF8 content must still be found"
    );
}

/// The overlap window is a stated bound, so state it in a test: a secret
/// longer than the carry cannot be guaranteed across a boundary, and the
/// scan must still not produce false positives on clean content.
#[tokio::test]
async fn clean_oversized_artifact_still_seals() {
    let (_dir, store) = store_with(scan_for("SECRET_TOKEN")).await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    for _ in 0..3 {
        w.write_chunk(&vec![b'z'; 512 * 1024]).await.unwrap();
    }
    assert!(w.seal().await.is_ok());
}

#[tokio::test]
async fn startup_gc_cleans_orphans_but_preserves_unknown_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalArtifactStore::new(dir.path(), clean_scan())
        .unwrap()
        .with_quota(crate::gc::ArtifactGcConfig {
            max_total_bytes: 10 * 1024 * 1024,
        });
    std::fs::write(store.tmp_path("orphan"), b"abandoned").unwrap();

    let mut decl = test_declaration();
    decl.retention_policy = RetentionPolicy::Ephemeral;
    let mut w = store.begin_write(decl).await.unwrap();
    w.write_chunk(b"ephemeral-bytes").await.unwrap();
    let meta = w.seal().await.unwrap();

    let report = crate::gc::enforce_at_startup(&store, store.quota()).unwrap();
    assert_eq!(report.cleaned_tmp, 1);
    assert!(report.deleted.is_empty());
    assert_eq!(
        store.metadata(&meta.artifact_id).await.unwrap().artifact_id,
        meta.artifact_id
    );
}

#[tokio::test]
async fn quota_blocks_seal_when_over_budget() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalArtifactStore::new(dir.path(), clean_scan())
        .unwrap()
        .with_quota(crate::gc::ArtifactGcConfig { max_total_bytes: 8 });
    let mut w1 = store.begin_write(test_declaration()).await.unwrap();
    w1.write_chunk(b"12345678").await.unwrap(); // 8 bytes — fills quota
    w1.seal().await.unwrap();

    let mut w2 = store.begin_write(test_declaration()).await.unwrap();
    w2.write_chunk(b"x").await.unwrap();
    let err = w2.seal().await.unwrap_err();
    assert!(
        matches!(err, ArtifactError::QuotaExceeded { .. }),
        "expected QuotaExceeded, got {err}"
    );
}

#[tokio::test]
async fn seal_accept_provenance_round_trip() {
    use tetonic_domain::artifact::{provenance_trust_label, ProvenanceTrustLabel};
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"accepted-patch").await.unwrap();
    let sealed = w.seal().await.unwrap();
    assert_eq!(
        provenance_trust_label(&sealed),
        ProvenanceTrustLabel::LocalSealed
    );

    let accepted = store.mark_accepted(&sealed.artifact_id).await.unwrap();
    assert_eq!(accepted.lifecycle_state, ArtifactState::Accepted);
    assert_eq!(
        accepted.verification_state,
        VerificationState::LocallyVerified
    );

    let bundle = store
        .reconstruct_provenance(&sealed.artifact_id)
        .await
        .unwrap();
    assert_eq!(bundle.artifact_id, sealed.artifact_id);
    assert_eq!(bundle.producer_attempt_id, sealed.producer_attempt_id);
    assert_eq!(bundle.content_digest, sealed.content_digest);
    assert_eq!(bundle.data_class, sealed.data_class);
    assert_eq!(bundle.trust_label, ProvenanceTrustLabel::Accepted);
}

#[tokio::test]
async fn provenance_missing_id_is_not_found() {
    let (_dir, store) = create_test_store().await;
    let err = store
        .reconstruct_provenance(&ArtifactId::new("art_missing"))
        .await
        .unwrap_err();
    assert!(matches!(err, ArtifactError::NotFound(_)));
}

#[tokio::test]
async fn remote_quarantined_labeled_unverified_until_accepted() {
    use tetonic_domain::artifact::{provenance_trust_label, ProvenanceTrustLabel};
    use tetonic_domain::ArtifactOrigin;
    let (_dir, store) = create_test_store().await;
    let meta = ArtifactMetadata {
        artifact_id: ArtifactId::new("art_remote_q"),
        kind: ArtifactKind::Patch,
        schema_version: 1,
        producer_run_id: RunId::new("run_r"),
        producer_task_id: TaskId::new("task_r"),
        producer_attempt_id: AttemptId::new("att_r"),
        worker_id: None,
        content_digest: ContentDigest::new(hex::encode(Sha256::digest(b"remote-bytes"))),
        size_bytes: 0,
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        storage_location: ArtifactLocation::Remote("art_remote_q".into()),
        lifecycle_state: ArtifactState::Sealed,
        verification_state: VerificationState::LocallyVerified,
        origin: ArtifactOrigin::Remote,
        created_at: Utc::now(),
        retention_policy: RetentionPolicy::SecurityAudit,
    };
    let persisted = store
        .persist_quarantined_remote(meta, b"remote-bytes")
        .await
        .unwrap();
    assert_eq!(persisted.origin, ArtifactOrigin::Remote);
    assert_eq!(persisted.lifecycle_state, ArtifactState::Sealed);
    assert_eq!(persisted.verification_state, VerificationState::Unverified);
    assert_eq!(
        provenance_trust_label(&persisted),
        ProvenanceTrustLabel::RemoteUnverified
    );

    let accepted = store.mark_accepted(&persisted.artifact_id).await.unwrap();
    assert_eq!(
        provenance_trust_label(&accepted),
        ProvenanceTrustLabel::Accepted
    );
    assert_eq!(accepted.verification_state, VerificationState::Unverified);
}

#[tokio::test]
async fn project_history_survives_gc() {
    let (_dir, store) = create_test_store().await;
    let mut w = store.begin_write(test_declaration()).await.unwrap();
    w.write_chunk(b"keep-me").await.unwrap();
    let meta = w.seal().await.unwrap();
    let report = crate::gc::enforce_at_startup(&store, store.quota()).unwrap();
    assert!(report.deleted.is_empty());
    assert_eq!(store.list_metadata().unwrap().len(), 1);
    assert_eq!(
        store.metadata(&meta.artifact_id).await.unwrap().artifact_id,
        meta.artifact_id
    );
}

async fn seal_bytes(
    store: &LocalArtifactStore,
    declaration: ArtifactDeclaration,
    bytes: &[u8],
) -> ArtifactMetadata {
    let mut writer = store.begin_write(declaration).await.unwrap();
    writer.write_chunk(bytes).await.unwrap();
    writer.seal().await.unwrap()
}

#[tokio::test]
async fn identical_bytes_keep_independent_ownership_retention_and_acceptance() {
    let (_dir, store) = create_test_store().await;
    let mut pinned = test_declaration();
    pinned.retention_policy = RetentionPolicy::UserPinned;
    let first = seal_bytes(&store, pinned, b"same content").await;
    let accepted = store.mark_accepted(&first.artifact_id).await.unwrap();
    let mut transient = test_declaration();
    transient.producer_run_id = RunId::new("run_2");
    transient.producer_attempt_id = AttemptId::new("attempt_2");
    transient.retention_policy = RetentionPolicy::UntilRunCompletes;
    transient.data_class = DataClass::RepositorySource;
    let second = seal_bytes(&store, transient, b"same content").await;
    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(first.content_digest, second.content_digest);
    assert_eq!(second.lifecycle_state, ArtifactState::Sealed);
    let still_first = store.metadata(&first.artifact_id).await.unwrap();
    assert_eq!(still_first.producer_run_id, first.producer_run_id);
    assert_eq!(still_first.producer_attempt_id, first.producer_attempt_id);
    assert_eq!(still_first.data_class, first.data_class);
    assert_eq!(still_first.retention_policy, RetentionPolicy::UserPinned);
    assert_eq!(still_first.lifecycle_state, accepted.lifecycle_state);
    let roots = crate::gc::ArtifactGcRoots {
        completed_run_ids: ["run_1".into(), "run_2".into()].into(),
        ..Default::default()
    };
    let report = crate::gc::collect_garbage(&store, &roots, store.quota()).unwrap();
    assert_eq!(report.deleted, vec![second.artifact_id.0]);
    let mut reader = store.open(&first.artifact_id).await.unwrap();
    let mut buf = [0; 32];
    let n = reader.read_chunk(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"same content");
}

#[tokio::test]
async fn gc_preserves_referenced_and_unknown_runs() {
    let (_dir, store) = create_test_store().await;
    let mut declaration = test_declaration();
    declaration.retention_policy = RetentionPolicy::UntilRunCompletes;
    let referenced = seal_bytes(&store, declaration.clone(), b"referenced").await;
    let reclaimable = seal_bytes(&store, declaration.clone(), b"completed").await;
    declaration.producer_run_id = RunId::new("recovery_required");
    let recovery = seal_bytes(&store, declaration, b"recovery").await;
    let roots = crate::gc::ArtifactGcRoots {
        completed_run_ids: ["run_1".into()].into(),
        active_artifact_ids: [referenced.artifact_id.0.clone()].into(),
    };
    let report = crate::gc::collect_garbage(&store, &roots, store.quota()).unwrap();
    assert_eq!(report.deleted, vec![reclaimable.artifact_id.0]);
    assert!(store.open(&referenced.artifact_id).await.is_ok());
    assert!(store.open(&recovery.artifact_id).await.is_ok());
}

#[tokio::test]
async fn gc_in_another_process_preserves_live_writer() {
    let (dir, store) = create_test_store().await;
    let mut declaration = test_declaration();
    declaration.retention_policy = RetentionPolicy::UntilRunCompletes;
    let sealed = seal_bytes(&store, declaration, b"recovery data").await;
    let mut writer = store.begin_write(test_declaration()).await.unwrap();
    writer.write_chunk(b"before").await.unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "store::tests::gc_child_process", "--nocapture"])
        .env("LOKAI_ARTIFACT_GC_TEST_ROOT", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(store.open(&sealed.artifact_id).await.is_ok());
    writer.write_chunk(b"after").await.unwrap();
    let meta = writer.seal().await.unwrap();
    assert_eq!(
        meta.content_digest,
        ContentDigest::new(hex::encode(Sha256::digest(b"beforeafter")))
    );
    assert!(store.open(&meta.artifact_id).await.is_ok());
}

#[tokio::test]
async fn gc_child_process() {
    if let Some(root) = std::env::var_os("LOKAI_ARTIFACT_CRASH_TEST_ROOT") {
        let store = LocalArtifactStore::new(root, clean_scan()).unwrap();
        let mut writer = store.begin_write(test_declaration()).await.unwrap();
        writer.write_chunk(b"interrupted").await.unwrap();
        // Simulate process loss: no writer destructor is run.
        std::process::exit(0);
    }
    let Some(root) = std::env::var_os("LOKAI_ARTIFACT_GC_TEST_ROOT") else {
        return;
    };
    let store = LocalArtifactStore::new(root, clean_scan()).unwrap();
    let report = crate::gc::enforce_at_startup(&store, store.quota()).unwrap();
    assert_eq!(report.cleaned_tmp, 0);
    assert!(report.deleted.is_empty());
    assert_eq!(store.list_metadata().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_dir(store.root_dir.join("tmp"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn concurrent_seals_respect_quota_across_store_handles() {
    let (dir, first) = create_test_store().await;
    let quota = crate::gc::ArtifactGcConfig { max_total_bytes: 8 };
    let first = first.with_quota(quota.clone());
    let second = LocalArtifactStore::new(dir.path(), clean_scan())
        .unwrap()
        .with_quota(quota);
    let mut one = first.begin_write(test_declaration()).await.unwrap();
    let mut two = second.begin_write(test_declaration()).await.unwrap();
    one.write_chunk(b"12345678").await.unwrap();
    two.write_chunk(b"abcdefgh").await.unwrap();
    let (one, two) = tokio::join!(one.seal(), two.seal());
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let error = one.err().or(two.err()).unwrap();
    assert!(
        matches!(error, ArtifactError::QuotaExceeded { .. }),
        "{error}"
    );
    assert_eq!(first.usage_bytes().unwrap(), 8);
}

#[tokio::test]
async fn remote_import_cannot_overwrite_or_claim_acceptance() {
    let (_dir, store) = create_test_store().await;
    let original = seal_bytes(&store, test_declaration(), b"trusted").await;
    let accepted = store.mark_accepted(&original.artifact_id).await.unwrap();
    assert!(matches!(
        store
            .persist_quarantined_remote(accepted.clone(), b"trusted")
            .await,
        Err(ArtifactError::InvalidStateTransition(_))
    ));
    assert_eq!(
        store
            .metadata(&original.artifact_id)
            .await
            .unwrap()
            .lifecycle_state,
        ArtifactState::Accepted
    );
    let mut remote = accepted;
    remote.artifact_id = ArtifactId::new("remote_new");
    let imported = store
        .persist_quarantined_remote(remote, b"trusted")
        .await
        .unwrap();
    assert_eq!(imported.lifecycle_state, ArtifactState::Sealed);
    assert_eq!(imported.verification_state, VerificationState::Unverified);
    assert_eq!(imported.origin, tetonic_domain::ArtifactOrigin::Remote);
}

#[tokio::test]
async fn remote_import_checks_digest_path_and_quota_before_publication() {
    let (_dir, store) = create_test_store().await;
    let original = seal_bytes(&store, test_declaration(), b"12345678").await;
    let mut remote = original.clone();
    remote.artifact_id = ArtifactId::new("remote_invalid");
    assert!(matches!(
        store
            .persist_quarantined_remote(remote.clone(), b"wrong")
            .await,
        Err(ArtifactError::DigestMismatch { .. })
    ));
    remote.artifact_id = ArtifactId::new("../outside");
    assert!(store
        .persist_quarantined_remote(remote.clone(), b"12345678")
        .await
        .is_err());
    assert!(store.delete(&remote.artifact_id).await.is_err());
    remote.artifact_id = ArtifactId::new("remote_over_quota");
    let store = store.with_quota(crate::gc::ArtifactGcConfig { max_total_bytes: 8 });
    assert!(matches!(
        store.persist_quarantined_remote(remote, b"12345678").await,
        Err(ArtifactError::QuotaExceeded { .. })
    ));
    assert_eq!(store.list_metadata().unwrap().len(), 1);
    assert_eq!(store.usage_bytes().unwrap(), 8);
}

#[tokio::test]
async fn legacy_digest_ids_remain_readable() {
    let (_dir, store) = create_test_store().await;
    let original = seal_bytes(&store, test_declaration(), b"legacy").await;
    let mut legacy = original.clone();
    legacy.artifact_id = ArtifactId::new(format!("art_{}", legacy.content_digest.0));
    let path = store.object_path(&legacy.artifact_id);
    std::fs::rename(store.object_path(&original.artifact_id), &path).unwrap();
    legacy.storage_location = ArtifactLocation::LocalFile(path);
    store.write_metadata(&legacy).unwrap();
    std::fs::remove_file(store.meta_path(&original.artifact_id)).unwrap();
    assert!(store.open(&legacy.artifact_id).await.is_ok());
    assert!(crate::gc::enforce_at_startup(&store, store.quota())
        .unwrap()
        .deleted
        .is_empty());
}

#[tokio::test]
async fn interrupted_publication_still_counts_towards_quota() {
    let (_dir, store) = create_test_store().await;
    let store = store.with_quota(crate::gc::ArtifactGcConfig { max_total_bytes: 8 });
    std::fs::write(
        store.object_path(&ArtifactId::new("orphan_object")),
        b"12345678",
    )
    .unwrap();
    let mut writer = store.begin_write(test_declaration()).await.unwrap();
    writer.write_chunk(b"x").await.unwrap();
    assert!(matches!(
        writer.seal().await,
        Err(ArtifactError::QuotaExceeded { .. })
    ));
}
#[tokio::test]
async fn startup_reclaims_write_after_process_exit() {
    let (dir, store) = create_test_store().await;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "store::tests::gc_child_process", "--nocapture"])
        .env("LOKAI_ARTIFACT_CRASH_TEST_ROOT", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_dir(store.root_dir.join("tmp"))
            .unwrap()
            .count(),
        1
    );
    let report = crate::gc::enforce_at_startup(&store, store.quota()).unwrap();
    assert_eq!(report.cleaned_tmp, 1);
    assert_eq!(
        std::fs::read_dir(store.root_dir.join("tmp"))
            .unwrap()
            .count(),
        0
    );
}
