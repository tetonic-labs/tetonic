//! Explicit, bounded inspection of stopped output through existing context grants.
use super::*;
use crate::execution::drafts::{SavedDraft, MAX_DRAFT_BYTES};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadWorkDraft {
    pub artifact_id: String,
    pub file: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}
#[derive(Serialize)]
pub struct WorkDraftView {
    pub artifact_id: String,
    pub files: Vec<WorkDraftFile>,
    pub page: Option<DraftTextPage>,
}
#[derive(Serialize)]
pub struct WorkDraftFile {
    pub path: String,
    pub operation: tetonic_domain::StagedOperationKind,
    pub byte_count: usize,
    pub text_available: bool,
}
#[derive(Serialize)]
pub struct DraftTextPage {
    pub file: usize,
    pub content: String,
    pub offset: usize,
    pub next_offset: Option<usize>,
}
fn unavailable() -> AppError {
    AppError::InvalidRequest("This saved draft is unavailable. No files were changed. Inspect the original work before trying again.".into())
}

impl WorkService {
    pub async fn read_work_draft(
        &self,
        work: &str,
        request: ReadWorkDraft,
    ) -> Result<WorkDraftView, AppError> {
        if request.artifact_id.is_empty()
            || request.artifact_id.len() > 200
            || !request
                .artifact_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(unavailable());
        }
        let task = Box::pin(self.task(work)).await?;
        let receipt = task
            .saved_drafts
            .iter()
            .find(|r| r.artifact_id == request.artifact_id)
            .ok_or_else(unavailable)?;
        let scope = self.services.authorized_scope().await?;
        let context = task
            .plan
            .as_ref()
            .map_or(scope.context(), |p| &p.information_context_id)
            .to_owned();
        let artifacts = self
            .services
            .local
            .contexts()
            .bind_artifacts(
                &self.services.host.credential,
                context,
                self.services.host.app.host.runtime.artifact_store().clone(),
            )
            .await
            .map_err(resource)?;
        let id = tetonic_domain::ArtifactId::new(request.artifact_id.clone());
        let meta = artifacts.metadata(&id).await.map_err(|_| unavailable())?;
        if meta.producer_run_id.0 != receipt.run_id
            || meta.producer_task_id.0 != receipt.task_id
            || meta.producer_attempt_id.0 != receipt.attempt_id
            || meta.content_digest.0 != receipt.digest
            || meta.kind != tetonic_domain::artifact::ArtifactKind::FileSnapshot
            || meta.lifecycle_state == tetonic_domain::artifact::ArtifactState::Accepted
            || meta.size_bytes > MAX_DRAFT_BYTES as u64
        {
            return Err(unavailable());
        }
        let mut reader = artifacts.open(&id).await.map_err(|_| unavailable())?;
        let mut bytes = Vec::new();
        let mut buffer = vec![0; 65536];
        loop {
            let count = reader
                .read_chunk(&mut buffer)
                .await
                .map_err(|_| unavailable())?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > MAX_DRAFT_BYTES {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        if bytes.len() as u64 != meta.size_bytes {
            return Err(unavailable());
        }
        let receipt = receipt.clone();
        let view = tokio::task::spawn_blocking(move || project_draft(bytes, receipt, request))
            .await
            .map_err(|_| unavailable())??;
        // Recheck access after the content read, including work membership.
        Box::pin(self.task(work)).await?;
        if artifacts.metadata(&id).await.map_err(|_| unavailable())? != meta {
            return Err(unavailable());
        }
        Ok(view)
    }
}

fn project_draft(
    bytes: Vec<u8>,
    receipt: tetonic_memory::WorkspaceDraftReceipt,
    request: ReadWorkDraft,
) -> Result<WorkDraftView, AppError> {
    if format!("{:x}", Sha256::digest(&bytes)) != receipt.digest {
        return Err(unavailable());
    }
    let value: SavedDraft = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if value.schema_version != 1 || value.files.len() != receipt.file_count {
        return Err(unavailable());
    }
    let page = if let Some(index) = request.file {
        let file = value.files.get(index).ok_or_else(unavailable)?;
        let content = file.content.as_deref().ok_or_else(unavailable)?;
        if request.offset > content.len() || !content.is_char_boundary(request.offset) {
            return Err(unavailable());
        }
        let mut end = (request.offset + 16384).min(content.len());
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        Some(DraftTextPage {
            file: index,
            content: content[request.offset..end].into(),
            offset: request.offset,
            next_offset: (end < content.len()).then_some(end),
        })
    } else {
        None
    };
    Ok(WorkDraftView {
        artifact_id: request.artifact_id,
        page,
        files: value
            .files
            .into_iter()
            .map(|f| WorkDraftFile {
                path: f.path,
                operation: f.operation,
                byte_count: f.byte_count,
                text_available: f.content.is_some(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_unicode_files_page_without_loss_and_refuse_invalid_offsets() {
        let content = format!("{}🙂tail", "a".repeat(16383));
        let bytes = serde_json::to_vec(&SavedDraft {
            schema_version: 1,
            files: vec![crate::execution::drafts::SavedDraftFile {
                path: "file.txt".into(),
                operation: tetonic_domain::StagedOperationKind::Create,
                byte_count: content.len(),
                content: Some(content.clone()),
            }],
        })
        .unwrap();
        let receipt = tetonic_memory::WorkspaceDraftReceipt {
            artifact_id: "draft".into(),
            digest: format!("{:x}", Sha256::digest(&bytes)),
            run_id: "r".into(),
            task_id: "t".into(),
            attempt_id: "a".into(),
            file_count: 1,
        };
        let read = |offset| {
            project_draft(
                bytes.clone(),
                receipt.clone(),
                ReadWorkDraft {
                    artifact_id: "draft".into(),
                    file: Some(0),
                    offset,
                },
            )
        };
        let first = read(0).unwrap().page.unwrap();
        let second = read(first.next_offset.unwrap()).unwrap().page.unwrap();
        assert_eq!(format!("{}{}", first.content, second.content), content);
        assert!(second.next_offset.is_none());
        assert!(read(16384).is_err());
        assert!(read(usize::MAX).is_err());
    }
}
