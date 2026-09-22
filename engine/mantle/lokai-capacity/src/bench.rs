//! Microbench report types (output of `infer_gate.py` and Rust probe).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::gates::GateVerdict;
use crate::profile::ObservedPlacement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchSuite {
    RawShort,
    RawLong,
    ToolPayload,
    AgentMicro,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SuiteResult {
    pub wall_s: f64,
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub eval_tokens: u32,
    #[serde(default)]
    pub prefill_tps: f64,
    #[serde(default)]
    pub decode_tps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BenchReport {
    pub schema_version: u32,
    pub model: String,
    pub ollama_base: String,
    pub suites: HashMap<BenchSuite, SuiteResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<ObservedPlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<GateVerdict>,
}

impl BenchReport {
    pub fn with_gates(mut self, verdict: GateVerdict) -> Self {
        self.gates = Some(verdict);
        self
    }
}
