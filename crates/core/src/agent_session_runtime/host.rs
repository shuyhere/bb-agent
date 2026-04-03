use std::path::{Path, PathBuf};

use crate::agent_session::{AgentSession, AgentSessionConfig, ModelRef};
use crate::agent_session_extensions::SessionResourceBootstrap;

use super::runtime::AgentSessionRuntime;
use super::types::{
    AgentSessionRuntimeBootstrap, CreateAgentSessionRuntimeOptions, RuntimeModelRef,
};

#[derive(Debug)]
pub struct AgentSessionRuntimeHandle {
    pub cwd: PathBuf,
    pub session: AgentSession,
    pub runtime: AgentSessionRuntime,
}

pub fn create_agent_session_runtime(
    bootstrap: &AgentSessionRuntimeBootstrap,
    options: CreateAgentSessionRuntimeOptions,
) -> AgentSessionRuntimeHandle {
    let session = AgentSession::new(AgentSessionConfig {
        cwd: options.cwd.clone(),
        scoped_models: bootstrap.scoped_models.clone(),
        initial_active_tool_names: bootstrap.initial_active_tool_names.clone(),
        resource_bootstrap: bootstrap.resource_bootstrap.clone(),
        session_start_event: options.session_start_event,
        model: bootstrap.model.clone(),
        thinking_level: bootstrap.thinking_level.unwrap_or_default(),
        ..AgentSessionConfig::default()
    });

    let mut runtime = AgentSessionRuntime::default();
    runtime.model = session
        .model()
        .map(|model| runtime_model_from_session_model(model));

    AgentSessionRuntimeHandle {
        cwd: options.cwd,
        session,
        runtime,
    }
}

pub struct AgentSessionRuntimeHost {
    bootstrap: AgentSessionRuntimeBootstrap,
    current: AgentSessionRuntimeHandle,
}

impl AgentSessionRuntimeHost {
    pub fn new(
        bootstrap: AgentSessionRuntimeBootstrap,
        current: AgentSessionRuntimeHandle,
    ) -> Self {
        Self { bootstrap, current }
    }

    pub fn from_bootstrap(bootstrap: AgentSessionRuntimeBootstrap) -> Self {
        let cwd = bootstrap
            .cwd
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let current =
            create_agent_session_runtime(&bootstrap, CreateAgentSessionRuntimeOptions::new(cwd));
        Self { bootstrap, current }
    }

    pub fn bootstrap(&self) -> &AgentSessionRuntimeBootstrap {
        &self.bootstrap
    }

    pub fn session(&self) -> &AgentSession {
        &self.current.session
    }

    pub fn session_mut(&mut self) -> &mut AgentSession {
        &mut self.current.session
    }

    pub fn runtime(&self) -> &AgentSessionRuntime {
        &self.current.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut AgentSessionRuntime {
        &mut self.current.runtime
    }

    pub fn cwd(&self) -> &Path {
        &self.current.cwd
    }

    pub fn reload_resources(&mut self, resource_bootstrap: SessionResourceBootstrap) {
        self.bootstrap.resource_bootstrap = resource_bootstrap;
        self.bootstrap.model = self.current.session.model().cloned();
        self.bootstrap.thinking_level = Some(self.current.session.thinking_level());
        let cwd = self.current.cwd.clone();
        let session_start_event = Some(crate::agent_session::SessionStartEvent {
            reason: "reload".to_string(),
        });
        self.current = create_agent_session_runtime(
            &self.bootstrap,
            CreateAgentSessionRuntimeOptions {
                cwd,
                session_start_event,
            },
        );
    }
}

fn runtime_model_from_session_model(model: &ModelRef) -> RuntimeModelRef {
    RuntimeModelRef {
        provider: model.provider.clone(),
        id: model.id.clone(),
        context_window: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_session::ThinkingLevel;
    use crate::agent_session_extensions::{
        PromptTemplateDefinition, PromptTemplateInfo, SessionResourceBootstrap, SourceInfo,
    };

    #[test]
    fn reload_resources_replaces_bootstrap_and_preserves_session_state() {
        let cwd = PathBuf::from("/tmp/runtime-host-reload");
        let mut host = AgentSessionRuntimeHost::from_bootstrap(AgentSessionRuntimeBootstrap {
            cwd: Some(cwd.clone()),
            model: Some(ModelRef {
                provider: "test".to_string(),
                id: "demo".to_string(),
                reasoning: true,
            }),
            thinking_level: Some(ThinkingLevel::High),
            resource_bootstrap: SessionResourceBootstrap {
                prompts: vec![PromptTemplateDefinition {
                    info: PromptTemplateInfo {
                        name: "before".to_string(),
                        description: "before reload".to_string(),
                        source_info: SourceInfo {
                            path: "before.md".to_string(),
                            source: "settings:test".to_string(),
                        },
                    },
                    content: "before".to_string(),
                }],
                ..SessionResourceBootstrap::default()
            },
            ..AgentSessionRuntimeBootstrap::default()
        });

        host.reload_resources(SessionResourceBootstrap {
            prompts: vec![PromptTemplateDefinition {
                info: PromptTemplateInfo {
                    name: "after".to_string(),
                    description: "after reload".to_string(),
                    source_info: SourceInfo {
                        path: "after.md".to_string(),
                        source: "settings:test".to_string(),
                    },
                },
                content: "after".to_string(),
            }],
            ..SessionResourceBootstrap::default()
        });

        assert_eq!(host.cwd(), cwd.as_path());
        assert_eq!(
            host.session().model().map(|model| model.id.as_str()),
            Some("demo")
        );
        assert_eq!(host.session().thinking_level(), ThinkingLevel::High);
        assert_eq!(host.bootstrap().resource_bootstrap.prompts.len(), 1);
        assert_eq!(
            host.bootstrap().resource_bootstrap.prompts[0].info.name,
            "after"
        );
        assert_eq!(host.session().expand_input_text("/after"), "after");
    }
}
