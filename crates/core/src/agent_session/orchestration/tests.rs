use super::super::super::agent_session::{AgentSessionConfig, PromptOptions};
use super::super::super::agent_session_extensions::{
    ExtensionsResult, PromptTemplateDefinition, PromptTemplateInfo, RegisteredCommand,
    SessionResourceBootstrap, SkillDefinition, SkillInfo, SourceInfo,
};
use super::super::error::AgentSessionError;
use super::super::session::AgentSession;

fn test_session() -> AgentSession {
    AgentSession::new(AgentSessionConfig {
        resource_bootstrap: SessionResourceBootstrap {
            extensions: ExtensionsResult {
                registered_commands: vec![RegisteredCommand {
                    invocation_name: "hello".to_string(),
                    description: "Say hello".to_string(),
                    source_info: SourceInfo {
                        path: "ext.js".to_string(),
                        source: "extension:test".to_string(),
                    },
                }],
                ..ExtensionsResult::default()
            },
            skills: vec![SkillDefinition {
                info: SkillInfo {
                    name: "review".to_string(),
                    description: "Review skill".to_string(),
                    source_info: SourceInfo {
                        path: "skill.md".to_string(),
                        source: "settings:test".to_string(),
                    },
                },
                content: "# Review\nUse the review workflow".to_string(),
            }],
            prompts: vec![PromptTemplateDefinition {
                info: PromptTemplateInfo {
                    name: "summarize".to_string(),
                    description: "Summarize content".to_string(),
                    source_info: SourceInfo {
                        path: "prompt.md".to_string(),
                        source: "settings:test".to_string(),
                    },
                },
                content: "Summarize the current state".to_string(),
            }],
        },
        ..AgentSessionConfig::default()
    })
}

#[test]
fn expands_skill_command_with_user_args() {
    let session = test_session();
    let expanded = session.expand_skill_command("/skill:review focus on tests".to_string());
    assert_eq!(
        expanded,
        "# Review\nUse the review workflow\n\nUser: focus on tests"
    );
}

#[test]
fn expands_prompt_template_with_user_args() {
    let session = test_session();
    let expanded = session.expand_prompt_template("/summarize pending changes".to_string());
    assert_eq!(
        expanded,
        "Summarize the current state\n\nUser: pending changes"
    );
}

#[test]
fn unknown_commands_are_left_unchanged() {
    let session = test_session();
    assert_eq!(
        session.expand_skill_command("/skill:missing test".to_string()),
        "/skill:missing test"
    );
    assert_eq!(
        session.expand_prompt_template("/missing test".to_string()),
        "/missing test"
    );
}

#[test]
fn registered_extension_commands_cannot_be_queued() {
    let mut session = test_session();
    session.state.model = Some(super::super::super::agent_session::ModelRef {
        provider: "test".to_string(),
        id: "model".to_string(),
        reasoning: false,
    });

    let result = session.prompt("/hello world", PromptOptions::default());
    assert_eq!(result, Ok(()));

    let steer_result = session.steer("/hello world", Vec::new());
    assert_eq!(
        steer_result,
        Err(AgentSessionError::ExtensionCommandCannotBeQueued)
    );
}

#[test]
fn prompt_templates_are_not_treated_as_extension_commands() {
    let mut session = test_session();
    session.state.model = Some(super::super::super::agent_session::ModelRef {
        provider: "test".to_string(),
        id: "model".to_string(),
        reasoning: false,
    });

    let result = session.prompt("/summarize release notes", PromptOptions::default());
    assert!(result.is_ok());
}
