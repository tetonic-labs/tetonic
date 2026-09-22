use super::*;
use crate::client::ClientError;
use crate::microbench::ChatOnceResult;
use async_trait::async_trait;
use std::sync::Mutex;

struct Client {
    events: Mutex<Vec<String>>,
    fail_chat: bool,
    gpu_pct: f32,
}

impl Client {
    fn new(fail_chat: bool, gpu_pct: f32) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            fail_chat,
            gpu_pct,
        }
    }
    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
}

#[async_trait]
impl InferenceClient for Client {
    fn base_url(&self) -> &str {
        "http://localhost:11434"
    }
    async fn reachable(&self) -> bool {
        true
    }
    async fn version(&self) -> Option<String> {
        None
    }
    async fn list_models(&self) -> Result<Vec<String>, ClientError> {
        unreachable!()
    }
    async fn model_capabilities(&self, _: &str) -> Result<Vec<String>, ClientError> {
        unreachable!()
    }
    async fn model_info(&self, _: &str) -> Result<tetonic_inference::ModelInfo, ClientError> {
        unreachable!()
    }
    async fn create_model(&self, _: &str, _: &str) -> Result<(), ClientError> {
        self.record("create".into());
        Ok(())
    }
    async fn delete_model(&self, _: &str) -> Result<(), ClientError> {
        unreachable!()
    }
    async fn unload_model(&self, model: &str) -> Result<(), ClientError> {
        assert_eq!(model, BENCH_MODEL_TAG);
        self.record("unload".into());
        Ok(())
    }
    async fn chat_once(
        &self,
        model: &str,
        _: &str,
        _: u32,
        context: Option<u32>,
    ) -> Result<ChatOnceResult, ClientError> {
        assert_eq!(model, BENCH_MODEL_TAG);
        self.record(format!("chat:{}", context.unwrap()));
        if self.fail_chat {
            return Err(ClientError::Ollama("load failed".into()));
        }
        Ok(ChatOnceResult {
            wall_s: 1.0,
            prompt_tokens: 1,
            eval_tokens: 1,
            prefill_tps: 1.0,
            decode_tps: 1.0,
        })
    }
    async fn fetch_observed_placement(&self, model: &str) -> Option<ObservedPlacement> {
        assert_eq!(model, BENCH_MODEL_TAG);
        self.record("measure".into());
        Some(ObservedPlacement {
            gpu_processor_pct: Some(self.gpu_pct),
            processor_split: None,
            vram_used_mb: 100,
            resident_model: model.into(),
            load_wall_s: 0.0,
            measured_at: Utc::now(),
        })
    }
}

#[tokio::test]
async fn successful_full_offload_needs_only_one_probe_and_releases_runner() {
    let client = Client::new(false, 100.0);
    let mut probes = 0;
    let result = search_best_num_gpu(
        &client,
        "base",
        8192,
        &GatePolicy::default(),
        &AtomicBool::new(false),
        &mut |_| probes += 1,
    )
    .await;
    assert_eq!(result.unwrap().0, NUM_GPU_MAX);
    assert_eq!(probes, 1);
    assert_eq!(
        *client.events.lock().unwrap(),
        [
            "unload",
            "create",
            "chat:8192",
            "chat:8192",
            "measure",
            "unload"
        ]
    );
}

#[tokio::test]
async fn failed_warmup_still_releases_benchmark_runner() {
    let client = Client::new(true, 100.0);
    assert!(
        try_bench_config(&client, "base", 4096, 999, &GatePolicy::default())
            .await
            .is_none()
    );
    assert_eq!(
        *client.events.lock().unwrap(),
        ["unload", "create", "chat:4096", "unload"]
    );
}

#[tokio::test]
async fn failed_placement_is_not_a_feasible_candidate() {
    let client = Client::new(false, 10.0);
    assert!(
        try_bench_config(&client, "base", 4096, 999, &GatePolicy::default())
            .await
            .is_none()
    );
    assert_eq!(client.events.lock().unwrap().last().unwrap(), "unload");
}
