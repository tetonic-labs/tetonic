//! Local operator composition without inference, sessions or a coding runtime.
use super::*;
use std::path::PathBuf;

#[derive(Clone)]
pub struct LocalControl {
    store: SharedStore,
    credentials: Arc<LocalCredentials>,
}

impl LocalControl {
    /// Local operator door: filesystem access to this database is administrative
    /// authority. This is not a remote employee client or multiuser transport.
    pub async fn open(path: PathBuf, audience: String) -> Result<Self, ResourceError> {
        Self::open_with_read_connections(path, audience, 2).await
    }

    pub(crate) async fn open_with_read_connections(
        path: PathBuf,
        audience: String,
        read_connections: usize,
    ) -> Result<Self, ResourceError> {
        if path.as_os_str().is_empty()
            || path == std::path::Path::new(":memory:")
            || path.to_str().is_some_and(|p| p.starts_with("file:"))
        {
            return Err(ResourceError::Invalid);
        }
        if audience.trim().is_empty() || audience.len() > 256 || audience.contains('\0') {
            return Err(ResourceError::Invalid);
        }
        if !(1..=16).contains(&read_connections) {
            return Err(ResourceError::Invalid);
        }
        let store = tokio::task::spawn_blocking(move || SharedStore::open(path, read_connections))
            .await
            .map_err(|_| ResourceError::Storage)??;
        let credentials = Arc::new(LocalCredentials {
            store: store.clone(),
            audience,
        });
        Ok(Self { store, credentials })
    }

    pub(crate) fn store(&self) -> &SharedStore {
        &self.store
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
