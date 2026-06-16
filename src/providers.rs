use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{Modality, ProviderConfig, ProviderType, VoicemuxConfig};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderDescriptor {
    pub name: String,
    pub provider_type: ProviderType,
    pub supports_stt: bool,
    pub supports_tts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAdapter {
    OpenAi(OpenAiAdapter),
    DeepgramStt(NativeAdapter),
    ElevenlabsTts(NativeAdapter),
}

impl ProviderAdapter {
    pub fn from_config(name: &str, config: &ProviderConfig) -> Result<Self, ProviderError> {
        match config.provider_type {
            ProviderType::OpenaiAudio | ProviderType::OpenaiStt | ProviderType::OpenaiTts => {
                Ok(Self::OpenAi(OpenAiAdapter::from_config(name, config)?))
            }
            ProviderType::DeepgramStt => Ok(Self::DeepgramStt(NativeAdapter::new(
                name,
                ProviderType::DeepgramStt,
                config,
            ))),
            ProviderType::ElevenlabsTts => Ok(Self::ElevenlabsTts(NativeAdapter::new(
                name,
                ProviderType::ElevenlabsTts,
                config,
            ))),
        }
    }

    pub fn descriptor(&self) -> ProviderDescriptor {
        match self {
            Self::OpenAi(adapter) => adapter.descriptor(),
            Self::DeepgramStt(adapter) | Self::ElevenlabsTts(adapter) => adapter.descriptor(),
        }
    }

    pub fn supports_modality(&self, modality: Modality) -> bool {
        match self {
            Self::OpenAi(adapter) => adapter.supports_modality(modality),
            Self::DeepgramStt(adapter) | Self::ElevenlabsTts(adapter) => {
                adapter.supports_modality(modality)
            }
        }
    }

    pub fn openai_audio_speech_endpoint(&self) -> Result<OpenAiEndpoint, ProviderError> {
        match self {
            Self::OpenAi(adapter) if adapter.supports_modality(Modality::Tts) => {
                Ok(OpenAiEndpoint {
                    url: format!("{}/audio/speech", adapter.base_url),
                    authorization: adapter.authorization()?,
                })
            }
            Self::OpenAi(adapter) => Err(ProviderError::UnsupportedPassthrough {
                provider: adapter.name.clone(),
                endpoint: "/v1/audio/speech",
            }),
            Self::DeepgramStt(adapter) | Self::ElevenlabsTts(adapter) => {
                Err(ProviderError::UnsupportedPassthrough {
                    provider: adapter.name.clone(),
                    endpoint: "/v1/audio/speech",
                })
            }
        }
    }

    pub fn openai_audio_transcriptions_endpoint(&self) -> Result<OpenAiEndpoint, ProviderError> {
        match self {
            Self::OpenAi(adapter) if adapter.supports_modality(Modality::Stt) => {
                Ok(OpenAiEndpoint {
                    url: format!("{}/audio/transcriptions", adapter.base_url),
                    authorization: adapter.authorization()?,
                })
            }
            Self::OpenAi(adapter) => Err(ProviderError::UnsupportedPassthrough {
                provider: adapter.name.clone(),
                endpoint: "/v1/audio/transcriptions",
            }),
            Self::DeepgramStt(adapter) | Self::ElevenlabsTts(adapter) => {
                Err(ProviderError::UnsupportedPassthrough {
                    provider: adapter.name.clone(),
                    endpoint: "/v1/audio/transcriptions",
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiEndpoint {
    pub url: String,
    pub authorization: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiAdapter {
    name: String,
    provider_type: ProviderType,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    tags: Vec<String>,
}

impl OpenAiAdapter {
    fn from_config(name: &str, config: &ProviderConfig) -> Result<Self, ProviderError> {
        let base_url = config
            .base_url
            .clone()
            .ok_or_else(|| ProviderError::MissingBaseUrl(name.to_string()))?;

        Ok(Self {
            name: name.to_string(),
            provider_type: config.provider_type.clone(),
            base_url: normalize_base_url(&base_url),
            api_key_env: config.api_key_env.clone(),
            api_key: config.api_key.clone(),
            tags: config.tags.clone(),
        })
    }

    fn authorization(&self) -> Result<Option<String>, ProviderError> {
        if let Some(api_key) = &self.api_key {
            return Ok(Some(format!("Bearer {api_key}")));
        }

        let Some(env_name) = &self.api_key_env else {
            return Ok(None);
        };

        std::env::var(env_name)
            .map(|api_key| Some(format!("Bearer {api_key}")))
            .map_err(|_| ProviderError::MissingApiKeyEnv {
                provider: self.name.clone(),
                env_name: env_name.clone(),
            })
    }

    fn supports_modality(&self, modality: Modality) -> bool {
        match modality {
            Modality::Stt => matches!(
                self.provider_type,
                ProviderType::OpenaiAudio | ProviderType::OpenaiStt
            ),
            Modality::Tts => matches!(
                self.provider_type,
                ProviderType::OpenaiAudio | ProviderType::OpenaiTts
            ),
        }
    }

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            name: self.name.clone(),
            provider_type: self.provider_type.clone(),
            supports_stt: self.supports_modality(Modality::Stt),
            supports_tts: self.supports_modality(Modality::Tts),
            base_url: Some(self.base_url.clone()),
            tags: self.tags.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAdapter {
    name: String,
    provider_type: ProviderType,
    api_key_env: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    language: Option<String>,
    output_format: Option<String>,
    smart_format: Option<bool>,
    punctuate: Option<bool>,
    tags: Vec<String>,
}

impl NativeAdapter {
    fn new(name: &str, provider_type: ProviderType, config: &ProviderConfig) -> Self {
        Self {
            name: name.to_string(),
            provider_type,
            api_key_env: config.api_key_env.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            language: config.language.clone(),
            output_format: config.output_format.clone(),
            smart_format: config.smart_format,
            punctuate: config.punctuate,
            tags: config.tags.clone(),
        }
    }

    pub fn api_key(&self) -> Result<String, ProviderError> {
        if let Some(api_key) = &self.api_key {
            return Ok(api_key.clone());
        }

        let Some(env_name) = &self.api_key_env else {
            return Err(ProviderError::MissingApiKey(self.name.clone()));
        };

        std::env::var(env_name).map_err(|_| ProviderError::MissingApiKeyEnv {
            provider: self.name.clone(),
            env_name: env_name.clone(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn output_format(&self) -> Option<&str> {
        self.output_format.as_deref()
    }

    pub fn smart_format(&self) -> Option<bool> {
        self.smart_format
    }

    pub fn punctuate(&self) -> Option<bool> {
        self.punctuate
    }

    fn supports_modality(&self, modality: Modality) -> bool {
        match modality {
            Modality::Stt => matches!(self.provider_type, ProviderType::DeepgramStt),
            Modality::Tts => matches!(self.provider_type, ProviderType::ElevenlabsTts),
        }
    }

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            name: self.name.clone(),
            provider_type: self.provider_type.clone(),
            supports_stt: self.supports_modality(Modality::Stt),
            supports_tts: self.supports_modality(Modality::Tts),
            base_url: None,
            tags: self.tags.clone(),
        }
    }
}

pub fn build_provider_adapters(
    config: &VoicemuxConfig,
) -> Result<BTreeMap<String, ProviderAdapter>, ProviderError> {
    config
        .providers
        .iter()
        .map(|(name, provider)| {
            ProviderAdapter::from_config(name, provider).map(|adapter| (name.clone(), adapter))
        })
        .collect()
}

pub fn provider_descriptors(
    config: &VoicemuxConfig,
) -> Result<Vec<ProviderDescriptor>, ProviderError> {
    Ok(build_provider_adapters(config)?
        .into_values()
        .map(|adapter| adapter.descriptor())
        .collect())
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider '{0}' requires base_url")]
    MissingBaseUrl(String),
    #[error("provider '{0}' requires api_key or api_key_env")]
    MissingApiKey(String),
    #[error("provider '{provider}' requires environment variable '{env_name}'")]
    MissingApiKeyEnv { provider: String, env_name: String },
    #[error("provider '{provider}' does not support passthrough endpoint '{endpoint}'")]
    UnsupportedPassthrough {
        provider: String,
        endpoint: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VoicemuxConfig;

    fn example_config() -> VoicemuxConfig {
        VoicemuxConfig::from_yaml(include_str!("../examples/voicemux.yaml"))
            .expect("example config should parse")
    }

    #[test]
    fn builds_adapters_from_example_config() {
        let config = example_config();
        let adapters = build_provider_adapters(&config).expect("adapters should build");

        assert_eq!(adapters.len(), config.providers.len());
        assert!(adapters["speaches"].supports_modality(Modality::Stt));
        assert!(adapters["speaches"].supports_modality(Modality::Tts));
        assert!(adapters["deepgram"].supports_modality(Modality::Stt));
        assert!(!adapters["deepgram"].supports_modality(Modality::Tts));
        assert!(!adapters["local_kokoro"].supports_modality(Modality::Stt));
        assert!(adapters["local_kokoro"].supports_modality(Modality::Tts));
    }

    #[test]
    fn returns_provider_descriptors() {
        let config = example_config();
        let descriptors = provider_descriptors(&config).expect("descriptors should build");
        let speaches = descriptors
            .iter()
            .find(|provider| provider.name == "speaches")
            .expect("speaches descriptor should exist");

        assert_eq!(speaches.provider_type, ProviderType::OpenaiAudio);
        assert!(speaches.supports_stt);
        assert!(speaches.supports_tts);
        assert_eq!(
            speaches.base_url.as_deref(),
            Some("http://127.0.0.1:8000/v1")
        );
    }

    #[test]
    fn rejects_openai_provider_without_base_url() {
        let yaml = r#"
active_profile: local
profiles:
  local:
    stt: [broken]
    tts: [broken]
providers:
  broken:
    type: openai_audio
"#;

        let config = VoicemuxConfig::from_yaml(yaml).expect("config shape should parse");
        let error = build_provider_adapters(&config).expect_err("adapter should fail");

        assert!(matches!(error, ProviderError::MissingBaseUrl(provider) if provider == "broken"));
    }

    #[test]
    fn builds_openai_speech_endpoint() {
        let config = example_config();
        let adapters = build_provider_adapters(&config).expect("adapters should build");
        let endpoint = adapters["local_kokoro"]
            .openai_audio_speech_endpoint()
            .expect("speech endpoint should build");

        assert_eq!(endpoint.url, "http://127.0.0.1:8880/v1/audio/speech");
        assert_eq!(
            endpoint.authorization.as_deref(),
            Some("Bearer not-needed-but-openai-sdks-require-a-value")
        );
    }

    #[test]
    fn rejects_native_speech_passthrough() {
        let config = example_config();
        let adapters = build_provider_adapters(&config).expect("adapters should build");
        let error = adapters["elevenlabs"]
            .openai_audio_speech_endpoint()
            .expect_err("native endpoint should not build yet");

        assert!(matches!(
            error,
            ProviderError::UnsupportedPassthrough { provider, endpoint }
                if provider == "elevenlabs" && endpoint == "/v1/audio/speech"
        ));
    }

    #[test]
    fn builds_openai_transcriptions_endpoint() {
        let config = example_config();
        let adapters = build_provider_adapters(&config).expect("adapters should build");
        let endpoint = adapters["local_whisper"]
            .openai_audio_transcriptions_endpoint()
            .expect("transcriptions endpoint should build");

        assert_eq!(
            endpoint.url,
            "http://127.0.0.1:2022/v1/audio/transcriptions"
        );
        assert_eq!(
            endpoint.authorization.as_deref(),
            Some("Bearer not-needed-but-openai-sdks-require-a-value")
        );
    }

    #[test]
    fn rejects_native_transcriptions_passthrough() {
        let config = example_config();
        let adapters = build_provider_adapters(&config).expect("adapters should build");
        let error = adapters["deepgram"]
            .openai_audio_transcriptions_endpoint()
            .expect_err("native endpoint should not build yet");

        assert!(matches!(
            error,
            ProviderError::UnsupportedPassthrough { provider, endpoint }
                if provider == "deepgram" && endpoint == "/v1/audio/transcriptions"
        ));
    }
}
