//! Local operator composition without inference, sessions or a coding runtime.
use super::*;
use std::path::PathBuf;

pub struct LocalControl {
    store: SharedStore,
    credentials: Arc<LocalCredentials>,
}

impl LocalControl {
    /// Local operator door: filesystem access to this database is administrative
    /// authority. This is not a remote employee client or multiuser transport.
    pub async fn open(path: PathBuf, audience: String) -> Result<Self, ResourceError> {
        if path.as_os_str().is_empty()
            || path == std::path::Path::new(":memory:")
            || path.to_str().is_some_and(|p| p.starts_with("file:"))
        {
            return Err(ResourceError::Invalid);
        }
        if audience.trim().is_empty() || audience.len() > 256 || audience.contains('\0') {
            return Err(ResourceError::Invalid);
        }
        let store = tokio::task::spawn_blocking(move || SharedStore::open(path, 2))
            .await
            .map_err(|_| ResourceError::Storage)??;
        let credentials = Arc::new(LocalCredentials {
            store: store.clone(),
            audience,
        });
        Ok(Self { store, credentials })
    }

    pub async fn bootstrap(
        &self,
        principal: String,
        org: String,
        name: String,
    ) -> Result<(), ResourceError> {
        self.store
            .write(move |db| db.bootstrap_control(&principal, &org, &name))
            .await??;
        Ok(())
    }

    pub fn credentials(&self) -> &Arc<LocalCredentials> {
        &self.credentials
    }

    pub fn contexts(&self) -> ContextService {
        ContextService {
            store: self.store.clone(),
            verifier: self.credentials.clone(),
        }
    }

    pub fn resources(&self) -> ResourceService {
        let authority = Arc::new(super::membership::MembershipAuthority {
            store: self.store.clone(),
            verifier: self.credentials.clone(),
        });
        ResourceService {
            store: self.store.clone(),
            authority,
        }
    }
}
