//! Shared capacity-plane DDL (coordinator `lokai.db` + worker `worker.db`).

pub const CREATE_RUNTIME_PROFILES: &str =
    "CREATE TABLE IF NOT EXISTS runtime_profiles (
         id            TEXT PRIMARY KEY,
         node_id       TEXT NOT NULL,
         role          TEXT NOT NULL,
         fingerprint   TEXT NOT NULL,
         created_at    TEXT NOT NULL,
         gates_passed  INTEGER NOT NULL,
         json          TEXT NOT NULL
     );
     CREATE INDEX IF NOT EXISTS idx_runtime_profiles_node ON runtime_profiles(node_id, created_at);";

pub const CREATE_CAPACITY_BINDINGS: &str = "CREATE TABLE IF NOT EXISTS capacity_bindings (
         node_id            TEXT NOT NULL,
         role               TEXT NOT NULL,
         active_profile_id  TEXT,
         updated_at         TEXT NOT NULL,
         PRIMARY KEY (node_id, role)
     );";

pub const INSERT_RUNTIME_PROFILE: &str =
    "INSERT INTO runtime_profiles(id, node_id, role, fingerprint, created_at, gates_passed, json)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

pub const SELECT_RUNTIME_PROFILE: &str =
    "SELECT id, node_id, role, fingerprint, created_at, gates_passed, json
     FROM runtime_profiles WHERE id = ?1";

pub const LIST_RUNTIME_PROFILES: &str =
    "SELECT id, node_id, role, fingerprint, created_at, gates_passed, json
     FROM runtime_profiles ORDER BY created_at DESC";

pub const LIST_RUNTIME_PROFILES_FOR_NODE: &str =
    "SELECT id, node_id, role, fingerprint, created_at, gates_passed, json
     FROM runtime_profiles WHERE node_id = ?1 ORDER BY created_at DESC";

pub const UPSERT_CAPACITY_BINDING: &str =
    "INSERT INTO capacity_bindings(node_id, role, active_profile_id, updated_at)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(node_id, role) DO UPDATE SET
       active_profile_id = excluded.active_profile_id,
       updated_at = excluded.updated_at";

pub const SELECT_CAPACITY_BINDING: &str =
    "SELECT active_profile_id FROM capacity_bindings WHERE node_id = ?1 AND role = ?2";
