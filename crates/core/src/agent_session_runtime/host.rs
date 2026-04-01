use std::path::{Path, PathBuf};

use crate::agent_session::{AgentSession, AgentSessionConfig, ModelRef};

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
}

fn runtime_model_from_session_model(model: &ModelRef) -> RuntimeModelRef {
    RuntimeModelRef {
        provider: model.provider.clone(),
        id: model.id.clone(),
        context_window: 0,
    }
}

