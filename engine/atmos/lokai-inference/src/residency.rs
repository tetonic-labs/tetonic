//! Ollama allocation admission. No prompt is sent to a CPU-offloaded runner.
use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, Weak};

#[derive(Default)]
pub(super) struct RuntimeAdmission {
    recovered: Option<AllocationKey>,
}

#[derive(Clone, PartialEq, Eq)]
struct AllocationKey {
    model: String,
    num_ctx: Option<u32>,
    draft_model: Option<String>,
    draft_count: Option<u32>,
}

// Bootstrap and compute-plane providers for the same runtime share admission.
// Hold this lease through generation: another local caller must not resize or
// unload the allocation between its placement check and its last token.
pub(super) fn runtime_admission(base: &str) -> Arc<tokio::sync::Mutex<RuntimeAdmission>> {
    type Admissions = HashMap<String, Weak<tokio::sync::Mutex<RuntimeAdmission>>>;
    static REGISTRY: OnceLock<Mutex<Admissions>> = OnceLock::new();
    let mut registry = REGISTRY.get_or_init(Default::default).lock().unwrap();
    registry.retain(|_, value| value.strong_count() > 0);
    let key = base.trim_end_matches('/');
    if let Some(existing) = registry.get(key).and_then(Weak::upgrade) {
        return existing;
    }
    let admission = Arc::new(tokio::sync::Mutex::new(RuntimeAdmission::default()));
    registry.insert(key.to_owned(), Arc::downgrade(&admission));
    admission
}

impl OllamaProvider {
    pub(super) async fn admit_allocation(
        &self,
        admission: &mut RuntimeAdmission,
        model: &str,
        options: &mut OllamaChatOptions,
    ) -> Result<(), InferenceError> {
        let key = AllocationKey {
            model: model.to_owned(),
            num_ctx: options.num_ctx,
            draft_model: options.draft_model.clone(),
            draft_count: options.draft_count,
        };
        if admission.recovered.as_ref() == Some(&key) {
            options.num_gpu = Some(-1);
            options.num_batch = Some(128);
        }
        self.invalidate_ps_cache().await;
        let ps = self.get_ps_cached().await?;
        let resident = ps["models"].as_array().unwrap().iter().find(|m| {
            m.get("name")
                .or_else(|| m.get("model"))
                .and_then(Value::as_str)
                .is_some_and(|name| ollama_model_matches(name, model))
        });
        let already_spilled =
            resident.is_some_and(|m| match (m["size"].as_u64(), m["size_vram"].as_u64()) {
                (Some(size), Some(vram)) => vram_spill_below_threshold(size, vram).is_some(),
                _ => false,
            });
        // /ps cannot prove all load options match (including model defaults and
        // draft settings). An empty load reuses a matching runner, or prepares
        // the exact allocation that chat will use, without processing a prompt.
        // An already-spilled runner is released before attempting another load.
        let mut loaded = already_spilled;
        for attempt in 0..2 {
            if !loaded {
                let response = self
                    .guard
                    .post_json(
                        &format!("{}/api/generate", self.base_url),
                        &json!({"model":model,"prompt":"","stream":false,"options":options}),
                        "inference:ollama:admit",
                    )
                    .await?;
                if response.get("error").is_some() || response["done"] != true {
                    return Err(InferenceError::Provider(format!(
                        "model allocation failed: {response}"
                    )));
                }
                self.invalidate_ps_cache().await;
            }
            match self.check_vram_spill(model).await {
                Ok(()) => return Ok(()),
                Err(error @ InferenceError::GpuSpillDetected { .. }) => {
                    // Release this rejected allocation, never unrelated runners.
                    // Automatic layer placement removes stale forced-offload
                    // recipes; a smaller prefill batch frees GPU workspace.
                    // Model, context, precision and output budget stay intact.
                    self.unload_model_inner(model).await?;
                    if attempt == 1 {
                        admission.recovered = None;
                        return Err(error);
                    }
                    options.num_gpu = Some(-1);
                    options.num_batch = Some(128);
                    admission.recovered = Some(key.clone());
                    loaded = false;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }
}
