//! Durable control resources. These repositories require an already-authorized
//! caller; an organization ID is a storage scope, not authentication evidence.
//! Creation never starts execution, grants capabilities or charges inference.

use rusqlite::{params, OptionalExtension};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationRow {
    pub org_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRow {
    pub org_id: String,
    pub team_id: String,
    pub name: String,
    /// Stable principal reference. Membership/grants are a separate contract.
    pub owner_principal_id: String,
}

fn validate_field(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(StoreError::InvalidControlResource(field.into()));
    }
    Ok(())
}

impl Store {
    pub(crate) fn migrate_team_resources_v29(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version = 29)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS organizations (
                org_id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS teams (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                name TEXT NOT NULL,
                owner_principal_id TEXT NOT NULL,
                PRIMARY KEY (org_id, team_id),
                FOREIGN KEY (org_id) REFERENCES organizations(org_id)
             );",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version, applied_at) VALUES (29, ?1)",
            params![crate::util::now()],
        )?;
        Ok(())
    }

    /// Exact retries are idempotent; changed attributes require an explicit
    /// update operation rather than silently overwriting another creator.
    pub fn create_organization(&self, row: &OrganizationRow) -> Result<()> {
        validate_field(&row.org_id, "org_id")?;
        validate_field(&row.name, "name")?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.conn.execute(
            "INSERT INTO organizations(org_id, name) VALUES (?1, ?2)
             ON CONFLICT(org_id) DO NOTHING",
            params![row.org_id, row.name],
        )?;
        if self.get_organization(&row.org_id)?.as_ref() != Some(row) {
            return Err(StoreError::ControlResourceConflict);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_organization(&self, org_id: &str) -> Result<Option<OrganizationRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id, name FROM organizations WHERE org_id = ?1",
                params![org_id],
                |r| {
                    Ok(OrganizationRow {
                        org_id: r.get(0)?,
                        name: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn create_team(&self, row: &TeamRow) -> Result<()> {
        validate_field(&row.org_id, "org_id")?;
        validate_field(&row.team_id, "team_id")?;
        validate_field(&row.name, "name")?;
        validate_field(&row.owner_principal_id, "owner_principal_id")?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.conn.execute(
            "INSERT INTO teams(org_id, team_id, name, owner_principal_id)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT(org_id, team_id) DO NOTHING",
            params![row.org_id, row.team_id, row.name, row.owner_principal_id],
        )?;
        if self.get_team(&row.org_id, &row.team_id)?.as_ref() != Some(row) {
            return Err(StoreError::ControlResourceConflict);
        }
        tx.commit()?;
        Ok(())
    }

    /// No global lookup by team ID: the organization is required at this door.
    pub fn get_team(&self, org_id: &str, team_id: &str) -> Result<Option<TeamRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT org_id, team_id, name, owner_principal_id FROM teams
             WHERE org_id = ?1 AND team_id = ?2",
                params![org_id, team_id],
                |r| {
                    Ok(TeamRow {
                        org_id: r.get(0)?,
                        team_id: r.get(1)?,
                        name: r.get(2)?,
                        owner_principal_id: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(id: &str) -> OrganizationRow {
        OrganizationRow {
            org_id: id.into(),
            name: format!("Organization {id}"),
        }
    }
    fn team(org_id: &str) -> TeamRow {
        TeamRow {
            org_id: org_id.into(),
            team_id: "maintainers".into(),
            name: format!("{org_id} maintainers"),
            owner_principal_id: format!("{org_id}-owner"),
        }
    }

    #[test]
    fn team_keys_are_scoped_and_resources_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.db");
        {
            let store = Store::open(&path).unwrap();
            for id in ["a", "b"] {
                store.create_organization(&org(id)).unwrap();
                store.create_team(&team(id)).unwrap();
            }
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get_team("a", "maintainers").unwrap(), Some(team("a")));
        assert_eq!(store.get_team("b", "maintainers").unwrap(), Some(team("b")));
        assert!(store.get_team("other", "maintainers").unwrap().is_none());
        assert_eq!(store.get_organization("a").unwrap(), Some(org("a")));
    }

    #[test]
    fn retries_do_not_overwrite_names_or_ownership() {
        let store = Store::open(":memory:").unwrap();
        store.create_organization(&org("a")).unwrap();
        store.create_organization(&org("a")).unwrap();
        let mut changed_org = org("a");
        changed_org.name = "replacement".into();
        assert!(matches!(
            store.create_organization(&changed_org),
            Err(StoreError::ControlResourceConflict)
        ));
        store.create_team(&team("a")).unwrap();
        store.create_team(&team("a")).unwrap();
        let mut changed = team("a");
        changed.owner_principal_id = "another-owner".into();
        assert!(matches!(
            store.create_team(&changed),
            Err(StoreError::ControlResourceConflict)
        ));
        assert_eq!(store.get_team("a", "maintainers").unwrap(), Some(team("a")));
    }

    #[test]
    fn invalid_or_orphaned_resources_are_not_persisted() {
        let store = Store::open(":memory:").unwrap();
        assert!(store.create_team(&team("missing")).is_err());
        assert!(store.get_team("missing", "maintainers").unwrap().is_none());
        store.create_organization(&org("a")).unwrap();
        let mut invalid = team("a");
        invalid.owner_principal_id = " ".into();
        assert!(matches!(
            store.create_team(&invalid),
            Err(StoreError::InvalidControlResource(_))
        ));
        assert!(store.get_team("a", "maintainers").unwrap().is_none());
    }

    #[test]
    fn upgrade_from_v28_preserves_identity_and_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
        {
            let store = Store::open(&path).unwrap();
            store
                .conn
                .execute_batch(
                    "DROP TABLE control_credential_events; DROP TABLE control_credentials; DROP TABLE team_members; DROP TABLE organization_members; DROP TABLE control_principals;
                DROP TABLE teams; DROP TABLE organizations;
                DELETE FROM schema_versions WHERE version >= 29;
                INSERT INTO agent_identities VALUES
                ('legacy', 'coding', 'digest', 'default', '[]', '[]', 'legacy', 't', 't');",
                )
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .get_agent_identity("legacy")
                .unwrap()
                .unwrap()
                .bound_definition_digest,
            "digest"
        );
        store.create_organization(&org("a")).unwrap();
        store.create_team(&team("a")).unwrap();
        assert!(crate::pre_migrate_backup_directory(&path).is_dir());
    }

    #[test]
    fn competing_creators_cannot_replace_team_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("race.db");
        Store::open(&path)
            .unwrap()
            .create_organization(&org("a"))
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["owner-1", "owner-2"]
            .into_iter()
            .map(|owner| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let store = Store::open(path).unwrap();
                    let mut row = team("a");
                    row.owner_principal_id = owner.into();
                    barrier.wait();
                    match store.create_team(&row) {
                        Ok(()) => Some(row),
                        Err(StoreError::ControlResourceConflict) => None,
                        Err(other) => panic!("unexpected create error: {other}"),
                    }
                })
            })
            .collect();
        let winners: Vec<_> = handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect();
        assert_eq!(winners.len(), 1);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .get_team("a", "maintainers")
                .unwrap(),
            Some(winners[0].clone())
        );
    }
}
