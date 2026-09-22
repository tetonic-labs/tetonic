//! Verify sealed object bytes against metadata `content_digest`.

use lokai_domain::artifact::{ArtifactError, ArtifactStore};
use lokai_domain::ids::ArtifactId;
use lokai_domain::workspace::ContentDigest;
use sha2::{Digest, Sha256};

/// Hash stored bytes and reject if they do not match metadata.
pub async fn verify_stored_content(
    store: &dyn ArtifactStore,
    artifact_id: &ArtifactId,
) -> Result<ContentDigest, ArtifactError> {
    let meta = store.metadata(artifact_id).await?;
    let mut reader = store.open(artifact_id).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 8192];
    loop {
        let n = reader.read_chunk(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = ContentDigest::new(hex::encode(hasher.finalize()));
    if actual != meta.content_digest {
        return Err(ArtifactError::DigestMismatch {
            expected: meta.content_digest,
            actual,
        });
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalArtifactStore;
    use lokai_domain::artifact::{
        ArtifactDeclaration, ArtifactKind, ArtifactLocation, RetentionPolicy,
    };
    use lokai_domain::classify::DataClass;
    use lokai_domain::ids::{AttemptId, RunId, TaskId};

    fn declaration() -> ArtifactDeclaration {
        ArtifactDeclaration {
            kind: ArtifactKind::FileSnapshot,
            producer_run_id: RunId::new("run_1"),
            producer_task_id: TaskId::new("task_1"),
            producer_attempt_id: AttemptId::new("attempt_1"),
            worker_id: None,
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            retention_policy: RetentionPolicy::ProjectHistory,
        }
    }

    #[tokio::test]
    async fn sealed_bytes_match_metadata_digest() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalArtifactStore::new(
            dir.path(),
            crate::ScanPolicy::Scan(std::sync::Arc::new(|_| false)),
        )
        .unwrap();
        let mut writer = store.begin_write(declaration()).await.unwrap();
        writer.write_chunk(b"turn-output").await.unwrap();
        let meta = writer.seal().await.unwrap();
        let digest = verify_stored_content(&store, &meta.artifact_id)
            .await
            .expect("verify");
        assert_eq!(digest, meta.content_digest);
    }

    #[tokio::test]
    async fn tampered_bytes_fail_verify() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalArtifactStore::new(
            dir.path(),
            crate::ScanPolicy::Scan(std::sync::Arc::new(|_| false)),
        )
        .unwrap();
        let mut writer = store.begin_write(declaration()).await.unwrap();
        writer.write_chunk(b"original").await.unwrap();
        let meta = writer.seal().await.unwrap();
        match meta.storage_location {
            ArtifactLocation::LocalFile(path) => {
                std::fs::write(path, b"tampered").unwrap();
            }
            other => panic!("expected local file, got {other:?}"),
        }
        let err = verify_stored_content(&store, &meta.artifact_id)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ArtifactError::DigestMismatch { .. }),
            "{err:?}"
        );
    }
}
