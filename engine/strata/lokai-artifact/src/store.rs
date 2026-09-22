use async_trait::async_trait;
use chrono::Utc;
use lokai_domain::artifact::{
    ArtifactDeclaration, ArtifactError, ArtifactLocation, ArtifactMetadata, ArtifactReader,
    ArtifactState, ArtifactStore, ArtifactWriter, VerificationState,
};
use lokai_domain::ids::ArtifactId;
use lokai_domain::workspace::ContentDigest;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const MAX_ARTIFACT_SIZE_BYTES: u64 = 100 * 1024 * 1024; // 100MB limit

/// Bytes of already-scanned text retained across a chunk boundary.
///
/// This is a **stated bound on what this store can detect**, not a tuning knob:
/// the scanner sees a sliding window, so a secret longer than this can straddle
/// two windows and be missed by both. Raising it widens detection and costs
/// memory per in-flight writer.
const SCAN_OVERLAP_BYTES: usize = 4096;

pub type ArtifactTextScanner = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Whether artifacts are scanned for secrets before they seal
/// (M6, INV-GATE-001, INV-ART-001).
///
/// There is deliberately **no variant that seals without scanning**. Before M6
/// the field was `Option<ArtifactTextScanner>` and `new` defaulted it to `None`,
/// so an unscanned seal was what you got by forgetting — production remembered
/// at one call site and the eval binaries did not. Making the policy a required
/// argument moves that from a runtime property to a compile error.
#[derive(Clone)]
pub enum ScanPolicy {
    /// Scan every artifact with this hook as it is written.
    Scan(ArtifactTextScanner),
    /// No scanner is available, so `seal` refuses.
    ///
    /// Correct for a store that never seals. A store that does seal and is built
    /// with this will fail closed rather than quietly accept unscanned content.
    Refuse,
}

pub struct LocalArtifactStore {
    root_dir: PathBuf,
    scan: ScanPolicy,
    quota: crate::gc::ArtifactGcConfig,
}

impl LocalArtifactStore {
    pub fn new(root_dir: impl AsRef<Path>, scan: ScanPolicy) -> std::io::Result<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&root_dir)?;
        std::fs::create_dir_all(root_dir.join("tmp"))?;
        std::fs::create_dir_all(root_dir.join("objects"))?;
        std::fs::create_dir_all(root_dir.join("meta"))?;
        std::fs::create_dir_all(root_dir.join("deletions"))?;
        Ok(Self {
            root_dir,
            scan,
            quota: crate::gc::ArtifactGcConfig::from_env(),
        })
    }

    /// Convenience for the `ScanPolicy::Scan` case. It no longer decides
    /// *whether* scanning happens — `new` already required that answer.
    pub fn with_scanner(mut self, scanner: ArtifactTextScanner) -> Self {
        self.scan = ScanPolicy::Scan(scanner);
        self
    }

    pub fn with_quota(mut self, quota: crate::gc::ArtifactGcConfig) -> Self {
        self.quota = quota;
        self
    }

    pub fn quota(&self) -> &crate::gc::ArtifactGcConfig {
        &self.quota
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub(crate) fn mutation_lock(&self) -> Result<std::fs::File, ArtifactError> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root_dir.join("store.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file)?;
        Ok(file)
    }

    async fn mutation_lock_async(&self) -> Result<std::fs::File, ArtifactError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.mutation_lock() {
                Ok(lock) => return Ok(lock),
                Err(ArtifactError::Io(e))
                    if e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn validate_id(id: &ArtifactId) -> Result<(), ArtifactError> {
        if id.0.is_empty()
            || id.0.len() > 200
            || !id
                .0
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ArtifactError::Internal(
                "invalid artifact identifier".into(),
            ));
        }
        Ok(())
    }

    fn tmp_path(&self, id: &str) -> PathBuf {
        self.root_dir.join("tmp").join(id)
    }

    fn object_path(&self, id: &ArtifactId) -> PathBuf {
        self.root_dir.join("objects").join(id.to_string())
    }

    fn meta_path(&self, id: &ArtifactId) -> PathBuf {
        self.root_dir.join("meta").join(format!("{}.json", id))
    }

    fn write_metadata(&self, meta: &ArtifactMetadata) -> Result<(), ArtifactError> {
        let meta_json =
            serde_json::to_vec_pretty(meta).map_err(|e| ArtifactError::Internal(e.to_string()))?;
        let meta_path = self.meta_path(&meta.artifact_id);
        let meta_tmp = self.tmp_path(&format!(
            "{}_{}_meta",
            meta.artifact_id.0,
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&meta_tmp, &meta_json)?;
        crate::publication::publish(&self.root_dir, &meta_tmp, &meta_path)?;
        Ok(())
    }

    /// Reconstruct thin provenance for a sealed/accepted artifact (R24).
    pub async fn reconstruct_provenance(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<lokai_domain::artifact::ArtifactProvenanceBundle, ArtifactError> {
        let meta = self.metadata(artifact_id).await?;
        Ok(lokai_domain::artifact::ArtifactProvenanceBundle::from_metadata(&meta))
    }

    /// Persist remote quarantined bytes + meta; trust stays unverified until Accepted (R24).
    pub async fn persist_quarantined_remote(
        &self,
        mut meta: ArtifactMetadata,
        bytes: &[u8],
    ) -> Result<ArtifactMetadata, ArtifactError> {
        Self::validate_id(&meta.artifact_id)?;
        let _lock = self.mutation_lock_async().await?;
        if self.meta_path(&meta.artifact_id).exists()
            || self.object_path(&meta.artifact_id).exists()
            || self.deletion_path(&meta.artifact_id).exists()
        {
            return Err(ArtifactError::InvalidStateTransition(
                "artifact occurrence already exists".into(),
            ));
        }
        if bytes.len() as u64 > MAX_ARTIFACT_SIZE_BYTES {
            return Err(ArtifactError::SizeLimitExceeded(MAX_ARTIFACT_SIZE_BYTES));
        }
        let actual = ContentDigest::new(hex::encode(Sha256::digest(bytes)));
        if actual != meta.content_digest {
            return Err(ArtifactError::DigestMismatch {
                expected: meta.content_digest,
                actual,
            });
        }
        crate::gc::ensure_quota_for_write(
            self,
            &Default::default(),
            &self.quota,
            bytes.len() as u64,
        )?;
        meta.origin = lokai_domain::ArtifactOrigin::Remote;
        meta.lifecycle_state = ArtifactState::Sealed;
        meta.verification_state = VerificationState::Unverified;
        let obj_path = self.object_path(&meta.artifact_id);
        let obj_tmp = self.tmp_path(&format!(
            "{}_{}_obj",
            meta.artifact_id.0,
            uuid::Uuid::new_v4()
        ));
        // The blocking operation owns the lock: cancellation cannot release it
        // while an OS write/rename is still in flight.
        let store = self.clone();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            let _lock = _lock;
            std::fs::write(&obj_tmp, &bytes)?;
            crate::publication::publish(&store.root_dir, &obj_tmp, &obj_path)?;
            meta.storage_location = ArtifactLocation::LocalFile(obj_path);
            meta.size_bytes = bytes.len() as u64;
            store.write_metadata(&meta)?;
            Ok(meta)
        })
        .await
        .map_err(|e| ArtifactError::Internal(e.to_string()))?
    }

    /// List sealed artifact metadata (sync; used by startup GC).
    pub fn list_metadata(&self) -> Result<Vec<ArtifactMetadata>, ArtifactError> {
        let dir = self.root_dir.join("meta");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(ArtifactError::Io)? {
            let entry = entry.map_err(ArtifactError::Io)?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read(&path).map_err(ArtifactError::Io)?;
            let meta: ArtifactMetadata = serde_json::from_slice(&content)
                .map_err(|e| ArtifactError::Internal(e.to_string()))?;
            out.push(meta);
        }
        Ok(out)
    }

    /// Physical object bytes, including interrupted publications without metadata.
    pub fn usage_bytes(&self) -> Result<u64, ArtifactError> {
        let mut total = 0u64;
        for entry in std::fs::read_dir(self.root_dir.join("objects"))? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                total = total.saturating_add(entry.metadata()?.len());
            }
        }
        Ok(total)
    }

    /// Remove abandoned write temp files (interrupted seals).
    pub fn clean_abandoned_tmp(&self) -> Result<usize, ArtifactError> {
        let _lock = self.mutation_lock()?;
        self.clean_abandoned_tmp_locked()
    }

    pub(crate) fn clean_abandoned_tmp_locked(&self) -> Result<usize, ArtifactError> {
        let dir = self.root_dir.join("tmp");
        if !dir.exists() {
            return Ok(0);
        }
        let mut n = 0usize;
        for entry in std::fs::read_dir(&dir).map_err(ArtifactError::Io)? {
            let entry = entry.map_err(ArtifactError::Io)?;
            let path = entry.path();
            if path.is_file() {
                let lease = match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                {
                    Ok(lease) => lease,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                match fs2::FileExt::try_lock_exclusive(&lease) {
                    Ok(()) => match std::fs::remove_file(&path) {
                        Ok(()) => n += 1,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    },
                    Err(e) if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                        continue
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(n)
    }

    pub fn delete_sync(&self, artifact_id: &ArtifactId) -> Result<(), ArtifactError> {
        let _lock = self.mutation_lock()?;
        self.delete_sync_locked(artifact_id)
    }

    pub(crate) fn delete_sync_locked(&self, artifact_id: &ArtifactId) -> Result<(), ArtifactError> {
        Self::validate_id(artifact_id)?;
        let marker = self.deletion_path(artifact_id);
        let temporary = self.tmp_path(&format!("delete_{}", uuid::Uuid::new_v4()));
        // The marker contains no caller-selected filesystem paths. Once durable,
        // it authorizes recovery of this already-requested deletion only.
        std::fs::write(&temporary, b"lokai-artifact-delete-v1\n")?;
        crate::publication::publish(&self.root_dir, &temporary, &marker)?;
        self.finish_deletion_locked(artifact_id)
    }

    fn deletion_path(&self, artifact_id: &ArtifactId) -> PathBuf {
        self.root_dir
            .join("deletions")
            .join(format!("{}.delete", artifact_id.0))
    }

    fn finish_deletion_locked(&self, artifact_id: &ArtifactId) -> Result<(), ArtifactError> {
        let obj_path = self.object_path(artifact_id);
        let meta_path = self.meta_path(artifact_id);
        crate::publication::delete(&self.root_dir, &meta_path, &obj_path)?;
        std::fs::remove_file(self.deletion_path(artifact_id))?;
        crate::publication::sync_directory(&self.root_dir, &self.root_dir.join("deletions"))?;
        Ok(())
    }

    pub(crate) fn recover_deletions_locked(&self) -> Result<Vec<String>, ArtifactError> {
        let mut recovered = Vec::new();
        for entry in std::fs::read_dir(self.root_dir.join("deletions"))? {
            let entry = entry?;
            let name = entry.file_name();
            let id = name
                .to_str()
                .and_then(|name| name.strip_suffix(".delete"))
                .ok_or_else(|| {
                    ArtifactError::Internal("invalid artifact deletion record".into())
                })?;
            let id = ArtifactId::new(id);
            Self::validate_id(&id)?;
            if !entry.file_type()?.is_file()
                || entry.metadata()?.len() != b"lokai-artifact-delete-v1\n".len() as u64
                || std::fs::read(entry.path())? != b"lokai-artifact-delete-v1\n"
            {
                return Err(ArtifactError::Internal(
                    "invalid artifact deletion record".into(),
                ));
            }
            self.finish_deletion_locked(&id)?;
            recovered.push(id.0);
        }
        Ok(recovered)
    }
}

#[async_trait]
impl ArtifactStore for LocalArtifactStore {
    async fn begin_write(
        &self,
        declaration: ArtifactDeclaration,
    ) -> Result<Box<dyn ArtifactWriter>, ArtifactError> {
        let _lock = self.mutation_lock_async().await?;
        let temp_id = uuid::Uuid::new_v4().to_string();
        let tmp_path = self.tmp_path(&temp_id);
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        fs2::FileExt::try_lock_exclusive(&lease)?;
        let file = File::from_std(lease.try_clone()?);

        Ok(Box::new(LocalArtifactWriter {
            store: Arc::new(self.clone()),
            tmp_path,
            file: Some(file),
            writer_lease: Some(lease),
            hasher: Sha256::new(),
            written_bytes: 0,
            declaration,
            scan_carry: String::new(),
            scan_hit: false,
        }))
    }

    /// Open a sealed artifact, verifying its content against the seal digest
    /// first (M6, INV-ART-001: "`open` requires seal digest").
    ///
    /// Verification is **eager**, not streamed as the caller reads. Streaming
    /// would cost nothing up front and would let a caller consume most of a
    /// corrupt artifact before the final read failed, which satisfies "the read
    /// eventually errors" rather than "open requires the digest". The price is a
    /// full hash per open, bounded by `MAX_ARTIFACT_SIZE_BYTES`; the structural
    /// path alone cannot guarantee integrity even in content-addressed storage.
    async fn open(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Box<dyn ArtifactReader>, ArtifactError> {
        let _lock = self.mutation_lock_async().await?;
        Self::validate_id(artifact_id)?;
        if self.deletion_path(artifact_id).exists() {
            return Err(ArtifactError::InvalidStateTransition(
                "artifact deletion pending".into(),
            ));
        }
        let meta = self.metadata(artifact_id).await?;
        let path = self.object_path(artifact_id);

        let mut file = File::open(&path)
            .await
            .map_err(|_| ArtifactError::NotFound(artifact_id.clone()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf).await.map_err(ArtifactError::Io)?;
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

        // Return the verified handle rather than reopening potentially different bytes.
        file.seek(std::io::SeekFrom::Start(0)).await?;
        Ok(Box::new(LocalArtifactReader { file }))
    }

    async fn metadata(&self, artifact_id: &ArtifactId) -> Result<ArtifactMetadata, ArtifactError> {
        Self::validate_id(artifact_id)?;
        let path = self.meta_path(artifact_id);
        let content = fs::read(&path)
            .await
            .map_err(|_| ArtifactError::NotFound(artifact_id.clone()))?;
        let meta: ArtifactMetadata =
            serde_json::from_slice(&content).map_err(|e| ArtifactError::Internal(e.to_string()))?;
        Ok(meta)
    }

    async fn mark_accepted(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        let _lock = self.mutation_lock_async().await?;
        Self::validate_id(artifact_id)?;
        if self.deletion_path(artifact_id).exists() {
            return Err(ArtifactError::InvalidStateTransition(
                "artifact deletion pending".into(),
            ));
        }
        let mut meta = self.metadata(artifact_id).await?;
        match meta.lifecycle_state {
            ArtifactState::Sealed | ArtifactState::Verified | ArtifactState::Accepted => {}
            other => {
                return Err(ArtifactError::InvalidStateTransition(format!(
                    "cannot mark_accepted from {other:?}"
                )));
            }
        }
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let _lock = _lock;
            // Verify and sync the actual object before durable acceptance, including
            // artifacts created by older versions or recovered after interruption.
            use std::io::Read;
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(store.object_path(&meta.artifact_id))?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf)?;
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
            file.sync_all()?;
            crate::publication::sync_directory(&store.root_dir, &store.root_dir.join("objects"))?;
            meta.lifecycle_state = ArtifactState::Accepted;
            if matches!(
                meta.verification_state,
                VerificationState::Unverified | VerificationState::StructurallyValid
            ) && matches!(meta.origin, lokai_domain::ArtifactOrigin::Local)
            {
                meta.verification_state = VerificationState::LocallyVerified;
            }
            store.write_metadata(&meta)?;
            Ok(meta)
        })
        .await
        .map_err(|e| ArtifactError::Internal(e.to_string()))?
    }

    async fn delete(&self, artifact_id: &ArtifactId) -> Result<(), ArtifactError> {
        let _lock = self.mutation_lock_async().await?;
        self.delete_sync_locked(artifact_id)
    }
}

impl Clone for LocalArtifactStore {
    fn clone(&self) -> Self {
        Self {
            root_dir: self.root_dir.clone(),
            scan: self.scan.clone(),
            quota: self.quota.clone(),
        }
    }
}

pub struct LocalArtifactWriter {
    store: Arc<LocalArtifactStore>,
    tmp_path: PathBuf,
    file: Option<File>,
    writer_lease: Option<std::fs::File>,
    hasher: Sha256,
    written_bytes: u64,
    declaration: ArtifactDeclaration,
    /// Tail of the text scanned so far, kept so a secret split across two
    /// `write_chunk` calls is still seen (`SCAN_OVERLAP_BYTES`).
    scan_carry: String,
    scan_hit: bool,
}

impl LocalArtifactWriter {
    /// Scan this chunk together with the tail of the previous one.
    ///
    /// Lossy decoding is deliberate: a binary artifact carrying an embedded
    /// ASCII credential is still scanned, where the pre-M6 code skipped
    /// anything that was not valid UTF-8 outright.
    fn scan(&mut self, data: &[u8]) {
        let ScanPolicy::Scan(scanner) = &self.store.scan else {
            return;
        };
        if self.scan_hit {
            return;
        }
        let mut window = std::mem::take(&mut self.scan_carry);
        window.push_str(&String::from_utf8_lossy(data));
        if scanner(&window) {
            self.scan_hit = true;
            self.scan_carry.clear();
            return;
        }
        let mut start = window.len().saturating_sub(SCAN_OVERLAP_BYTES);
        while start < window.len() && !window.is_char_boundary(start) {
            start += 1;
        }
        self.scan_carry = window[start..].to_string();
    }
}

#[async_trait]
impl ArtifactWriter for LocalArtifactWriter {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), ArtifactError> {
        if self.file.is_none() {
            return Err(ArtifactError::InvalidStateTransition(
                "already sealed or abandoned".into(),
            ));
        }
        if self.written_bytes + data.len() as u64 > MAX_ARTIFACT_SIZE_BYTES {
            return Err(ArtifactError::SizeLimitExceeded(MAX_ARTIFACT_SIZE_BYTES));
        }

        self.hasher.update(data);
        self.written_bytes += data.len() as u64;
        self.scan(data);

        if let Some(f) = &mut self.file {
            f.write_all(data).await?;
        }
        Ok(())
    }

    async fn seal(mut self: Box<Self>) -> Result<ArtifactMetadata, ArtifactError> {
        if let Some(mut f) = self.file.take() {
            f.flush().await?;
            f.sync_all().await?;
        } else {
            return Err(ArtifactError::InvalidStateTransition(
                "already sealed or abandoned".into(),
            ));
        }

        let _lock = self.store.mutation_lock_async().await?;
        self.writer_lease.take();
        let digest_bytes = self.hasher.clone().finalize();
        let digest_str = hex::encode(digest_bytes);
        let digest = ContentDigest::new(digest_str);

        // Quota gate before promoting tmp → sealed object (R4-2).
        crate::gc::ensure_quota_for_write(
            &self.store,
            &std::collections::HashSet::new(),
            &self.store.quota,
            self.written_bytes,
        )?;

        // The scan already happened, incrementally, in `write_chunk` (M6). It
        // used to happen here by reading the whole temp file back, which is why
        // it was skipped above 1 MiB and skipped again for non-UTF8 input. Both
        // exemptions are gone: there is no size at which scanning stops, and
        // non-UTF8 content is decoded lossily rather than waved through.
        match &self.store.scan {
            ScanPolicy::Refuse => {
                let _ = std::fs::remove_file(&self.tmp_path);
                return Err(ArtifactError::Internal(
                    "artifact store was built without a secret scanner; refusing to seal".into(),
                ));
            }
            ScanPolicy::Scan(_) if self.scan_hit => {
                let _ = std::fs::remove_file(&self.tmp_path);
                return Err(ArtifactError::Internal(
                    "Artifact contains secrets and was rejected".into(),
                ));
            }
            ScanPolicy::Scan(_) => {}
        }

        // Content identity is the digest; ownership identity is a unique occurrence.
        // Equal bytes never replace another producer's metadata or retention.
        let artifact_id = ArtifactId::new(format!("art_{}", uuid::Uuid::new_v4()));

        let obj_path = self.store.object_path(&artifact_id);
        crate::publication::publish(&self.store.root_dir, &self.tmp_path, &obj_path)?;

        let meta = ArtifactMetadata {
            artifact_id: artifact_id.clone(),
            kind: self.declaration.kind.clone(),
            schema_version: 1,
            producer_run_id: self.declaration.producer_run_id.clone(),
            producer_task_id: self.declaration.producer_task_id.clone(),
            producer_attempt_id: self.declaration.producer_attempt_id.clone(),
            worker_id: self.declaration.worker_id.clone(),
            content_digest: digest,
            size_bytes: self.written_bytes,
            workspace_version: self.declaration.workspace_version.clone(),
            data_class: self.declaration.data_class,
            storage_location: ArtifactLocation::LocalFile(obj_path),
            lifecycle_state: ArtifactState::Sealed,
            verification_state: VerificationState::Unverified,
            origin: lokai_domain::ArtifactOrigin::Local,
            created_at: Utc::now(),
            retention_policy: self.declaration.retention_policy.clone(),
        };

        // Keep metadata publication synchronous under the lock so dropping an
        // async caller cannot leave an unprotected rename running in the pool.
        self.store.write_metadata(&meta)?;

        Ok(meta)
    }

    async fn abandon(mut self: Box<Self>) -> Result<(), ArtifactError> {
        self.file = None;
        self.writer_lease.take();
        let _ = std::fs::remove_file(&self.tmp_path);
        Ok(())
    }
}

impl Drop for LocalArtifactWriter {
    fn drop(&mut self) {
        self.file.take();
        self.writer_lease.take();
        // The name is unique to this writer; a completed seal has moved it away.
        let _ = std::fs::remove_file(&self.tmp_path);
    }
}

pub struct LocalArtifactReader {
    file: File,
}

#[async_trait]
impl ArtifactReader for LocalArtifactReader {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, ArtifactError> {
        let n = self.file.read(buf).await?;
        Ok(n)
    }
}

pub type ArtifactStoreImpl = LocalArtifactStore;

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
