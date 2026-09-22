use std::{path::Path, time::Duration};

use lokai_domain::execution::ProcessClass;
use lokai_sandbox::{SandboxError, SandboxedProcess};
use serde::Deserialize;

/// Grader commands are trusted harness inputs. Process policy stays in sandbox.
pub(super) async fn run(
    workspace: &Path,
    command: &str,
    timeout: Duration,
) -> Result<String, &'static str> {
    let outcome = run_captured(workspace, command, timeout).await?;
    if !outcome.success {
        return Err("grader_command_failed");
    }
    Ok(outcome.output)
}

pub(super) struct Outcome {
    pub success: bool,
    pub output: String,
}

pub(super) async fn run_captured(
    workspace: &Path,
    command: &str,
    timeout: Duration,
) -> Result<Outcome, &'static str> {
    let (program, args) = lokai_sandbox::split_verify_command(command, workspace)
        .map_err(|_| "grader_invalid_command")?;
    let backend = lokai_sandbox::platform_backend();
    let caps = backend.capabilities();
    if !caps.runtime_enforcement || !caps.output_limits {
        return Err("grader_limits_unavailable");
    }
    let mut request = lokai_sandbox::profile_for_class(ProcessClass::RepositoryTool, workspace);
    // Toolchain discovery needs these paths; do not inherit arbitrary credentials
    // or compiler flags from the invoking process.
    request.environment.allowlist.extend(
        [
            "CARGO_HOME",
            "RUSTUP_HOME",
            "APPDATA",
            "LOCALAPPDATA",
            "ProgramData",
            "ProgramW6432",
            "INCLUDE",
            "LIB",
            "LIBPATH",
            "VCINSTALLDIR",
            "VCToolsInstallDir",
            "WindowsSdkDir",
            "WindowsSDKVersion",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    request.runtime_limit = timeout;
    request.resources.max_output_bytes_per_stream = 256 * 1024;
    request = lokai_sandbox::apply_executable(request, &program, &args);
    let result = backend.execute(request).await.map_err(|e| match e {
        SandboxError::Timeout => "grader_timeout",
        _ => "grader_execution_error",
    })?;
    match result {
        SandboxedProcess::Completed(result) => {
            tracing::debug!(sandbox = %result.report.audit_summary, "Grader execution policy");
            if result.truncated {
                return Err("grader_output_limit");
            }
            Ok(Outcome {
                success: result.success,
                output: format!("{}\n{}", result.stdout, result.stderr),
            })
        }
        SandboxedProcess::Service(_) => Err("grader_unexpected_service"),
    }
}

#[derive(Default)]
struct Counts {
    passed: u64,
    failed: u64,
    skipped: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    lokai_test_report: u32,
    passed: u32,
    failed: u32,
    skipped: u32,
}

fn counts(output: &str) -> Option<Counts> {
    let mut total = Counts::default();
    let mut summaries = 0;
    let mut json_report = false;
    for line in output.lines().map(str::trim) {
        if line.starts_with('{') && line.contains("lokai_test_report") {
            // One structured report, or Cargo summaries; never both or duplicates.
            if summaries != 0 {
                return None;
            }
            let report: Report = serde_json::from_str(line).ok()?;
            if report.lokai_test_report != 1 {
                return None;
            }
            total = Counts {
                passed: report.passed.into(),
                failed: report.failed.into(),
                skipped: report.skipped.into(),
            };
            summaries += 1;
            json_report = true;
        } else if let Some(rest) = line.strip_prefix("test result: ") {
            if json_report {
                return None;
            }
            let reported_ok = rest.starts_with("ok. ");
            let rest = rest
                .strip_prefix("ok. ")
                .or_else(|| rest.strip_prefix("FAILED. "))?;
            let fields: Vec<_> = rest.split(';').collect();
            let number = |i: usize, suffix: &str| -> Option<u64> {
                fields.get(i)?.trim().strip_suffix(suffix)?.parse().ok()
            };
            total.passed = total.passed.checked_add(number(0, " passed")?)?;
            let failed = number(1, " failed")?;
            if reported_ok != (failed == 0) {
                return None;
            }
            total.failed = total.failed.checked_add(failed)?;
            total.skipped = total.skipped.checked_add(number(2, " ignored")?)?;
            summaries += 1;
        }
    }
    (summaries > 0).then_some(total)
}

pub(super) fn mutation_verdict(outcome: Result<Outcome, &'static str>) -> Option<&'static str> {
    let outcome = match outcome {
        Ok(value) => value,
        Err(reason) => return Some(reason),
    };
    if outcome.success {
        return Some("mutation_survived");
    }
    match counts(&outcome.output) {
        Some(counts) if counts.failed > 0 && counts.skipped == 0 => None,
        _ => Some("mutation_failure_not_proven"),
    }
}

pub(super) fn test_verdict(
    output: Result<String, &'static str>,
    expected: Option<u32>,
    fail_if_skipped: bool,
) -> Option<&'static str> {
    let output = match output {
        Ok(output) => output,
        Err(reason) => return Some(reason),
    };
    let Some(counts) = counts(&output) else {
        return Some("test_evidence_missing_or_invalid");
    };
    if counts.failed > 0 {
        return Some("tests_failed");
    }
    if fail_if_skipped && counts.skipped > 0 {
        return Some("tests_skipped");
    }
    if counts.passed < u64::from(expected.unwrap_or(1).max(1)) {
        return Some("insufficient_tests");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_requires_observed_test_failure_not_any_process_error() {
        for (success, output, expected) in [
            (false, "compiler error", Some("mutation_failure_not_proven")),
            (
                false,
                "test result: ok. 0 passed; 1 failed; 0 ignored;",
                Some("mutation_failure_not_proven"),
            ),
            (
                false,
                "test result: FAILED. 0 passed; 1 failed; 1 ignored;",
                Some("mutation_failure_not_proven"),
            ),
            (
                true,
                "test result: ok. 1 passed; 0 failed; 0 ignored;",
                Some("mutation_survived"),
            ),
            (
                false,
                "test result: FAILED. 0 passed; 1 failed; 0 ignored;",
                None,
            ),
        ] {
            assert_eq!(
                mutation_verdict(Ok(Outcome {
                    success,
                    output: output.into()
                })),
                expected
            );
        }
        assert_eq!(
            mutation_verdict(Err("grader_timeout")),
            Some("grader_timeout")
        );
    }

    #[test]
    fn test_counts_fail_closed() {
        for output in [
            "ok",
            "",
            "test result: ok. 0 passed; 0 failed; 0 ignored;",
            "test result: ok. 1 passed; 1 failed; 0 ignored;",
            "test result: ok. 1 passed; 0 failed; 1 ignored;",
            r#"{"lokai_test_report":1,"passed":1}"#,
        ] {
            assert!(
                test_verdict(Ok(output.into()), Some(1), true).is_some(),
                "{output}"
            );
        }
        let ok = "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured;\ntest result: ok. 0 passed; 0 failed; 0 ignored;";
        assert_eq!(test_verdict(Ok(ok.into()), Some(2), true), None);
        assert_eq!(
            test_verdict(Ok(ok.into()), Some(3), true),
            Some("insufficient_tests")
        );
        let json = r#"{"lokai_test_report":1,"passed":2,"failed":0,"skipped":0}"#;
        assert_eq!(test_verdict(Ok(json.into()), Some(2), true), None);
        assert!(counts(&format!("{json}\n{json}")).is_none());
        assert!(counts(&format!("{json}\n{ok}")).is_none());
        assert_eq!(
            test_verdict(Err("grader_timeout"), None, false),
            Some("grader_timeout")
        );
    }

    #[tokio::test]
    async fn commands_are_bounded_and_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        for (script, expected) in [
            ("import time\ntime.sleep(10)\n", "grader_timeout"),
            ("print('x' * 600000)\n", "grader_output_limit"),
            ("raise SystemExit(1)\n", "grader_command_failed"),
        ] {
            std::fs::write(dir.path().join("grade.py"), script).unwrap();
            let result = run(dir.path(), "python grade.py", Duration::from_secs(1)).await;
            assert_eq!(result.unwrap_err(), expected);
        }
        assert_eq!(
            run(dir.path(), "", Duration::from_secs(1))
                .await
                .unwrap_err(),
            "grader_invalid_command"
        );
        assert_eq!(
            run(dir.path(), "echo ok", Duration::from_secs(1))
                .await
                .unwrap_err(),
            "grader_invalid_command"
        );
    }
}
