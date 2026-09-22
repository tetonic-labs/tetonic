//! Manifest-owned grading-input digests. This detects persistent modifications;
//! it is not process isolation against concurrent replace/restore attacks.
use crate::manifest::GraderType;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

const MAX_FILES: usize = 64;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

pub(super) fn check(graders: &[GraderType], workspace: &Path) -> Option<&'static str> {
    check_inner(graders, workspace).err()
}

fn check_inner(graders: &[GraderType], workspace: &Path) -> Result<(), &'static str> {
    let executes = graders.iter().any(|grader| {
        matches!(
            grader,
            GraderType::TestExecution { .. }
                | GraderType::CommandExecution { .. }
                | GraderType::MutationTest { .. }
        )
    });
    if executes
        && !graders
            .iter()
            .any(|grader| matches!(grader, GraderType::ProtectedFiles { .. }))
    {
        return Err("grader_integrity_unconfigured");
    }
    let mut count = 0;
    let mut total = 0;
    for grader in graders {
        let GraderType::ProtectedFiles { sha256 } = grader else {
            continue;
        };
        if sha256.is_empty() {
            return Err("grader_integrity_invalid");
        }
        for (path, expected) in sha256 {
            count += 1;
            if count > MAX_FILES {
                return Err("grader_integrity_limit");
            }
            if expected.len() != 71
                || !expected.starts_with("sha256:")
                || !expected[7..]
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err("grader_integrity_invalid");
            }
            let mut file = open_input(workspace, path)?;
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 8192];
            let mut size = 0;
            loop {
                let n = file
                    .read(&mut buffer)
                    .map_err(|_| "grader_input_read_error")?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                total += n as u64;
                if size > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
                    return Err("grader_integrity_limit");
                }
                digest.update(&buffer[..n]);
            }
            if format!("sha256:{}", hex::encode(digest.finalize())) != *expected {
                return Err("grader_input_modified");
            }
        }
    }
    Ok(())
}

pub(super) fn open_input(workspace: &Path, path: &str) -> Result<fs::File, &'static str> {
    let relative = path.replace('\\', "/");
    if relative.is_empty() || relative.chars().any(|c| c.is_control() || c == ':') {
        return Err("grader_integrity_invalid");
    }
    let mut current = workspace.to_path_buf();
    let parts = relative.split('/').collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        if matches!(*part, "" | "." | "..") || part.ends_with(['.', ' ']) {
            return Err("grader_integrity_invalid");
        }
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(|_| "grader_input_missing")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("grader_input_alias");
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("grader_input_alias");
        }
        if index + 1 < parts.len() && !metadata.is_dir() {
            return Err("grader_integrity_invalid");
        }
    }
    let file = fs::File::open(&current).map_err(|_| "grader_input_missing")?;
    let metadata = file.metadata().map_err(|_| "grader_input_missing")?;
    if !metadata.is_file() {
        return Err("grader_integrity_invalid");
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("grader_integrity_limit");
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn pins(path: &str, contents: &[u8]) -> GraderType {
        GraderType::ProtectedFiles {
            sha256: BTreeMap::from([(
                path.into(),
                format!("sha256:{}", hex::encode(Sha256::digest(contents))),
            )]),
        }
    }

    #[tokio::test]
    async fn command_qualification_requires_trusted_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let context = crate::graders::GraderContext {
            output_digest: None,
            touched_files: vec![],
            found_secrets: false,
            workspace: temp.path().to_path_buf(),
        };
        for grader in [
            GraderType::CommandExecution {
                command: "nonexistent-executable".into(),
            },
            GraderType::TestExecution {
                command: "nonexistent-executable".into(),
                require_success: true,
                fail_if_skipped: true,
                expected_pass_count: Some(1),
            },
        ] {
            assert_eq!(
                crate::graders::grade(&[grader], &context).await.unwrap(),
                Some("grader_integrity_unconfigured")
            );
        }
    }

    #[test]
    fn modified_missing_and_oversized_inputs_cannot_pass() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("grade.py");
        fs::write(&path, b"original").unwrap();
        let graders = [pins("grade.py", b"original")];
        assert_eq!(check(&graders, temp.path()), None);
        fs::write(&path, b"forged pass").unwrap();
        assert_eq!(check(&graders, temp.path()), Some("grader_input_modified"));
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert_eq!(check(&graders, temp.path()), Some("grader_integrity_limit"));
        fs::remove_file(&path).unwrap();
        assert_eq!(check(&graders, temp.path()), Some("grader_input_missing"));
    }

    #[test]
    fn malformed_manifest_paths_and_digests_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        for path in [
            "../file",
            "/file",
            "C:\\file",
            "file:stream",
            "file.",
            "file ",
            "",
        ] {
            assert_eq!(
                check(&[pins(path, b"")], temp.path()),
                Some("grader_integrity_invalid"),
                "{path}"
            );
        }
        assert_eq!(
            check(
                &[GraderType::ProtectedFiles {
                    sha256: BTreeMap::new()
                }],
                temp.path()
            ),
            Some("grader_integrity_invalid")
        );
    }

    #[tokio::test]
    async fn integrity_runs_before_commands_regardless_of_manifest_order() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("grade.py"), b"forged pass").unwrap();
        let graders = [
            GraderType::CommandExecution {
                command: "nonexistent-executable".into(),
            },
            pins("grade.py", b"original"),
        ];
        let context = crate::graders::GraderContext {
            output_digest: None,
            touched_files: vec![],
            found_secrets: false,
            workspace: temp.path().to_path_buf(),
        };
        assert_eq!(
            crate::graders::grade(&graders, &context).await.unwrap(),
            Some("grader_input_modified")
        );
    }

    #[tokio::test]
    async fn successful_command_cannot_hide_grader_modification() {
        let temp = tempfile::tempdir().unwrap();
        let script =
            b"from pathlib import Path\nPath(__file__).write_text('forged pass')\nprint('done')\n";
        fs::write(temp.path().join("grade.py"), script).unwrap();
        let graders = [
            pins("grade.py", script),
            GraderType::CommandExecution {
                command: "python grade.py".into(),
            },
        ];
        let context = crate::graders::GraderContext {
            output_digest: None,
            touched_files: vec![],
            found_secrets: false,
            workspace: temp.path().to_path_buf(),
        };
        assert_eq!(
            crate::graders::grade(&graders, &context).await.unwrap(),
            Some("grader_input_modified")
        );
    }

    #[tokio::test]
    async fn unauthorized_changes_are_rejected_before_command_runs() {
        let temp = tempfile::tempdir().unwrap();
        let script = b"from pathlib import Path\nPath('ran').write_text('yes')\n";
        fs::write(temp.path().join("grade.py"), script).unwrap();
        let graders = [
            GraderType::CommandExecution {
                command: "python grade.py".into(),
            },
            pins("grade.py", script),
            GraderType::FileBoundary {
                allowed_paths: vec!["src/lib.rs".into()],
                prohibited_paths: vec![],
            },
        ];
        let context = crate::graders::GraderContext {
            output_digest: None,
            touched_files: vec![".cargo/config.toml".into()],
            found_secrets: false,
            workspace: temp.path().to_path_buf(),
        };
        assert_eq!(
            crate::graders::grade(&graders, &context).await.unwrap(),
            Some("grader_failed")
        );
        assert!(!temp.path().join("ran").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_grader_and_parent_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("real"), b"original").unwrap();
        std::os::unix::fs::symlink("real", temp.path().join("grade.py")).unwrap();
        assert_eq!(
            check(&[pins("grade.py", b"original")], temp.path()),
            Some("grader_input_alias")
        );
        std::os::unix::fs::symlink(temp.path(), temp.path().join("parent")).unwrap();
        assert_eq!(
            check(&[pins("parent/real", b"original")], temp.path()),
            Some("grader_input_alias")
        );
    }
}
