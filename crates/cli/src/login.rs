mod cli;
mod providers;
mod resolver;
mod store;

use anyhow::Result;
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::registry::{Model, ModelRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::oauth::OAuthCredentials;

use providers::{
    get_provider_status, is_oauth_provider, known_providers, normalize_provider_for_model_selection,
};
use store::{
    AuthEntry, AuthProfile, load_auth, provider_storage_key, stored_auth_methods_for_store,
    stored_auth_profile_by_id, stored_auth_profile_for_method,
};

pub(crate) use cli::{handle_login, handle_logout, run_oauth_login, try_open_browser};
pub(crate) use providers::{
    ProviderAuthMethod, provider_api_key_variant, provider_auth_method, provider_display_name,
    provider_login_hint, provider_meta, provider_oauth_variant,
};
pub(crate) use resolver::{
    AuthSource, ProviderAuthOptionSummary, ResolvedProviderAuth, add_cached_github_copilot_models,
    auth_source, authenticated_model_candidates, available_model_for_provider,
    preferred_available_model_for_provider, preferred_startup_provider_and_model,
    provider_auth_option_summaries, provider_auth_status_summary, provider_model_selection_detail,
    resolve_provider_auth, resolve_provider_auth_choice,
};

#[cfg(test)]
pub(crate) use resolver::save_oauth_credentials;
pub(crate) use store::{
    active_auth_method, auth_path, configured_providers, github_copilot_api_base_url,
    github_copilot_cached_models, github_copilot_domain, github_copilot_runtime_headers,
    github_copilot_status, normalize_github_domain, remove_auth, save_api_key,
    save_github_copilot_config, set_active_auth_profile, stored_auth_methods, stored_auth_profiles,
};

#[cfg(test)]
pub(crate) fn auth_test_env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
