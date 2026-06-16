use std::collections::BTreeMap;

use reqwest::Client;

use crate::config::VoicemuxConfig;
use crate::providers::{build_provider_adapters, ProviderAdapter, ProviderError};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: VoicemuxConfig,
    pub providers: BTreeMap<String, ProviderAdapter>,
    pub client: Client,
}

impl AppState {
    pub fn new(config: VoicemuxConfig) -> Result<Self, ProviderError> {
        let providers = build_provider_adapters(&config)?;
        let client = Client::new();

        Ok(Self {
            config,
            providers,
            client,
        })
    }
}
