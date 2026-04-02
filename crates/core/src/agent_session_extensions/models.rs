use std::collections::BTreeMap;

use super::types::{ModelDescriptor, ProviderConfig};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRegistryState {
    pub models: BTreeMap<(String, String), ModelDescriptor>,
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl ModelRegistryState {
    pub fn find(&self, provider: &str, id: &str) -> Option<ModelDescriptor> {
        self.models
            .get(&(provider.to_owned(), id.to_owned()))
            .cloned()
    }

    pub fn register_provider(&mut self, name: impl Into<String>, config: ProviderConfig) {
        self.providers.insert(name.into(), config);
    }

    pub fn unregister_provider(&mut self, name: &str) {
        self.providers.remove(name);
    }
}
