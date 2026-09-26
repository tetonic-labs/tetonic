//! Explicit copy of one stored message into another information context.
//! The destination record does not grant later reads of the source.
use crate::{new_id, Result, Store, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

/// Provenance for one authorized disclosure. The body lives only in the
/// destination message; this receipt does not include that body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPublication {
    pub publication_id: String,
    pub destination_session_id: String,
    pub destination_message_seq: i64,
    pub source_digest: String,
}

impl Store {
    pub(crate) fn migrate_context_publications_v44(&self) -> Result<()> {
        if self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_versions WHERE version=44)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        self.conn.execute_batch(
            "CREATE TABLE context_publications (
            publication_id TEXT PRIMARY KEY NOT NULL,
            request_id TEXT NOT NULL,
            publisher_principal_id TEXT NOT NULL REFERENCES control_principals(principal_id),
            source_context_id TEXT NOT NULL REFERENCES information_contexts(context_id),
            source_session_id TEXT NOT NULL,
            source_message_seq INTEGER NOT NULL,
            source_digest TEXT NOT NULL,
            destination_context_id TEXT NOT NULL REFERENCES information_contexts(context_id),
            destination_session_id TEXT NOT NULL,
            destination_message_seq INTEGER NOT NULL,
            published_at TEXT NOT NULL,
            UNIQUE(publisher_principal_id, destination_context_id, request_id)
        );
        CREATE TRIGGER context_publication_immutable BEFORE UPDATE ON context_publications
        BEGIN SELECT RAISE(ABORT,'publication is immutable'); END;",
        )?;
        self.conn.execute(
            "INSERT INTO schema_versions(version,applied_at) VALUES(44,?1)",
            [crate::util::now()],
        )?;
        Ok(())
    }

    /// Copy one existing message into an open destination discussion.
    /// The caller cannot substitute a paraphrase: the stored source bytes are the
    /// approved content. A repeated request with the same source and destination
    /// returns the original receipt. A repeated request that names different
    /// source coordinates conflicts.
    pub fn publish_context_message(
        &self,
        actor: &str,
        source_context: &str,
        source_session: &str,
        source_seq: i64,
        destination_context: &str,
        destination_session: &str,
        request_id: &str,
    ) -> Result<ContextPublication> {
        validate_request(request_id)?;
        if source_context == destination_context {
            return Err(StoreError::InvalidControlResource(
                "publication_destination".into(),
            ));
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        self.require_context_access(actor, source_context)?;
        self.require_context_access(actor, destination_context)?;
        let existing: Option<(String, String, String, i64, String, i64, String)> = self
            .conn
            .query_row(
                "SELECT publication_id,source_context_id,source_session_id,source_message_seq,destination_session_id,destination_message_seq,source_digest
                 FROM context_publications
                 WHERE publisher_principal_id=?1 AND destination_context_id=?2 AND request_id=?3",
                params![actor, destination_context, request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .optional()?;
        if let Some((id, context, session, seq, destination, destination_seq, digest)) = existing {
            if context == source_context
                && session == source_session
                && seq == source_seq
                && destination == destination_session
            {
                tx.commit()?;
                return Ok(ContextPublication {
                    publication_id: id,
                    destination_session_id: destination,
                    destination_message_seq: destination_seq,
                    source_digest: digest,
                });
            }
            return Err(StoreError::ControlResourceConflict);
        }
        let source: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT m.role,m.content FROM messages m
                 JOIN sessions s ON s.id=m.session_id
                 WHERE s.id=?1 AND s.context_id=?2 AND m.seq=?3
                   AND m.role IN ('user','assistant')
                   AND NOT EXISTS(SELECT 1 FROM spawn_rollbacks r WHERE r.session_id=m.session_id AND r.agent_id=m.agent_id)",
                params![source_session, source_context, source_seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((role, content)) = source else {
            return Err(StoreError::ControlAccessDenied);
        };
        if content.is_empty() {
            return Err(StoreError::InvalidControlResource("message".into()));
        }
        let open: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND context_id=?2 AND mode='discussion' AND status='open')",
            params![destination_session, destination_context],
            |r| r.get(0),
        )?;
        if !open {
            return Err(StoreError::ControlAccessDenied);
        }
        let digest = format!("sha256:{:x}", Sha256::digest(content.as_bytes()));
        let row = self.append_message(destination_session, &role, "", &content, None)?;
        self.conn.execute(
            "UPDATE messages SET author_principal_id=?1,client_message_id=?2 WHERE rowid=?3",
            params![actor, request_id, row],
        )?;
        let destination_seq: i64 =
            self.conn
                .query_row("SELECT seq FROM messages WHERE rowid=?1", [row], |r| {
                    r.get(0)
                })?;
        let publication_id = new_id("pub");
        self.conn.execute(
            "INSERT INTO context_publications(publication_id,request_id,publisher_principal_id,source_context_id,source_session_id,source_message_seq,source_digest,destination_context_id,destination_session_id,destination_message_seq,published_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                publication_id,
                request_id,
                actor,
                source_context,
                source_session,
                source_seq,
                digest,
                destination_context,
                destination_session,
                destination_seq,
                crate::util::now()
            ],
        )?;
        tx.commit()?;
        Ok(ContextPublication {
            publication_id,
            destination_session_id: destination_session.to_string(),
            destination_message_seq: destination_seq,
            source_digest: digest,
        })
    }
}

fn validate_request(id: &str) -> Result<()> {
    if id.trim().is_empty() || id.contains('\0') || id.len() > 256 {
        return Err(StoreError::InvalidControlResource("request_id".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextOwner, OrganizationRole, TeamRow};

    #[test]
    fn publication_copies_into_destination_without_opening_the_source() {
        let db = Store::open(":memory:").unwrap();
        db.bootstrap_control("alice", "org", "Org").unwrap();
        db.register_control_principal("bob").unwrap();
        db.register_control_principal("admin").unwrap();
        db.set_organization_member("org", "bob", OrganizationRole::Member)
            .unwrap();
        db.set_organization_member("org", "admin", OrganizationRole::Administrator)
            .unwrap();
        db.create_team(&TeamRow {
            org_id: "org".into(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_principal_id: "alice".into(),
        })
        .unwrap();
        db.add_team_member("org", "team", "bob").unwrap();
        db.create_information_context(
            "alice",
            "private",
            &ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .unwrap();
        db.create_information_context(
            "alice",
            "shared",
            &ContextOwner::Team {
                org_id: "org".into(),
                team_id: "team".into(),
            },
        )
        .unwrap();
        db.insert_open_discussion("alice", "private", "private")
            .unwrap();
        db.insert_open_discussion("alice", "shared", "shared")
            .unwrap();
        let seq = db
            .append_context_message(
                "alice",
                "private",
                "private",
                "secret",
                "searchword PRIVATECANARY",
            )
            .unwrap();
        assert!(db
            .publish_context_message("bob", "private", "private", seq, "shared", "shared", "share")
            .is_err());
        assert!(db
            .publish_context_message(
                "admin", "private", "private", seq, "shared", "shared", "share"
            )
            .is_err());
        assert!(db
            .recall_context_messages("bob", "shared", "PRIVATECANARY", 10)
            .unwrap()
            .is_empty());
        let first = db
            .publish_context_message(
                "alice", "private", "private", seq, "shared", "shared", "share",
            )
            .unwrap();
        let again = db
            .publish_context_message(
                "alice", "private", "private", seq, "shared", "shared", "share",
            )
            .unwrap();
        assert_eq!(first, again);
        assert!(db
            .publish_context_message(
                "alice",
                "private",
                "private",
                seq + 1,
                "shared",
                "shared",
                "share"
            )
            .is_err());
        assert!(db
            .publish_context_message("bob", "private", "private", 99, "shared", "shared", "guess")
            .is_err());
        let hits = db
            .recall_context_messages("bob", "shared", "PRIVATECANARY", 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("PRIVATECANARY"));
        assert!(db
            .scoped_transcript("bob", "private", "private", 10)
            .is_err());
        assert_eq!(
            db.scoped_transcript("alice", "private", "private", 10)
                .unwrap()
                .len(),
            1
        );
        let shared = db.scoped_transcript("bob", "shared", "shared", 10).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].2, "searchword PRIVATECANARY");
        db.remove_team_member("org", "team", "bob").unwrap();
        assert!(db
            .recall_context_messages("bob", "shared", "PRIVATECANARY", 10)
            .is_err());
        assert!(db
            .conn
            .execute(
                "UPDATE context_publications SET source_digest='sha256:changed' WHERE publication_id=?1",
                [first.publication_id],
            )
            .is_err());
    }
}
