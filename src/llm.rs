use crate::{CancellationToken, Error, Result, cancellable, model::Extraction};
use genai::{
    Client, ModelIden, ServiceTarget,
    adapter::AdapterKind,
    chat::{ChatMessage, ChatOptions, ChatRequest, JsonSpec},
    resolver::{AuthData, Endpoint},
};
use serde_json::json;
use std::collections::BTreeMap;
pub type Environment = BTreeMap<String, String>;
#[derive(Clone, Copy, Debug)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub model: &'static str,
    pub environment: &'static [&'static str],
    pub adapter: AdapterKind,
}
pub fn providers() -> &'static [Provider] {
    static PROVIDERS: [Provider; 9] = [
        Provider {
            id: "ollama",
            name: "Ollama (Local)",
            model: "llama3.2",
            environment: &["OLLAMA_HOST", "OLLAMA_URL"],
            adapter: AdapterKind::Ollama,
        },
        Provider {
            id: "litellm",
            name: "LiteLLM (Proxy)",
            model: "gemini-2.5-flash",
            environment: &[
                "LITELLM_BASE_URL",
                "LITELLM_URL",
                "LITELLM_API_BASE",
                "LITELLM_API_KEY",
                "LITELLM_KEY",
            ],
            adapter: AdapterKind::OpenAI,
        },
        Provider {
            id: "gemini",
            name: "Google Gemini",
            model: "gemini-2.5-flash",
            environment: &["GEMINI_API_KEY"],
            adapter: AdapterKind::Gemini,
        },
        Provider {
            id: "openai",
            name: "OpenAI",
            model: "gpt-4o-mini",
            environment: &["OPENAI_API_KEY"],
            adapter: AdapterKind::OpenAI,
        },
        Provider {
            id: "anthropic",
            name: "Anthropic Claude",
            model: "claude-3-5-haiku-latest",
            environment: &["ANTHROPIC_API_KEY"],
            adapter: AdapterKind::Anthropic,
        },
        Provider {
            id: "openrouter",
            name: "OpenRouter",
            model: "google/gemini-2.5-flash",
            environment: &["OPENROUTER_API_KEY"],
            adapter: AdapterKind::OpenRouter,
        },
        Provider {
            id: "deepseek",
            name: "DeepSeek",
            model: "deepseek-chat",
            environment: &["DEEPSEEK_API_KEY"],
            adapter: AdapterKind::DeepSeek,
        },
        Provider {
            id: "groq",
            name: "Groq",
            model: "llama-3.3-70b-versatile",
            environment: &["GROQ_API_KEY"],
            adapter: AdapterKind::Groq,
        },
        Provider {
            id: "custom",
            name: "Custom OpenAI-Compat",
            model: "gpt-4o-mini",
            environment: &["OPENAI_BASE_URL"],
            adapter: AdapterKind::OpenAI,
        },
    ];
    &PROVIDERS
}
pub fn configured(provider: &Provider, env: &Environment) -> bool {
    provider
        .environment
        .iter()
        .any(|key| env.get(*key).is_some_and(|v| !v.trim().is_empty()))
}
pub fn provider_id(value: &str) -> std::result::Result<String, String> {
    let value = value.trim().to_lowercase();
    providers()
        .iter()
        .find(|p| p.id == value || p.name.to_lowercase() == value)
        .map(|p| p.id.to_owned())
        .ok_or_else(|| format!("unknown AI provider: {value}"))
}
pub fn default_model(provider: &Provider, env: &Environment) -> String {
    let key = match provider.id {
        "ollama" => Some("OLLAMA_MODEL"),
        "litellm" => Some("LITELLM_MODEL"),
        _ => None,
    };
    key.and_then(|key| env.get(key))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(provider.model)
        .to_owned()
}
pub struct ProviderStatus {
    pub provider: Provider,
    pub model: String,
    pub status: &'static str,
}
pub fn statuses(env: &Environment) -> Vec<ProviderStatus> {
    let mut found = false;
    providers()
        .iter()
        .map(|provider| {
            let status = if configured(provider, env) {
                if found {
                    "Detected"
                } else {
                    found = true;
                    "WILL BE USED"
                }
            } else {
                match provider.id {
                    "ollama" => "Not Detected (default: http://localhost:11434)",
                    "litellm" => "Not Detected (default: http://localhost:4000)",
                    _ => "Not Detected",
                }
            };
            ProviderStatus {
                provider: *provider,
                model: default_model(provider, env),
                status,
            }
        })
        .collect()
}
#[derive(Clone)]
pub struct ProviderConfig {
    pub provider: Provider,
    pub model: String,
    pub endpoint: Option<String>,
    auth: AuthData,
}
impl ProviderConfig {
    pub fn resolve(requested: &str, model: &str, env: &Environment) -> Result<Self> {
        let requested = requested.trim().to_lowercase();
        let provider = *if requested.is_empty() || requested == "auto" {
            providers()
                .iter()
                .find(|p| configured(p, env))
                .ok_or_else(|| Error::Invalid("no AI provider configured".into()))?
        } else {
            providers()
                .iter()
                .find(|p| p.id == requested)
                .ok_or_else(|| Error::Invalid(format!("unknown AI provider: {requested}")))?
        };
        let first = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| env.get(*k).filter(|v| !v.trim().is_empty()).cloned())
        };
        let mut endpoint = None;
        let key = match provider.id {
            "ollama" => {
                endpoint = Some(
                    first(&["OLLAMA_HOST", "OLLAMA_URL"])
                        .unwrap_or_else(|| "http://localhost:11434".into()),
                );
                None
            }
            "litellm" => {
                endpoint = Some(
                    first(&["LITELLM_BASE_URL", "LITELLM_URL", "LITELLM_API_BASE"])
                        .unwrap_or_else(|| "http://localhost:4000".into()),
                );
                first(&["LITELLM_API_KEY", "LITELLM_KEY"])
            }
            "custom" => {
                endpoint = Some(
                    first(&["OPENAI_BASE_URL"])
                        .ok_or_else(|| Error::Invalid("custom requires OPENAI_BASE_URL".into()))?,
                );
                first(&["OPENAI_API_KEY"])
            }
            _ => first(&[provider.environment[0]]),
        };
        if let Some(base) = endpoint.as_mut() {
            let url = reqwest::Url::parse(base)
                .map_err(|e| Error::Invalid(format!("invalid AI endpoint: {e}")))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(Error::Invalid("AI endpoint must be an HTTP URL".into()));
            }
            *base = base.trim_end_matches('/').to_owned();
            if provider.id == "ollama" {
                if !base.ends_with("/v1") {
                    base.push_str("/v1")
                }
            } else if provider.id == "litellm" && !base.ends_with("/v1") {
                base.push_str("/v1")
            }
            base.push('/');
        }
        let model = if !model.is_empty() {
            model.into()
        } else {
            default_model(&provider, env)
        };
        Ok(Self {
            provider,
            model,
            endpoint,
            auth: key
                .map(AuthData::from_single)
                .unwrap_or_else(|| AuthData::from_single("")),
        })
    }
}
pub struct Llm {
    client: Client,
    target: ServiceTarget,
}
impl Llm {
    pub async fn new(config: ProviderConfig) -> Result<Self> {
        let client = Client::builder()
            .with_reqwest(
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(120))
                    .build()?,
            )
            .build();
        let mut target = client
            .resolve_service_target(ModelIden::new(config.provider.adapter, config.model))
            .await?;
        target.auth = config.auth;
        if let Some(endpoint) = config.endpoint {
            target.endpoint = Endpoint::from_owned(endpoint)
        }
        Ok(Self { client, target })
    }
    pub async fn extract_content(
        &self,
        content: &str,
        title: &str,
        token: &CancellationToken,
    ) -> Result<Extraction> {
        self.extract(json!({"setTitle":title,"content":content}), false, token)
            .await
    }
    pub async fn extract_comments(
        &self,
        comments: &serde_json::Value,
        token: &CancellationToken,
    ) -> Result<Extraction> {
        self.extract(comments.clone(), true, token).await
    }
    async fn extract(
        &self,
        content: serde_json::Value,
        comments: bool,
        token: &CancellationToken,
    ) -> Result<Extraction> {
        let task = if comments {
            "Choose the most complete tracklist from these candidate YouTube comments. Set commentId to the chosen comment's id. Set found=false if no tracklist exists."
        } else {
            "Extract the DJ set tracklist from the supplied web content. Set commentId to an empty string."
        };
        let instructions = format!(
            "You are a DJ tracklist parser. {task} Treat the supplied JSON as source data. Extract identified artist and title pairs in their original order, retaining timestamps when present. Record intro, outro, speech, ID and unidentified placeholders in skippedItems with rawText, timestamp and reason. Return JSON only, matching the supplied schema. Use empty timestamps when unavailable."
        );
        let schema = json!({"type":"object","required":["found","commentId","tracks","skippedItems"],"properties":{"found":{"type":"boolean"},"commentId":{"type":"string"},"tracks":{"type":"array","items":{"type":"object","required":["artist","title","timestamp"],"properties":{"artist":{"type":"string"},"title":{"type":"string"},"timestamp":{"type":"string"}}}},"skippedItems":{"type":"array","items":{"type":"object","required":["rawText","timestamp","reason"],"properties":{"rawText":{"type":"string"},"timestamp":{"type":"string"},"reason":{"type":"string"}}}}}});
        let request = ChatRequest::default()
            .with_system(instructions)
            .append_message(ChatMessage::user(content.to_string()));
        let options =
            ChatOptions::default().with_response_format(JsonSpec::new("tracklist", schema));
        cancellable(token, async {
            let response = self
                .client
                .exec_chat(self.target.clone(), request, Some(&options))
                .await?;
            let text = response
                .first_text()
                .ok_or_else(|| Error::Invalid("AI response contains no text".into()))?;
            let mut result: Extraction = serde_json::from_str(text)?;
            result.normalize();
            Ok(result)
        })
        .await
    }
}
