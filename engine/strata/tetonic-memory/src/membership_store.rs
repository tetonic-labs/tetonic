//! Durable control-plane membership. Mutation methods are trusted storage doors,
//! not public administrative APIs. They grant no execution or knowledge access.

use crate::{Result, Store, StoreError};
use rusqlite::params;

#[derive(Debug, Clone, Copy)]
pub enum OrganizationRole {
    Administrator,
    TeamCreator,
    Member,
}

impl OrganizationRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Administrator => "administrator",
            Self::TeamCreator => "team_creator",
            Self::Member => "member",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrganizationRow, TeamRow};

    fn seed(store: &Store) {
        for id in ["a", "b"] {
            store
                .create_organization(&OrganizationRow {
                    org_id: id.into(),
                    name: id.into(),
                })
                .unwrap();
            store
                .create_team(&TeamRow {
                    org_id: id.into(),
                    team_id: "team".into(),
                    name: id.into(),
                    owner_principal_id: "owner".into(),
                })
                .unwrap();
        }
        for id in ["member", "owner", "admin"] {
            store
                .put_control_principal(id, true, id == "admin")
                .unwrap();
            store
                .set_organization_member("a", id, OrganizationRole::Member)
                .unwrap();
        }
    }

    #[test]
    fn permissions_are_scoped_and_platform_role_does_not_imply_membership() {
        let store = Store::open(":memory:").unwrap();
        seed(&store);
        assert!(store
            .control_access("admin", ControlPermission::CreateOrganization, "", "")
            .unwrap());
        assert!(!store
            .control_access("admin", ControlPermission::ReadOrganization, "b", "")
            .unwrap());
        assert!(!store
            .control_access("member", ControlPermission::CreateTeam, "a", "team")
            .unwrap());
        store
            .set_organization_member("a", "member", OrganizationRole::TeamCreator)
            .unwrap();
        assert!(store
            .control_access("member", ControlPermission::CreateTeam, "a", "team")
            .unwrap());
        assert!(!store
            .control_access("member", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
        store.add_team_member("a", "team", "member").unwrap();
        assert!(store
            .control_access("member", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
        assert!(!store
            .control_access("member", ControlPermission::ReadTeam, "b", "team")
            .unwrap());
        assert!(store.add_team_member("b", "team", "member").is_err());
        assert!(store.add_team_member("a", "missing", "member").is_err());
        assert!(store
            .control_access("owner", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
        assert!(!store
            .control_access("owner", ControlPermission::ReadTeam, "b", "team")
            .unwrap());
        store
            .set_organization_member("a", "admin", OrganizationRole::Administrator)
            .unwrap();
        assert!(store
            .control_access("admin", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
        assert!(store
            .control_access("admin", ControlPermission::CreateTeam, "a", "new")
            .unwrap());
        assert!(!store
            .control_access("admin", ControlPermission::ReadTeam, "b", "team")
            .unwrap());
        store
            .set_organization_member("a", "admin", OrganizationRole::Member)
            .unwrap();
        assert!(!store
            .control_access("admin", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
    }

    #[test]
    fn revocation_survives_reopen_and_org_rejoin_does_not_restore_team_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grants.db");
        {
            let store = Store::open(&path).unwrap();
            seed(&store);
            store.add_team_member("a", "team", "member").unwrap();
            store.remove_organization_member("a", "member").unwrap();
            store
                .set_organization_member("a", "member", OrganizationRole::Member)
                .unwrap();
            store.put_control_principal("owner", false, false).unwrap();
            store.put_control_principal("admin", false, true).unwrap();
        }
        let store = Store::open(path).unwrap();
        for id in ["member", "owner", "admin", "unknown"] {
            assert!(!store
                .control_access(id, ControlPermission::ReadTeam, "a", "team")
                .unwrap());
        }
        assert!(!store
            .control_access("admin", ControlPermission::CreateOrganization, "", "")
            .unwrap());
        store.add_team_member("a", "team", "member").unwrap();
        store.remove_team_member("a", "team", "member").unwrap();
        assert!(!store
            .control_access("member", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
    }

    #[test]
    fn upgrading_v29_preserves_teams_and_grants_no_implicit_access() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.db");
        {
            let store = Store::open(&path).unwrap();
            seed(&store);
            store.conn.execute_batch("DROP TABLE control_admin_events; DROP TABLE control_credential_events; DROP TABLE control_credentials; DROP TABLE team_members; DROP TABLE organization_members; DROP TABLE control_principals; DELETE FROM schema_versions WHERE version>=30;").unwrap();
        }
        let store = Store::open(path).unwrap();
        assert!(store.get_team("a", "team").unwrap().is_some());
        assert!(!store
            .control_access("owner", ControlPermission::ReadTeam, "a", "team")
            .unwrap());
        assert!(!store
            .control_access("admin", ControlPermission::CreateOrganization, "", "")
            .unwrap());
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ControlPermission {
    CreateOrganization,
    ReadOrganization,
    CreateTeam,
    ReadTeam,
}

impl Store {
    pub(crate) fn migrate_memberships_v30(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=30)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS control_principals (
                principal_id TEXT PRIMARY KEY NOT NULL,
                enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
                platform_admin INTEGER NOT NULL CHECK(platform_admin IN (0,1))
             );
             CREATE TABLE IF NOT EXISTS organization_members (
                org_id TEXT NOT NULL REFERENCES organizations(org_id),
                principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
                role TEXT NOT NULL CHECK(role IN ('administrator','team_creator','member')),
                PRIMARY KEY(org_id,principal_id)
             );
             CREATE TABLE IF NOT EXISTS team_members (
                org_id TEXT NOT NULL,
                team_id TEXT NOT NULL,
                principal_id TEXT NOT NULL,
                PRIMARY KEY(org_id,team_id,principal_id),
                FOREIGN KEY(org_id,team_id) REFERENCES teams(org_id,team_id),
                FOREIGN KEY(org_id,principal_id) REFERENCES organization_members(org_id,principal_id) ON DELETE CASCADE
             );"
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES (30,?1)",
            params![crate::util::now()],
        )?;
        Ok(())
    }

    pub fn put_control_principal(
        &self,
        principal_id: &str,
        enabled: bool,
        platform_admin: bool,
    ) -> Result<()> {
        if principal_id.trim().is_empty() || principal_id.contains('\0') {
            return Err(StoreError::InvalidControlResource("principal_id".into()));
        }
        self.conn.execute("INSERT INTO control_principals VALUES (?1,?2,?3)
            ON CONFLICT(principal_id) DO UPDATE SET enabled=excluded.enabled, platform_admin=excluded.platform_admin",
            params![principal_id,enabled,platform_admin])?;
        Ok(())
    }

    pub fn set_organization_member(
        &self,
        org_id: &str,
        principal_id: &str,
        role: OrganizationRole,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO organization_members VALUES (?1,?2,?3)
            ON CONFLICT(org_id,principal_id) DO UPDATE SET role=excluded.role",
            params![org_id, principal_id, role.as_str()],
        )?;
        Ok(())
    }

    pub fn remove_organization_member(&self, org_id: &str, principal_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM organization_members WHERE org_id=?1 AND principal_id=?2",
            params![org_id, principal_id],
        )?;
        Ok(())
    }

    pub fn add_team_member(&self, org_id: &str, team_id: &str, principal_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO team_members VALUES (?1,?2,?3) ON CONFLICT DO NOTHING",
            params![org_id, team_id, principal_id],
        )?;
        Ok(())
    }

    pub fn remove_team_member(
        &self,
        org_id: &str,
        team_id: &str,
        principal_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM team_members WHERE org_id=?1 AND team_id=?2 AND principal_id=?3",
            params![org_id, team_id, principal_id],
        )?;
        Ok(())
    }

    /// Single-statement snapshot: disabled principals and missing org membership
    /// deny even if a team membership or owner reference still exists.
    pub fn control_access(
        &self,
        principal_id: &str,
        permission: ControlPermission,
        org_id: &str,
        team_id: &str,
    ) -> Result<bool> {
        let action = match permission {
            ControlPermission::CreateOrganization => "create_org",
            ControlPermission::ReadOrganization => "read_org",
            ControlPermission::CreateTeam => "create_team",
            ControlPermission::ReadTeam => "read_team",
        };
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM control_principals p
             WHERE p.principal_id=?1 AND p.enabled=1 AND (
               (?2='create_org' AND p.platform_admin=1) OR
               EXISTS(SELECT 1 FROM organization_members m WHERE m.org_id=?3 AND m.principal_id=p.principal_id AND (
                 ?2='read_org' OR
                 (?2='create_team' AND m.role IN ('administrator','team_creator')) OR
                 (?2='read_team' AND (m.role='administrator' OR
                   EXISTS(SELECT 1 FROM teams t WHERE t.org_id=?3 AND t.team_id=?4 AND t.owner_principal_id=p.principal_id) OR
                   EXISTS(SELECT 1 FROM team_members t WHERE t.org_id=?3 AND t.team_id=?4 AND t.principal_id=p.principal_id)))
               ))
             ))", params![principal_id,action,org_id,team_id], |r| r.get(0))?)
    }
}
