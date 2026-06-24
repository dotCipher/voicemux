use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VoicemuxConfig {
    pub active_profile: String,
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub aliases: AliasConfig,
    #[serde(default)]
    pub fallback: FallbackConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
}

impl VoicemuxConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path)?;
        Self::from_yaml(&contents)
    }

    pub fn from_yaml(contents: &str) -> Result<Self, ConfigError> {
        let config = serde_yaml::from_str::<Self>(contents)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if !self.profiles.contains_key(&self.active_profile) {
            return Err(ConfigError::UnknownActiveProfile(
                self.active_profile.clone(),
            ));
        }

        for (profile_name, profile) in &self.profiles {
            validate_route_chain(profile_name, Modality::Stt, &profile.stt, &self.providers)?;
            validate_route_chain(profile_name, Modality::Tts, &profile.tts, &self.providers)?;
        }

        Ok(())
    }
}

fn validate_route_chain(
    profile_name: &str,
    modality: Modality,
    chain: &[String],
    providers: &BTreeMap<String, ProviderConfig>,
) -> Result<(), ConfigError> {
    if chain.is_empty() {
        return Err(ConfigError::EmptyRouteChain {
            profile: profile_name.to_string(),
            modality,
        });
    }

    for provider_name in chain {
        let Some(provider) = providers.get(provider_name) else {
            return Err(ConfigError::UnknownProvider {
                profile: profile_name.to_string(),
                provider: provider_name.clone(),
            });
        };

        if !provider.supports_modality(modality) {
            return Err(ConfigError::ProviderDoesNotSupportModality {
                profile: profile_name.to_string(),
                provider: provider_name.clone(),
                modality,
            });
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Stt,
    Tts,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Tts => "tts",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProfileConfig {
    pub stt: Vec<String>,
    pub tts: Vec<String>,
    #[serde(default)]
    pub allow_cloud: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub smart_format: Option<bool>,
    #[serde(default)]
    pub punctuate: Option<bool>,
}

impl ProviderConfig {
    pub fn supports_modality(&self, modality: Modality) -> bool {
        match modality {
            Modality::Stt => matches!(
                self.provider_type,
                ProviderType::OpenaiAudio | ProviderType::OpenaiStt | ProviderType::DeepgramStt
            ),
            Modality::Tts => matches!(
                self.provider_type,
                ProviderType::OpenaiAudio | ProviderType::OpenaiTts | ProviderType::ElevenlabsTts
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    OpenaiAudio,
    OpenaiStt,
    OpenaiTts,
    DeepgramStt,
    ElevenlabsTts,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Stt,
    Tts,
    StreamingStt,
    StreamingTts,
    Translations,
    Voices,
    Models,
    Realtime,
    Local,
    Cloud,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct AliasConfig {
    #[serde(default)]
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub voices: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FallbackConfig {
    #[serde(default)]
    pub retry_timeouts: bool,
    #[serde(default)]
    pub fallback_on_statuses: Vec<u16>,
    #[serde(default = "default_max_attempts")]
    pub max_attempts_per_request: usize,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            retry_timeouts: false,
            fallback_on_statuses: vec![408, 429, 500, 502, 503, 504],
            max_attempts_per_request: default_max_attempts(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub include_provider_latency: bool,
    #[serde(default)]
    pub include_fallback_reason: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            include_provider_latency: true,
            include_fallback_reason: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            max_body_bytes: default_max_body_bytes(),
            request_timeout_seconds: default_request_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PerformanceConfig {
    #[serde(default = "default_true")]
    pub stream_tts_responses: bool,
    #[serde(default = "default_cache_provider_health_seconds")]
    pub cache_provider_health_seconds: u64,
    #[serde(default)]
    pub hot_reload_config: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            stream_tts_responses: true,
            cache_provider_health_seconds: default_cache_provider_health_seconds(),
            hot_reload_config: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("active profile '{0}' is not defined")]
    UnknownActiveProfile(String),
    #[error("profile '{profile}' has an empty {modality} route chain")]
    EmptyRouteChain { profile: String, modality: Modality },
    #[error("profile '{profile}' references unknown provider '{provider}'")]
    UnknownProvider { profile: String, provider: String },
    #[error(
        "profile '{profile}' references provider '{provider}' for unsupported {modality} route"
    )]
    ProviderDoesNotSupportModality {
        profile: String,
        provider: String,
        modality: Modality,
    },
}

impl std::fmt::Display for Modality {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn default_host() -> IpAddr {
    IpAddr::from([127, 0, 0, 1])
}

fn default_port() -> u16 {
    8787
}

fn default_max_body_bytes() -> usize {
    25 * 1024 * 1024
}

fn default_request_timeout_seconds() -> u64 {
    120
}

fn default_max_attempts() -> usize {
    2
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_cache_provider_health_seconds() -> u64 {
    10
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_config() {
        let config = VoicemuxConfig::from_yaml(include_str!("../examples/voicemux.yaml"))
            .expect("example config should parse");

        assert_eq!(config.active_profile, "hybrid");
        assert_eq!(config.server.port, 8787);
        assert_eq!(
            config.providers["speaches"].provider_type,
            ProviderType::OpenaiAudio
        );
        assert_eq!(
            config.providers["local_whisper"].capabilities,
            vec![Capability::Stt]
        );
        assert_eq!(
            config.profiles["hybrid"].stt,
            vec!["deepgram", "local_whisper"]
        );
        assert_eq!(config.aliases.voices["assistant"]["local_kokoro"], "af_sky");
    }

    #[test]
    fn rejects_unknown_active_profile() {
        let yaml = r#"
active_profile: missing
profiles:
  local:
    stt: [local_whisper]
    tts: [local_kokoro]
providers:
  local_whisper:
    type: openai_stt
  local_kokoro:
    type: openai_tts
"#;

        let error = VoicemuxConfig::from_yaml(yaml).expect_err("config should be invalid");
        assert!(
            matches!(error, ConfigError::UnknownActiveProfile(profile) if profile == "missing")
        );
    }

    #[test]
    fn rejects_unknown_provider_in_profile() {
        let yaml = r#"
active_profile: local
profiles:
  local:
    stt: [missing]
    tts: [local_kokoro]
providers:
  local_kokoro:
    type: openai_tts
"#;

        let error = VoicemuxConfig::from_yaml(yaml).expect_err("config should be invalid");
        assert!(
            matches!(error, ConfigError::UnknownProvider { profile, provider } if profile == "local" && provider == "missing")
        );
    }

    #[test]
    fn rejects_provider_with_wrong_modality() {
        let yaml = r#"
active_profile: local
profiles:
  local:
    stt: [local_kokoro]
    tts: [local_kokoro]
providers:
  local_kokoro:
    type: openai_tts
"#;

        let error = VoicemuxConfig::from_yaml(yaml).expect_err("config should be invalid");
        assert!(matches!(
            error,
            ConfigError::ProviderDoesNotSupportModality {
                profile,
                provider,
                modality: Modality::Stt,
            } if profile == "local" && provider == "local_kokoro"
        ));
    }
}
