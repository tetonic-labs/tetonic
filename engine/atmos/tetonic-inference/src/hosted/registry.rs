//! Provider registry and model auto-discovery.
use super::{HostedModelConfig, HostedWireProtocol, OutputLimitField};

#[derive(Debug, Clone)]
pub struct ProviderDescriptor {
    pub provider_id: &'static str,
    pub name: &'static str,
    pub default_endpoint: &'static str,
    pub protocol: HostedWireProtocol,
    pub env_keys: &'static [&'static str],
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    pub login_url: &'static str,
}

impl ProviderDescriptor {
    pub fn is_credential_configured(&self) -> bool {
        self.env_keys.iter().any(|key| {
            std::env::var(key)
                .map(|val| !val.trim().is_empty())
                .unwrap_or(false)
        })
    }

    pub fn get_configured_credential(&self) -> Option<String> {
        for key in self.env_keys {
            if let Ok(val) = std::env::var(key) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }
}

pub static PROVIDERS: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        provider_id: "anthropic",
        name: "Anthropic Claude",
        default_endpoint: "https://api.anthropic.com/v1/messages",
        protocol: HostedWireProtocol::AnthropicMessages,
        env_keys: &["ANTHROPIC_API_KEY", "CLAUDE_API_KEY"],
        default_model: "claude-3-5-sonnet-20241022",
        models: &[
            "claude-3-7-sonnet-20250219",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
            "claude-3-opus-20240229",
        ],
        login_url: "https://console.anthropic.com/settings/keys",
    },
    ProviderDescriptor {
        provider_id: "openai",
        name: "OpenAI",
        default_endpoint: "https://api.openai.com/v1/chat/completions",
        protocol: HostedWireProtocol::OpenAiChatCompletions,
        env_keys: &["OPENAI_API_KEY"],
        default_model: "gpt-4o",
        models: &["gpt-4o", "gpt-4o-mini", "o1", "o3-mini"],
        login_url: "https://platform.openai.com/api-keys",
    },
    ProviderDescriptor {
        provider_id: "deepseek",
        name: "DeepSeek",
        default_endpoint: "https://api.deepseek.com/v1/chat/completions",
        protocol: HostedWireProtocol::OpenAiChatCompletions,
        env_keys: &["DEEPSEEK_API_KEY"],
        default_model: "deepseek-coder",
        models: &["deepseek-chat", "deepseek-coder", "deepseek-reasoner"],
        login_url: "https://platform.deepseek.com/api_keys",
    },
];

pub fn resolve_provider_for_model(model: &str) -> Option<&'static ProviderDescriptor> {
    let lower = model.to_lowercase();
    if lower.starts_with("claude") {
        return Some(&PROVIDERS[0]);
    }
    if lower.starts_with("gpt") || lower.starts_with("o1") || lower.starts_with("o3") {
        return Some(&PROVIDERS[1]);
    }
    if lower.starts_with("deepseek") {
        return Some(&PROVIDERS[2]);
    }
    None
}

pub fn find_provider(provider_id: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS
        .iter()
        .find(|p| p.provider_id.eq_ignore_ascii_case(provider_id))
}

pub fn detect_active_cloud_provider() -> Option<(&'static ProviderDescriptor, String)> {
    for provider in PROVIDERS {
        for key_name in provider.env_keys {
            if let Ok(key_val) = std::env::var(key_name) {
                let trimmed = key_val.trim();
                if !trimmed.is_empty() {
                    return Some((provider, trimmed.to_string()));
                }
            }
        }
    }
    None
}

pub fn build_hosted_config(model: &str, max_output_tokens: Option<u32>) -> HostedModelConfig {
    let p = resolve_provider_for_model(model);
    let protocol = if let Some(provider) = p {
        provider.protocol
    } else {
        HostedWireProtocol::OpenAiChatCompletions
    };

    let max_tokens = max_output_tokens.unwrap_or(8192);

    let mut allowed_models: Vec<String> = if let Some(provider) = p {
        provider.models.iter().map(|m| m.to_string()).collect()
    } else {
        vec![]
    };
    if !allowed_models.iter().any(|m| m == model) {
        allowed_models.push(model.to_string());
    }

    HostedModelConfig {
        protocol,
        model: model.into(),
        allowed_models,
        max_output_tokens: max_tokens,
        output_limit_field: OutputLimitField::MaxTokens,
        supports_tools: true,
        supports_json_schema: protocol == HostedWireProtocol::OpenAiChatCompletions,
        send_temperature: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detects_claude_model() {
        let p = resolve_provider_for_model("claude-3-5-sonnet-20241022").unwrap();
        assert_eq!(p.provider_id, "anthropic");
        assert_eq!(p.protocol, HostedWireProtocol::AnthropicMessages);
        assert_eq!(p.default_endpoint, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn auto_detects_openai_model() {
        let p = resolve_provider_for_model("gpt-4o").unwrap();
        assert_eq!(p.provider_id, "openai");
        assert_eq!(p.protocol, HostedWireProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn auto_detects_deepseek_model() {
        let p = resolve_provider_for_model("deepseek-coder").unwrap();
        assert_eq!(p.provider_id, "deepseek");
        assert_eq!(p.protocol, HostedWireProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn descriptors_contain_catalog_models_and_login_urls() {
        for p in PROVIDERS {
            assert!(
                !p.models.is_empty(),
                "provider {} has empty models",
                p.provider_id
            );
            assert!(
                p.login_url.starts_with("https://"),
                "invalid login url for {}",
                p.provider_id
            );
            assert!(
                p.models.contains(&p.default_model),
                "default model missing in catalog"
            );
        }
    }
}
