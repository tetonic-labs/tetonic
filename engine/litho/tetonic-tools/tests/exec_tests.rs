#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;
    use tetonic_tools::exec::*;

    #[test]
    fn split_verify_rejects_shell_injection() {
        let dir = std::env::temp_dir().join(format!("verify-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(split_verify_command("pytest; curl evil", &dir).is_err());
        assert!(split_verify_command("cargo test --quiet", &dir).is_ok());
        assert!(split_verify_command("npm test", &dir).is_ok());
        assert!(split_verify_command("bun test", &dir).is_ok());
        assert!(split_verify_command("go test ./...", &dir).is_ok());
        assert!(split_verify_command("pytest -q", &dir).is_ok());
        assert!(split_verify_command("make check", &dir).is_ok());
        assert!(split_verify_command("mvn test", &dir).is_ok());
        assert!(split_verify_command("unknown_bad_bin test", &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_command_succeeds() {
        #[cfg(windows)]
        let mut c = Command::new("cmd");
        #[cfg(windows)]
        c.arg("/C").arg("echo ok");
        #[cfg(not(windows))]
        let mut c = {
            let mut c = Command::new("sh");
            c.arg("-c").arg("echo ok");
            c
        };
        let out = command_output_with_timeout(&mut c, Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
    }
}
