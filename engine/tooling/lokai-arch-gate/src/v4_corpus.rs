//! AUD-01 coding corpus registry (coding half of ARCH-V4-GATE-001 only).
//!
//! This inventory is not ARCH-V4-GATE-001 satisfied. The sessionless half is a
//! WORK-03 placeholder fixture. GATE-01 promotes SUB-002 / TOOL-001 / DEP-001 /
//! IFACE-001 / CMP-001 as production-failing detectors. Other ARCH-V4-* stay
//! planted inventory. walk_rs still skips fixtures/.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusLock {
    Preserve,
    Defect,
    PreserveNegativeControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorReach {
    Present,
    Absent,
    Bypass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodingCorpusRow {
    pub id: &'static str,
    pub lock: CorpusLock,
    pub paired: Option<&'static str>,
    pub establishing: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodingDoorRow {
    pub door: &'static str,
    pub bh_id: &'static str,
    pub reach: DoorReach,
}

/// Compile-time coding corpus. Names say coding; this is not GATE-001 satisfied.
pub const CODING_CORPUS: &[CodingCorpusRow] = &[
    CodingCorpusRow {
        id: "BH-RESP",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-01",
    },
    CodingCorpusRow {
        id: "BH-INVEST",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-01",
    },
    CodingCorpusRow {
        id: "BH-PLAN",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-01/CODE-03",
    },
    CodingCorpusRow {
        id: "BH-EDIT",
        lock: CorpusLock::Preserve,
        paired: Some("BH-FIN-COMMIT"),
        establishing: "CODE-01",
    },
    CodingCorpusRow {
        id: "BH-VERIFY-OK",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-01/WORK-FIN-01",
    },
    CodingCorpusRow {
        id: "BH-VERIFY-FAIL",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-01/WORK-FIN-01",
    },
    CodingCorpusRow {
        id: "BH-DEBUG",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-03",
    },
    CodingCorpusRow {
        id: "BH-REVIEW",
        lock: CorpusLock::Preserve,
        paired: Some("BH-ORCH-CRIT"),
        establishing: "CODE-03",
    },
    CodingCorpusRow {
        id: "BH-MULTI",
        lock: CorpusLock::Preserve,
        paired: Some("BH-ID-SESSION"),
        establishing: "CODE-02",
    },
    CodingCorpusRow {
        id: "BH-STREAM",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-02/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-APPR-ALLOW",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "PORTAL-01",
    },
    CodingCorpusRow {
        id: "BH-APPR-DENY",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "PORTAL-01",
    },
    CodingCorpusRow {
        id: "BH-APPR-LOSS",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "PORTAL-01/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-CANCEL",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "WORK-02",
    },
    CodingCorpusRow {
        id: "BH-ORCH-ROOT",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-03/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-ORCH-CRIT",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-03/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-ORCH-REV",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-03/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-ORCH-SPAWN",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "CODE-03/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-RECOVER",
        lock: CorpusLock::Preserve,
        paired: None,
        establishing: "WORK-FIN-02",
    },
    CodingCorpusRow {
        id: "BH-FIN-ERR",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02",
    },
    CodingCorpusRow {
        id: "BH-FIN-STRAND",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-FIN-DROP",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-FIN-COMMIT",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-01/SUB-01",
    },
    CodingCorpusRow {
        id: "BH-FIN-SIDE",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02",
    },
    CodingCorpusRow {
        id: "BH-ID-SESSION",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-01/WORK-02",
    },
    CodingCorpusRow {
        id: "BH-CMP-FABRIC",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02",
    },
    CodingCorpusRow {
        id: "BH-CMP-PATCH",
        lock: CorpusLock::Defect,
        paired: None,
        establishing: "WORK-FIN-02",
    },
    CodingCorpusRow {
        id: "BH-CMP-INFER",
        lock: CorpusLock::PreserveNegativeControl,
        paired: None,
        establishing: "WORK-02",
    },
];

/// Eval (AUD-PATH-005) cannot currently exhibit these modes. Not PRESERVE of never-verify.
pub const EVAL_DOOR_ABSENT: &[&str] = &[
    "BH-VERIFY-OK",
    "BH-VERIFY-FAIL",
    "BH-REVIEW",
    "BH-APPR-ALLOW",
    "BH-APPR-DENY",
    "BH-APPR-LOSS",
];

pub const BYPASS_AGENT_RUN: &str = "AUD-PATH-007";

/// Per-door reachability for coding corpus IDs. Eval ABSENT rows expire CODE-03 VERIFY.
pub const CODING_DOORS: &[CodingDoorRow] = &[
    CodingDoorRow {
        door: "AUD-PATH-001",
        bh_id: "BH-RESP",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-002",
        bh_id: "BH-RESP",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-003",
        bh_id: "BH-RESP",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-RESP",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-VERIFY-OK",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-VERIFY-FAIL",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-REVIEW",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-APPR-ALLOW",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-APPR-DENY",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-005",
        bh_id: "BH-APPR-LOSS",
        reach: DoorReach::Absent,
    },
    CodingDoorRow {
        door: "AUD-PATH-004",
        bh_id: "BH-DEBUG",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-004",
        bh_id: "BH-ORCH-SPAWN",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-006",
        bh_id: "BH-ORCH-ROOT",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-007",
        bh_id: "BH-RESP",
        reach: DoorReach::Bypass,
    },
    CodingDoorRow {
        door: "AUD-PATH-008",
        bh_id: "BH-CMP-INFER",
        reach: DoorReach::Present,
    },
    CodingDoorRow {
        door: "AUD-PATH-009",
        bh_id: "BH-RECOVER",
        reach: DoorReach::Present,
    },
];

pub const PLANTED_MUTANTS: &[&str] = &[
    "ARCH-V4-SUB-001.v4fix",
    "ARCH-V4-SUB-002.v4fix",
    "ARCH-V4-TOOL-001.v4fix",
    "ARCH-V4-DEP-001.v4fix",
    "ARCH-V4-WORK-001.v4fix",
    "ARCH-V4-WORK-002.v4fix",
    "ARCH-V4-WORK-003.v4fix",
    "ARCH-V4-FIN-001.v4fix",
    "ARCH-V4-FIN-002.v4fix",
    "ARCH-V4-CMP-001.v4fix",
    "ARCH-V4-APP-001.v4fix",
    "ARCH-V4-IFACE-001.v4fix",
    "ARCH-V4-OBS-001.v4fix",
    "ARCH-V4-GATE-001-coding.v4fix",
    "ARCH-V4-GATE-001-sessionless.v4fix",
];

pub fn fixtures_v4_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/v4")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn aud01_corpus_inventory_complete() {
        assert_eq!(CODING_CORPUS.len(), 28);
        let ids: HashSet<_> = CODING_CORPUS.iter().map(|r| r.id).collect();
        for id in [
            "BH-RESP",
            "BH-INVEST",
            "BH-PLAN",
            "BH-EDIT",
            "BH-VERIFY-OK",
            "BH-VERIFY-FAIL",
            "BH-DEBUG",
            "BH-REVIEW",
            "BH-MULTI",
            "BH-STREAM",
            "BH-APPR-ALLOW",
            "BH-APPR-DENY",
            "BH-APPR-LOSS",
            "BH-CANCEL",
            "BH-ORCH-ROOT",
            "BH-ORCH-CRIT",
            "BH-ORCH-REV",
            "BH-ORCH-SPAWN",
            "BH-RECOVER",
            "BH-FIN-ERR",
            "BH-FIN-STRAND",
            "BH-FIN-DROP",
            "BH-FIN-COMMIT",
            "BH-FIN-SIDE",
            "BH-ID-SESSION",
            "BH-CMP-FABRIC",
            "BH-CMP-PATCH",
            "BH-CMP-INFER",
        ] {
            assert!(ids.contains(id), "missing {id}");
        }
        let edit = CODING_CORPUS.iter().find(|r| r.id == "BH-EDIT").unwrap();
        assert_eq!(edit.paired, Some("BH-FIN-COMMIT"));
        let multi = CODING_CORPUS.iter().find(|r| r.id == "BH-MULTI").unwrap();
        assert_eq!(multi.paired, Some("BH-ID-SESSION"));
        let review = CODING_CORPUS.iter().find(|r| r.id == "BH-REVIEW").unwrap();
        assert_eq!(review.paired, Some("BH-ORCH-CRIT"));
        let appr_loss = CODING_CORPUS
            .iter()
            .find(|r| r.id == "BH-APPR-LOSS")
            .unwrap();
        assert_eq!(appr_loss.lock, CorpusLock::Defect);
        let infer = CODING_CORPUS
            .iter()
            .find(|r| r.id == "BH-CMP-INFER")
            .unwrap();
        assert_eq!(infer.lock, CorpusLock::PreserveNegativeControl);
        let fin_err = CODING_CORPUS.iter().find(|r| r.id == "BH-FIN-ERR").unwrap();
        assert_eq!(fin_err.lock, CorpusLock::Defect);
        assert_eq!(BYPASS_AGENT_RUN, "AUD-PATH-007");
        for id in EVAL_DOOR_ABSENT {
            assert!(ids.contains(id), "eval-absent id missing from corpus {id}");
            assert!(
                CODING_DOORS.iter().any(|d| d.door == "AUD-PATH-005"
                    && d.bh_id == *id
                    && d.reach == DoorReach::Absent),
                "eval ABSENT door row missing for {id}"
            );
        }
        assert!(
            !EVAL_DOOR_ABSENT.contains(&"BH-RESP"),
            "eval ABSENT must not mean eval never answers"
        );
        assert!(CODING_DOORS
            .iter()
            .any(|d| d.door == "AUD-PATH-007" && d.reach == DoorReach::Bypass));
        assert!(CODING_DOORS
            .iter()
            .any(|d| d.door == "AUD-PATH-004" && d.bh_id == "BH-ORCH-SPAWN"));
    }

    #[test]
    fn aud01_planted_mutants_exist_for_every_candidate_rule() {
        let dir = fixtures_v4_dir();
        for name in PLANTED_MUTANTS {
            let path = dir.join(name);
            assert!(path.is_file(), "missing planted mutant {}", path.display());
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("establishing:"),
                "{name} must name establishing package"
            );
            if *name == "ARCH-V4-GATE-001-sessionless.v4fix" {
                assert!(text.contains("WORK-03"));
                assert!(text.contains("AUD-PROOF-002"));
                assert!(
                    !text.contains("Session required"),
                    "sessionless placeholder must not require Session"
                );
            }
            if *name == "ARCH-V4-SUB-001.v4fix" {
                assert!(text.contains("SUB-01"));
                assert!(text.contains("not scanned as production"));
            }
            if *name == "ARCH-V4-SUB-002.v4fix" {
                assert!(text.contains("SUB-03"));
                assert!(text.contains("production-failing detector live"));
            }
        }
        assert_eq!(PLANTED_MUTANTS.len(), 15);
    }

    #[test]
    fn aud01_fixtures_dir_is_not_walked_as_production_rs() {
        let root = crate::engine_root();
        let files = crate::collect_rs_files(&root);
        for path in &files {
            let rel = path.to_string_lossy().replace('\\', "/");
            assert!(
                !rel.contains("/fixtures/"),
                "walk_rs must skip fixtures: {rel}"
            );
        }
    }
}
