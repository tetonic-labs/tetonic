//! Apply inference recipes — Modelfile on disk + Ollama estate tags.

use std::path::{Path, PathBuf};

use crate::client::{ClientError, InferenceClient};
use crate::paths::modelfiles_dir;
use crate::profile::InferenceRecipe;

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("client: {0}")]
    Client(#[from] ClientError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("path: {0}")]
    Path(String),
}

/// Strip `:tag` and any stacked `-estate` suffixes so re-optimize is idempotent.
pub fn estate_base_stem(base_model: &str) -> String {
    let mut stem = base_model
        .split(':')
        .next()
        .unwrap_or(base_model)
        .trim()
        .to_string();
    while stem.ends_with("-estate") {
        stem.truncate(stem.len() - "-estate".len());
    }
    stem
}

pub fn estate_model_name(base_model: &str) -> String {
    format!("{}-estate", estate_base_stem(base_model))
}

pub fn is_estate_or_bench_tag(name: &str) -> bool {
    let tag = name.split(':').next().unwrap_or(name);
    tag == crate::job::BENCH_MODEL_TAG || tag.ends_with("-estate")
}

pub fn render_modelfile(recipe: &InferenceRecipe) -> String {
    let mut lines = vec![
        format!("FROM {}", recipe.base_model),
        format!("PARAMETER num_ctx {}", recipe.num_ctx),
    ];
    if let Some(n) = recipe.num_gpu {
        lines.push(format!("PARAMETER num_gpu {}", n));
    }
    lines.join("\n")
}

pub fn write_modelfile(recipe: &InferenceRecipe, profile_id: &str) -> Result<PathBuf, ApplyError> {
    let dir = modelfiles_dir().ok_or_else(|| ApplyError::Path("lokai data dir".into()))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{profile_id}.Modelfile"));
    std::fs::write(&path, render_modelfile(recipe))?;
    Ok(path)
}

pub async fn apply_recipe(
    client: &dyn InferenceClient,
    recipe: &InferenceRecipe,
    profile_id: &str,
) -> Result<PathBuf, ApplyError> {
    let path = write_modelfile(recipe, profile_id)?;
    let content = std::fs::read_to_string(&path)?;
    client.create_model(&recipe.estate_model, &content).await?;
    Ok(path)
}

pub async fn apply_bench_tag(
    client: &dyn InferenceClient,
    tag: &str,
    base_model: &str,
    num_ctx: u32,
    num_gpu: u32,
) -> Result<(), ApplyError> {
    let recipe = InferenceRecipe {
        base_model: base_model.to_string(),
        estate_model: tag.to_string(),
        num_ctx,
        num_gpu: Some(num_gpu),
        keep_alive: "5m".into(),
        modelfile_path: None,
        env_hints: vec![],
        draft_model: None,
        draft_count: None,
    };
    let mf = render_modelfile(&recipe);
    client.create_model(tag, &mf).await?;
    Ok(())
}

pub async fn ollama_delete(
    client: &dyn InferenceClient,
    model_name: &str,
) -> Result<(), ApplyError> {
    client.delete_model(model_name).await?;
    Ok(())
}

#[allow(dead_code)]
pub fn modelfile_path_display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estate_tag_is_idempotent_across_reoptimize() {
        assert_eq!(estate_model_name("qwen3.6:latest"), "qwen3.6-estate");
        assert_eq!(estate_model_name("qwen3.6-estate"), "qwen3.6-estate");
        assert_eq!(
            estate_model_name("qwen3.6-gpu-estate-estate-estate:latest"),
            "qwen3.6-gpu-estate"
        );
        assert!(is_estate_or_bench_tag("qwen3.6-estate:latest"));
        assert!(is_estate_or_bench_tag("lokai-bench-opt"));
        assert!(!is_estate_or_bench_tag("qwen3.5:latest"));
    }
}
