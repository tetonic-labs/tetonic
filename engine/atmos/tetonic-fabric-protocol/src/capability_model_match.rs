//! Model placement matching using capability inventory (M5-2).

use crate::capability_document::ModelCapability;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSelection {
    pub local_name: String,
    pub digest: Option<String>,
}

impl ModelSelection {
    pub fn from_request(model: &str, digest: Option<&str>) -> Self {
        Self {
            local_name: model.to_string(),
            digest: digest.map(String::from),
        }
    }
}

/// Returns true when the worker inventory can satisfy the requested model.
///
/// When a digest is requested, only an exact digest match is accepted.
/// When the tag is mutable (`:latest`, `:main`) and inventory entries carry
/// digests, tag-only selection is rejected.
pub fn model_inventory_matches(inventory: &[ModelCapability], selection: &ModelSelection) -> bool {
    let candidates: Vec<_> = inventory
        .iter()
        .filter(|m| tag_matches(&m.local_name, &selection.local_name))
        .collect();
    if candidates.is_empty() {
        return false;
    }
    if let Some(req_digest) = selection.digest.as_deref() {
        return candidates
            .iter()
            .any(|m| m.model_digest.as_deref() == Some(req_digest));
    }
    if is_mutable_tag(&selection.local_name) && candidates.iter().any(|m| m.model_digest.is_some())
    {
        return false;
    }
    true
}

pub fn is_mutable_tag(model: &str) -> bool {
    model.ends_with(":latest") || model.ends_with(":main") || model == "latest" || model == "main"
}

fn tag_matches(inventory_name: &str, requested: &str) -> bool {
    inventory_name == requested || inventory_name.starts_with(&format!("{requested}:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_document::{ModelCapability, ModelLoadState};

    fn model(name: &str, digest: Option<&str>) -> ModelCapability {
        ModelCapability {
            local_name: name.into(),
            model_digest: digest.map(String::from),
            parameter_size_b: None,
            max_context_length: 8192,
            tool_call_support: false,
            structured_output_support: false,
            estimated_vram_bytes: None,
            quantization: None,
            load_state: ModelLoadState::Unknown,
        }
    }

    #[test]
    fn digest_required_when_requested() {
        let inv = vec![model("qwen:7b", Some("sha256:aaa"))];
        assert!(model_inventory_matches(
            &inv,
            &ModelSelection::from_request("qwen:7b", Some("sha256:aaa"))
        ));
        assert!(!model_inventory_matches(
            &inv,
            &ModelSelection::from_request("qwen:7b", Some("sha256:bbb"))
        ));
    }

    #[test]
    fn mutable_tag_rejected_when_digest_known() {
        let inv = vec![model("qwen:latest", Some("sha256:aaa"))];
        assert!(!model_inventory_matches(
            &inv,
            &ModelSelection::from_request("qwen:latest", None)
        ));
    }

    #[test]
    fn exact_tag_ok_without_request_digest() {
        let inv = vec![model("qwen:7b", Some("sha256:aaa"))];
        assert!(model_inventory_matches(
            &inv,
            &ModelSelection::from_request("qwen:7b", None)
        ));
    }
}
