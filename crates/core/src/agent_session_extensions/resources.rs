use super::types::{
    ExtensionsResult, PromptTemplateInfo, ResourceExtensionPaths, SkillCatalog,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ResourceCatalogState {
    skills: SkillCatalog,
    prompts: Vec<PromptTemplateInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLoaderState {
    pub extension_paths: ResourceExtensionPaths,
    pub extensions_result: ExtensionsResult,
    catalogs: ResourceCatalogState,
}

impl ResourceLoaderState {
    pub fn extend_resources(&mut self, paths: ResourceExtensionPaths) {
        self.extension_paths.extend_owned(paths);
    }

    pub fn reload(&mut self) {
        self.extension_paths.clear();
    }

    pub fn get_extensions(&self) -> ExtensionsResult {
        self.extensions_result.clone()
    }

    pub fn get_skills(&self) -> SkillCatalog {
        self.catalogs.skills.clone()
    }

    pub fn set_skills(&mut self, skills: SkillCatalog) {
        self.catalogs.skills = skills;
    }

    pub fn get_prompts(&self) -> Vec<PromptTemplateInfo> {
        self.catalogs.prompts.clone()
    }

    pub fn set_prompts(&mut self, prompts: Vec<PromptTemplateInfo>) {
        self.catalogs.prompts = prompts;
    }

    pub fn has_extension_paths(&self) -> bool {
        !self.extension_paths.is_empty()
    }

    pub fn has_catalog_entries(&self) -> bool {
        !self.catalogs.skills.is_empty() || !self.catalogs.prompts.is_empty()
    }
}
