use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionSelectModelParams {
    pub session_id: String,
    pub selection_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub provider_label: String,
    pub availability: String,
    pub current: bool,
    #[serde(default)]
    pub requires_auth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionModelsResult {
    pub revision: u64,
    pub models: Vec<ModelChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionInferenceParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionSetInferenceParams {
    pub session_id: String,
    pub profile: String,
    pub model_fast: String,
    pub model_hard: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionInferenceResult {
    pub profile: String,
    pub model_fast: String,
    pub model_hard: String,
    pub revision: u64,
    pub available_profiles: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn set_requires_revision_and_cannot_carry_provider_configuration() {
        let mut value =
            json!({"session_id":"s", "profile":"default", "model_fast":"m", "model_hard":"m"});
        assert!(serde_json::from_value::<SessionSetInferenceParams>(value.clone()).is_err());
        value["expected_revision"] = json!(0);
        assert!(serde_json::from_value::<SessionSetInferenceParams>(value.clone()).is_ok());
        value["endpoint"] = json!("https://unregistered.example");
        assert!(serde_json::from_value::<SessionSetInferenceParams>(value).is_err());
    }
}
