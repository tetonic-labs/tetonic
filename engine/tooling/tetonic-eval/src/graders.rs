use crate::manifest::GraderType;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

mod command;
mod mutation;
mod paths;
mod protected;

pub struct GraderContext {
    pub output_digest: Option<String>,
    pub touched_files: Vec<String>,
    pub found_secrets: bool,
    pub workspace: PathBuf,
}

pub fn patch_digest(files: &BTreeMap<String, Vec<u8>>, touched: &[String]) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut paths = touched.to_vec();
    paths.sort();
    for p in paths {
        hasher.update(p.as_bytes());
        if let Some(bytes) = files.get(&p) {
            hasher.update(bytes);
        }
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

pub fn outbound_contains_secrets(texts: &[String]) -> bool {
    let engine = lokai_secrets::scanner::ScannerEngine::default_engine();
    for text in texts {
        if text.is_empty() {
            continue;
        }
        match engine.scan_and_redact_sync(text, None) {
            Ok(Some((records, _))) if !records.is_empty() => return true,
            _ => {}
        }
    }
    false
}

pub async fn evaluate_graders(graders: &[GraderType], context: &GraderContext) -> Result<bool> {
    Ok(grade(graders, context).await?.is_none())
}

/// None means passed; otherwise a stable reason why qualification is blocked.
pub async fn grade(
    graders: &[GraderType],
    context: &GraderContext,
) -> Result<Option<&'static str>> {
    if graders.is_empty() {
        return Ok(Some("no_graders"));
    }
    if graders
        .iter()
        .any(|g| matches!(g, GraderType::ManualReview { .. }))
    {
        return Ok(Some("manual_review_required"));
    }
    if let Some(reason) = protected::check(graders, &context.workspace) {
        return Ok(Some(reason));
    }
    // Validate mutation boundaries before executing candidate build configuration,
    // regardless of the manifest's display order.
    for grader in graders {
        if let GraderType::FileBoundary {
            allowed_paths,
            prohibited_paths,
        } = grader
        {
            if let Some(reason) =
                paths::grade(&context.touched_files, allowed_paths, prohibited_paths)
            {
                return Ok(Some(reason));
            }
        }
    }
    for grader in graders {
        match grader {
            GraderType::ExactMatch {
                expected_patch_digest,
            } => {
                let actual = context.output_digest.as_deref().unwrap_or("");
                if actual != expected_patch_digest {
                    tracing::error!(
                        "ExactMatch grader failed: expected {}, got {}",
                        expected_patch_digest,
                        actual
                    );
                    return Ok(Some("grader_failed"));
                }
            }
            GraderType::TestExecution {
                command,
                fail_if_skipped,
                expected_pass_count,
                ..
            } => {
                let outcome =
                    command::run(&context.workspace, command, Duration::from_secs(60)).await;
                if let Some(reason) = protected::check(graders, &context.workspace) {
                    return Ok(Some(reason));
                }
                if let Some(reason) =
                    command::test_verdict(outcome, *expected_pass_count, *fail_if_skipped)
                {
                    return Ok(Some(reason));
                }
            }
            GraderType::CommandExecution { command } => {
                let outcome =
                    command::run(&context.workspace, command, Duration::from_secs(60)).await;
                if let Some(reason) = protected::check(graders, &context.workspace) {
                    return Ok(Some(reason));
                }
                if let Err(reason) = outcome {
                    return Ok(Some(reason));
                }
            }
            GraderType::ProtectedFiles { .. } => {}
            GraderType::MutationTest {
                command,
                input_files,
                source_path,
                mutant_source,
            } => {
                if let Some(reason) = mutation::grade(
                    graders,
                    context,
                    command,
                    input_files,
                    source_path,
                    mutant_source,
                )
                .await
                {
                    return Ok(Some(reason));
                }
            }
            GraderType::FileBoundary { .. } => {}
            GraderType::SecurityCheck { require_no_secrets } => {
                if *require_no_secrets && context.found_secrets {
                    tracing::error!("SecurityCheck grader failed: secrets reached the model.");
                    return Ok(Some("grader_failed"));
                }
            }
            GraderType::ManualReview { .. } => return Ok(Some("manual_review_required")),
        }
    }
    Ok(None)
}
