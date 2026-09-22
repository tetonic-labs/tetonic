//! Mutation trials operate on explicit bounded snapshots, never the live workspace.
use super::{command, protected, GraderContext};
use crate::manifest::GraderType;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    path::Path,
    time::Duration,
};

type Inputs = BTreeMap<String, Vec<u8>>;

pub(super) async fn grade(
    graders: &[GraderType],
    context: &GraderContext,
    command: &str,
    input_files: &[String],
    source_path: &str,
    mutant_source: &str,
) -> Option<&'static str> {
    run(
        graders,
        context,
        command,
        input_files,
        source_path,
        mutant_source,
    )
    .await
    .err()
}

async fn run(
    graders: &[GraderType],
    context: &GraderContext,
    command_text: &str,
    input_files: &[String],
    source_path: &str,
    mutant_source: &str,
) -> Result<(), &'static str> {
    if input_files.is_empty() || input_files.len() > 64 || mutant_source.len() > 16 * 1024 * 1024 {
        return Err("mutation_input_limit");
    }
    let source_path = source_path.replace('\\', "/");
    if !graders.iter().any(|g| matches!(g, GraderType::ProtectedFiles { sha256 } if sha256.keys().any(|p| p.replace('\\', "/") == source_path))) {
        return Err("mutation_source_unpinned");
    }
    let mut inputs = Inputs::new();
    let mut identities = HashSet::new();
    let mut total = 0;
    for path in input_files {
        let file = protected::open_input(&context.workspace, path)?;
        let path = path.replace('\\', "/");
        // Portable case-insensitive uniqueness avoids divergent copy semantics.
        if !identities.insert(path.to_lowercase()) {
            return Err("mutation_duplicate_input");
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "grader_input_read_error")?;
        total += bytes.len();
        if bytes.len() > 16 * 1024 * 1024 || total > 128 * 1024 * 1024 {
            return Err("mutation_input_limit");
        }
        inputs.insert(path, bytes);
    }
    let original = inputs.get(&source_path).ok_or("mutation_source_missing")?;
    if original == mutant_source.as_bytes() {
        return Err("mutation_unchanged_source");
    }
    let baseline = materialize(&inputs)?;
    if let Some(reason) = protected::check(graders, baseline.path()) {
        return Err(reason);
    }
    let original_pins = pins(&inputs);
    let outcome = command::run(baseline.path(), command_text, Duration::from_secs(60)).await;
    if let Some(reason) = protected::check(&original_pins, baseline.path()) {
        return Err(reason);
    }
    if let Some(reason) = command::test_verdict(outcome, Some(1), true) {
        return Err(reason);
    }
    // Start the mutant from the captured input bytes, not files/build artifacts
    // left by the baseline process. Submitted tests must be identical in both.
    inputs.insert(source_path, mutant_source.as_bytes().to_vec());
    let mutant = materialize(&inputs)?;
    let mutant_pins = pins(&inputs);
    let outcome = command::run_captured(mutant.path(), command_text, Duration::from_secs(60)).await;
    if let Some(reason) = protected::check(&mutant_pins, mutant.path()) {
        return Err(reason);
    }
    if let Some(reason) = command::mutation_verdict(outcome) {
        return Err(reason);
    }
    // Catch persistent changes to trusted live inputs during either trial.
    if let Some(reason) = protected::check(graders, &context.workspace) {
        return Err(reason);
    }
    Ok(())
}

fn pins(inputs: &Inputs) -> Vec<GraderType> {
    vec![GraderType::ProtectedFiles {
        sha256: inputs
            .iter()
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                )
            })
            .collect(),
    }]
}

fn materialize(inputs: &Inputs) -> Result<tempfile::TempDir, &'static str> {
    let temp = tempfile::tempdir().map_err(|_| "mutation_workspace_error")?;
    for (path, bytes) in inputs {
        let target = temp.path().join(Path::new(path));
        std::fs::create_dir_all(target.parent().ok_or("mutation_workspace_error")?)
            .map_err(|_| "mutation_workspace_error")?;
        std::fs::write(target, bytes).map_err(|_| "mutation_workspace_error")?;
    }
    Ok(temp)
}
