use std::collections::BTreeMap;

use reqwest::Client;

use crate::config::VoicemuxConfig;
use crate::providers::{build_provider_adapters, ProviderAdapter, ProviderError};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: VoicemuxConfig,
    pub providers: BTreeMap<String, ProviderAdapter>,
    pub client: Client,
    pub giggy_client: Client,
}

impl AppState {
    pub fn new(config: VoicemuxConfig) -> Result<Self, ProviderError> {
        let providers = build_provider_adapters(&config)?;
        let client = Client::new();
        let giggy_client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Giggy HTTP client configuration should be valid");

        Ok(Self {
            config,
            providers,
            client,
            giggy_client,
        })
    }
}
