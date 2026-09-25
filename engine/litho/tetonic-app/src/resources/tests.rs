use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

// Test-only grant authority. Production must supply verified credentials and
// durable membership/policy; these literal tokens are never a product adapter.
struct Authority {
    revoked: AtomicBool,
}

#[async_trait]
impl ResourceAuthority for Authority {
    async fn authorize(
        &self,
        credential: &str,
        action: &ResourceAction,
    ) -> Result<AuthorizedPrincipal, AccessError> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err(AccessError);
        }
        let allowed = match action {
            ResourceAction::ManageOrganization { .. } => false,
            ResourceAction::CreateOrganization { .. } => credential == "admin",
            ResourceAction::ReadOrganization { org_id } => credential == "admin" && org_id == "a",
            ResourceAction::CreateTeam { org_id, .. } => {
                ["alice", "bob"].contains(&credential) && org_id == "a"
            }
            ResourceAction::ReadTeam { org_id, team_id } => {
                credential == "alice" && org_id == "a" && team_id == "maintenance"
            }
        };
        if !allowed {
            return Err(AccessError);
        }
        AuthorizedPrincipal::new(credential.into())
    }
}

fn authority() -> Arc<Authority> {
    Arc::new(Authority {
        revoked: AtomicBool::new(false),
    })
}

fn app(workspace: &std::path::Path, store: Option<SharedStore>) -> Arc<crate::Application> {
    crate::Application::bootstrap_mock_with_store(
        workspace,
        store,
        Arc::new(crate::events::NoopEventSink),
        vec![],
    )
}

#[tokio::test]
async fn application_resources_use_existing_store_and_survive_recomposition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.db");
    {
        let store = SharedStore::open(&path, 1).unwrap();
        let application = app(dir.path(), Some(store.clone()));
        let service = application.resource_service(authority()).unwrap();
        service
            .create_organization("admin", "a".into(), "Acme".into())
            .await
            .unwrap();
        let row = service
            .create_team(
                "alice",
                "a".into(),
                "maintenance".into(),
                "Maintainers".into(),
            )
            .await
            .unwrap();
        assert_eq!(row.owner_principal_id, "alice");
        assert_eq!(
            store
                .read(|db| db.get_team("a", "maintenance"))
                .await
                .unwrap()
                .unwrap(),
            Some(row.clone())
        );
        assert_eq!(
            service
                .create_team(
                    "alice",
                    "a".into(),
                    "maintenance".into(),
                    "Maintainers".into()
                )
                .await
                .unwrap(),
            row
        );
        assert!(matches!(
            service
                .create_team(
                    "bob",
                    "a".into(),
                    "maintenance".into(),
                    "Maintainers".into()
                )
                .await,
            Err(ResourceError::Conflict)
        ));
    }
    let application = app(dir.path(), Some(SharedStore::open(path, 1).unwrap()));
    let service = application.resource_service(authority()).unwrap();
    assert_eq!(
        service
            .get_team("alice", "a".into(), "maintenance".into())
            .await
            .unwrap()
            .unwrap()
            .owner_principal_id,
        "alice"
    );
    assert_eq!(
        service
            .get_organization("admin", "a".into())
            .await
            .unwrap()
            .unwrap()
            .name,
        "Acme"
    );
}

#[tokio::test]
async fn denied_scope_and_action_never_mutate_or_disclose_resources() {
    let dir = tempfile::tempdir().unwrap();
    let store = SharedStore::open(dir.path().join("resources.db"), 1).unwrap();
    let application = app(dir.path(), Some(store.clone()));
    let service = application.resource_service(authority()).unwrap();
    service
        .create_organization("admin", "a".into(), "Acme".into())
        .await
        .unwrap();
    service
        .create_organization("admin", "b".into(), "Other".into())
        .await
        .unwrap();
    service
        .create_team(
            "alice",
            "a".into(),
            "maintenance".into(),
            "Maintainers".into(),
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .create_organization("alice", "c".into(), "Unauthorized".into())
            .await,
        Err(ResourceError::Denied)
    ));
    assert!(matches!(
        service
            .create_team(
                "alice",
                "b".into(),
                "maintenance".into(),
                "Unauthorized".into()
            )
            .await,
        Err(ResourceError::Denied)
    ));
    for (credential, org, team) in [
        ("bob", "a", "maintenance"),
        ("alice", "b", "maintenance"),
        ("alice", "a", "unknown"),
        ("forged", "a", "maintenance"),
    ] {
        assert!(matches!(
            service.get_team(credential, org.into(), team.into()).await,
            Err(ResourceError::Denied)
        ));
    }
    assert!(matches!(
        service.get_organization("alice", "a".into()).await,
        Err(ResourceError::Denied)
    ));
    assert!(store
        .read(|db| db.get_organization("c"))
        .await
        .unwrap()
        .unwrap()
        .is_none());
    assert!(store
        .read(|db| db.get_team("b", "maintenance"))
        .await
        .unwrap()
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn revocation_is_checked_on_reads_and_idempotent_retries() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(
        dir.path(),
        Some(SharedStore::open(dir.path().join("resources.db"), 1).unwrap()),
    );
    let grants = authority();
    let service = application.resource_service(grants.clone()).unwrap();
    service
        .create_organization("admin", "a".into(), "Acme".into())
        .await
        .unwrap();
    service
        .create_team(
            "alice",
            "a".into(),
            "maintenance".into(),
            "Maintainers".into(),
        )
        .await
        .unwrap();
    grants.revoked.store(true, Ordering::SeqCst);
    assert!(matches!(
        service
            .get_team("alice", "a".into(), "maintenance".into())
            .await,
        Err(ResourceError::Denied)
    ));
    assert!(matches!(
        service
            .create_team(
                "alice",
                "a".into(),
                "maintenance".into(),
                "Maintainers".into()
            )
            .await,
        Err(ResourceError::Denied)
    ));
    assert!(matches!(
        service
            .create_organization("admin", "a".into(), "Acme".into())
            .await,
        Err(ResourceError::Denied)
    ));
}

#[test]
fn volatile_application_cannot_compose_resource_service() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        app(dir.path(), None).resource_service(authority()),
        Err(ResourceError::StorageRequired)
    ));
    assert!(AuthorizedPrincipal::new(" ".into()).is_err());
}

struct VerifiedAlice;

#[tokio::test]
async fn local_credentials_authenticate_real_resource_operations_and_revoke_independently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local-auth.db");
    let store = SharedStore::open(&path, 1).unwrap();
    store
        .write(|db| -> Result<(), StoreError> {
            db.put_control_principal("local/alice", true, false)?;
            db.create_organization(&OrganizationRow {
                org_id: "a".into(),
                name: "A".into(),
            })?;
            db.set_organization_member(
                "a",
                "local/alice",
                tetonic_memory::OrganizationRole::TeamCreator,
            )
        })
        .await
        .unwrap()
        .unwrap();
    let application = app(dir.path(), Some(store));
    let verifier = Arc::new(application.local_credentials("engine-a".into()).unwrap());
    assert!(verifier.issue("local/alice".into(), 0).await.is_err());
    assert!(verifier.issue("local/alice".into(), 86401).await.is_err());
    assert!(verifier.issue("unknown".into(), 60).await.is_err());
    let key = verifier.issue("local/alice".into(), 3600).await.unwrap();
    let second = verifier.issue("local/alice".into(), 3600).await.unwrap();
    assert_ne!(key.expose_secret(), second.expose_secret());
    assert!(!format!("{key:?}").contains(key.expose_secret()));
    let service = application
        .membership_resource_service(verifier.clone())
        .unwrap();
    let team = service
        .create_team(
            key.expose_secret(),
            "a".into(),
            "team".into(),
            "Team".into(),
        )
        .await
        .unwrap();
    assert_eq!(team.owner_principal_id, "local/alice");
    assert!(application
        .local_credentials("engine-b".into())
        .unwrap()
        .verify(key.expose_secret())
        .await
        .is_err());
    for bad in ["local/alice", "ttc_", "ttc_forged"] {
        assert!(matches!(
            service.get_team(bad, "a".into(), "team".into()).await,
            Err(ResourceError::Denied)
        ));
    }
    verifier.revoke(key.credential_id.clone()).await.unwrap();
    assert!(matches!(
        service
            .get_team(key.expose_secret(), "a".into(), "team".into())
            .await,
        Err(ResourceError::Denied)
    ));
    assert!(service
        .get_team(second.expose_secret(), "a".into(), "team".into())
        .await
        .unwrap()
        .is_some());
    let reopened = app(dir.path(), Some(SharedStore::open(path, 1).unwrap()));
    let reopened_verifier = reopened.local_credentials("engine-a".into()).unwrap();
    assert!(reopened_verifier.verify(key.expose_secret()).await.is_err());
    assert!(reopened_verifier
        .verify(second.expose_secret())
        .await
        .is_ok());
}

#[async_trait]
impl CredentialVerifier for VerifiedAlice {
    async fn verify(&self, credential: &str) -> Result<AuthorizedPrincipal, AccessError> {
        if credential != "test-session" {
            return Err(AccessError);
        }
        AuthorizedPrincipal::new("issuer/alice".into())
    }
}

#[tokio::test]
async fn persistent_authority_uses_verified_identity_and_current_memberships() {
    let dir = tempfile::tempdir().unwrap();
    let store = SharedStore::open(dir.path().join("access.db"), 1).unwrap();
    store
        .write(|db| -> Result<(), StoreError> {
            db.create_organization(&OrganizationRow {
                org_id: "a".into(),
                name: "Acme".into(),
            })?;
            db.put_control_principal("issuer/alice", true, false)?;
            db.set_organization_member(
                "a",
                "issuer/alice",
                tetonic_memory::OrganizationRole::TeamCreator,
            )
        })
        .await
        .unwrap()
        .unwrap();
    let application = app(dir.path(), Some(store.clone()));
    let service = application
        .membership_resource_service(Arc::new(VerifiedAlice))
        .unwrap();
    assert!(matches!(
        service
            .create_organization("test-session", "b".into(), "B".into())
            .await,
        Err(ResourceError::Denied)
    ));
    let team = service
        .create_team("test-session", "a".into(), "team".into(), "Team".into())
        .await
        .unwrap();
    assert_eq!(team.owner_principal_id, "issuer/alice");
    assert_eq!(
        service
            .get_team("test-session", "a".into(), "team".into())
            .await
            .unwrap(),
        Some(team)
    );
    assert!(matches!(
        service
            .get_team("issuer/alice", "a".into(), "team".into())
            .await,
        Err(ResourceError::Denied)
    ));
    store
        .write(|db| db.remove_organization_member("a", "issuer/alice"))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        service
            .get_team("test-session", "a".into(), "team".into())
            .await,
        Err(ResourceError::Denied)
    ));
    assert!(matches!(
        service
            .create_team("test-session", "a".into(), "team".into(), "Team".into())
            .await,
        Err(ResourceError::Denied)
    ));
}
