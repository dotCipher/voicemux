use serde::{Deserialize, Serialize};

use crate::config::{Modality, ProviderConfig, VoicemuxConfig};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RouteRequest {
    pub modality: Modality,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RoutePlan {
    pub profile: String,
    pub modality: Modality,
    pub route: Vec<String>,
    pub selected_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
}

pub fn plan_route(config: &VoicemuxConfig, request: RouteRequest) -> Result<RoutePlan, RouteError> {
    let profile_name = request
        .profile
        .clone()
        .unwrap_or_else(|| config.active_profile.clone());
    let profile = config
        .profiles
        .get(&profile_name)
        .ok_or_else(|| RouteError::UnknownProfile(profile_name.clone()))?;

    let chain = match request.modality {
        Modality::Stt => &profile.stt,
        Modality::Tts => &profile.tts,
    };

    let selected_provider = chain
        .first()
        .ok_or_else(|| RouteError::EmptyRouteChain {
            profile: profile_name.clone(),
            modality: request.modality,
        })?
        .clone();

    let provider = config
        .providers
        .get(&selected_provider)
        .ok_or_else(|| RouteError::UnknownProvider(selected_provider.clone()))?;

    if !provider.supports_modality(request.modality) {
        return Err(RouteError::ProviderDoesNotSupportModality {
            provider: selected_provider,
            modality: request.modality,
        });
    }

    let resolved_model = resolve_alias(
        request.model.as_deref(),
        &selected_provider,
        &config.aliases.models,
        provider.model.as_deref(),
    );
    let resolved_voice = resolve_alias(
        request.voice.as_deref(),
        &selected_provider,
        &config.aliases.voices,
        default_voice(provider),
    );

    Ok(RoutePlan {
        profile: profile_name,
        modality: request.modality,
        route: chain.clone(),
        selected_provider,
        resolved_model,
        resolved_voice,
        response_format: request.response_format,
    })
}

fn resolve_alias(
    requested: Option<&str>,
    provider_name: &str,
    aliases: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    provider_default: Option<&str>,
) -> Option<String> {
    let Some(requested) = requested else {
        return provider_default.map(ToOwned::to_owned);
    };

    aliases
        .get(requested)
        .and_then(|by_provider| by_provider.get(provider_name))
        .cloned()
        .or_else(|| Some(requested.to_string()))
}

fn default_voice(_provider: &ProviderConfig) -> Option<&str> {
    None
}

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("profile '{0}' is not defined")]
    UnknownProfile(String),
    #[error("profile '{profile}' has an empty {modality} route chain")]
    EmptyRouteChain { profile: String, modality: Modality },
    #[error("provider '{0}' is not defined")]
    UnknownProvider(String),
    #[error("provider '{provider}' does not support {modality}")]
    ProviderDoesNotSupportModality {
        provider: String,
        modality: Modality,
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
    fn plans_default_stt_route() {
        let config = example_config();
        let plan = plan_route(
            &config,
            RouteRequest {
                modality: Modality::Stt,
                profile: None,
                model: Some("whisper-1".to_string()),
                voice: None,
                response_format: None,
            },
        )
        .expect("route should plan");

        assert_eq!(plan.profile, "hybrid");
        assert_eq!(plan.route, vec!["deepgram", "speaches"]);
        assert_eq!(plan.selected_provider, "deepgram");
        assert_eq!(plan.resolved_model.as_deref(), Some("nova-3"));
        assert_eq!(plan.resolved_voice, None);
    }

    #[test]
    fn plans_default_tts_route_with_voice_alias() {
        let config = example_config();
        let plan = plan_route(
            &config,
            RouteRequest {
                modality: Modality::Tts,
                profile: None,
                model: Some("tts-1".to_string()),
                voice: Some("assistant".to_string()),
                response_format: Some("mp3".to_string()),
            },
        )
        .expect("route should plan");

        assert_eq!(plan.profile, "hybrid");
        assert_eq!(plan.route, vec!["elevenlabs", "speaches"]);
        assert_eq!(plan.selected_provider, "elevenlabs");
        assert_eq!(plan.resolved_model.as_deref(), Some("eleven_turbo_v2_5"));
        assert_eq!(
            plan.resolved_voice.as_deref(),
            Some("ELEVENLABS_VOICE_ID_HERE")
        );
        assert_eq!(plan.response_format.as_deref(), Some("mp3"));
    }

    #[test]
    fn plans_explicit_local_profile() {
        let config = example_config();
        let plan = plan_route(
            &config,
            RouteRequest {
                modality: Modality::Tts,
                profile: Some("local".to_string()),
                model: Some("tts-1".to_string()),
                voice: Some("assistant".to_string()),
                response_format: None,
            },
        )
        .expect("route should plan");

        assert_eq!(plan.route, vec!["speaches", "local_kokoro"]);
        assert_eq!(plan.selected_provider, "speaches");
        assert_eq!(
            plan.resolved_model.as_deref(),
            Some("speaches-ai/Kokoro-82M-v1.0-ONNX")
        );
        assert_eq!(plan.resolved_voice.as_deref(), Some("af_heart"));
    }

    #[test]
    fn rejects_unknown_profile() {
        let config = example_config();
        let error = plan_route(
            &config,
            RouteRequest {
                modality: Modality::Stt,
                profile: Some("missing".to_string()),
                model: None,
                voice: None,
                response_format: None,
            },
        )
        .expect_err("route should fail");

        assert!(matches!(error, RouteError::UnknownProfile(profile) if profile == "missing"));
    }

    #[test]
    fn uses_provider_model_default_when_request_omits_model() {
        let config = example_config();
        let plan = plan_route(
            &config,
            RouteRequest {
                modality: Modality::Stt,
                profile: None,
                model: None,
                voice: None,
                response_format: None,
            },
        )
        .expect("route should plan");

        assert_eq!(plan.selected_provider, "deepgram");
        assert_eq!(plan.resolved_model.as_deref(), Some("nova-3"));
    }
}
