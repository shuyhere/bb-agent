//! Credential resolution helpers split by concern.

use super::*;

mod auth_sources;
mod models;
mod oauth_refresh;

pub(crate) use auth_sources::{
    AuthSource, add_cached_github_copilot_models, auth_source, authenticated_providers,
    provider_auth_option_summaries, provider_auth_status_summary, provider_model_selection_detail,
};
pub(crate) use models::{
    authenticated_model_candidates, available_model_for_provider,
    preferred_available_model_for_provider, preferred_startup_provider_and_model,
};
pub(crate) use oauth_refresh::{
    ResolvedProviderAuth, resolve_provider_auth, resolve_provider_auth_choice,
    save_oauth_credentials,
};
