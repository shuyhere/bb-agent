use std::fs;

use bb_core::agent_session_extensions::{
    PromptTemplateDefinition, PromptTemplateInfo, SkillDefinition, SkillInfo,
};
use bb_core::settings::PackageEntry;
use tempfile::tempdir;

use super::command_results::{
    parse_command_activate_agent_result, parse_command_dispatch_result, parse_command_invocation,
    parse_command_menu_result, parse_command_prompt_result, render_command_result,
};
use super::plugin_runtime::{build_plugin_runtime, map_tool_result};
use super::ui::ExtensionUiHandler;
use super::*;

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok()
}

#[test]
fn parses_frontmatter_name_and_description() {
    let metadata =
        parse_frontmatter("---\nname: demo-skill\ndescription: Helpful skill\n---\n# Demo");
    assert_eq!(metadata.get("name"), Some(&"demo-skill".to_string()));
    assert_eq!(
        metadata.get("description"),
        Some(&"Helpful skill".to_string())
    );
}

#[tokio::test]
async fn parses_command_invocation_and_args() {
    assert_eq!(
        parse_command_invocation("/hello world"),
        Some(("hello", Some("world")))
    );
    assert_eq!(parse_command_invocation("/hello"), Some(("hello", None)));
    assert_eq!(parse_command_invocation("hello"), None);
}

#[test]
fn input_hook_action_defaults_unknown_values_to_continue() {
    assert_eq!(
        InputHookAction::from_hook_action(Some("handled")),
        InputHookAction::Handled
    );
    assert_eq!(
        InputHookAction::from_hook_action(Some("continue")),
        InputHookAction::Continue
    );
    assert_eq!(
        InputHookAction::from_hook_action(Some("other")),
        InputHookAction::Continue
    );
    assert_eq!(
        InputHookAction::from_hook_action(None),
        InputHookAction::Continue
    );
}

#[test]
fn parses_extension_menu_result_with_items() {
    let value = serde_json::json!({
        "menu": {
            "title": "Shape",
            "items": [
                { "label": "New", "detail": "Make one", "value": "new" },
                { "label": "List", "value": "list" }
            ]
        }
    });
    let outcome = parse_command_menu_result("shape", &value).expect("menu");
    match outcome {
        ExtensionCommandOutcome::Menu {
            command,
            title,
            items,
        } => {
            assert_eq!(command, "shape");
            assert_eq!(title, "Shape");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].label, "New");
            assert_eq!(items[0].detail.as_deref(), Some("Make one"));
            assert_eq!(items[0].value, "new");
            assert_eq!(items[1].label, "List");
            assert_eq!(items[1].detail, None);
            assert_eq!(items[1].value, "list");
        }
        other => panic!("expected Menu, got {other:?}"),
    }
}

#[test]
fn parses_extension_prompt_result_with_resume_token() {
    let value = serde_json::json!({
        "prompt": {
            "title": "Shape — New Agent",
            "lines": ["Give me your resources."],
            "inputLabel": "Resources",
            "inputPlaceholder": "https://...",
            "resume": "opaque-token"
        }
    });
    let outcome = parse_command_prompt_result("shape", &value).expect("prompt");
    match outcome {
        ExtensionCommandOutcome::Prompt(prompt) => {
            assert_eq!(prompt.command, "shape");
            assert_eq!(prompt.title, "Shape — New Agent");
            assert_eq!(prompt.lines, vec!["Give me your resources."]);
            assert_eq!(prompt.input_label.as_deref(), Some("Resources"));
            assert_eq!(prompt.input_placeholder.as_deref(), Some("https://..."));
            assert_eq!(prompt.resume, "opaque-token");
        }
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[test]
fn parses_dispatch_and_activate_agent_results() {
    let short_dispatch = serde_json::json!({
        "dispatch": "  build the thing  ",
        "message": "Queued"
    });
    assert_eq!(
        parse_command_dispatch_result(&short_dispatch),
        Some(ExtensionCommandOutcome::Dispatch {
            note: Some("Queued".to_string()),
            prompt: "build the thing".to_string(),
        })
    );

    let activate = serde_json::json!({
        "activate_agent": {
            "agentId": "agent-123",
            "note": "Activated"
        }
    });
    assert_eq!(
        parse_command_activate_agent_result(&activate),
        Some(ExtensionCommandOutcome::ActivateAgent {
            agent_id: "agent-123".to_string(),
            note: Some("Activated".to_string()),
        })
    );
}

#[test]
fn non_menu_result_yields_text_or_nothing() {
    assert!(parse_command_menu_result("x", &serde_json::json!({"message": "hi"})).is_none());
    assert!(parse_command_menu_result("x", &serde_json::json!({"menu": {}})).is_none());
    assert!(parse_command_menu_result("x", &serde_json::json!({"menu": {"items": []}})).is_none());
    assert_eq!(
        render_command_result(&serde_json::json!({"message": "hello"})).as_deref(),
        Some("hello")
    );
}

#[test]
fn command_outcome_into_text_formats_non_tui_fallbacks() {
    let menu_text = ExtensionCommandOutcome::Menu {
        command: "shape".to_string(),
        title: "Shape".to_string(),
        items: vec![
            ExtensionMenuItem {
                label: "New".to_string(),
                detail: Some("Create one".to_string()),
                value: "new".to_string(),
            },
            ExtensionMenuItem {
                label: "List".to_string(),
                detail: None,
                value: "list".to_string(),
            },
        ],
    }
    .into_text()
    .unwrap();
    assert!(menu_text.contains("Shape"));
    assert!(menu_text.contains("1. New — Create one"));
    assert!(menu_text.contains("2. List"));

    let dispatch_text = ExtensionCommandOutcome::Dispatch {
        note: Some("Queued".to_string()),
        prompt: "Run build".to_string(),
    }
    .into_text()
    .unwrap();
    assert_eq!(dispatch_text, "Queued\nRun build");
}

#[test]
fn plugin_tool_result_mapping_preserves_blocks_and_flags() {
    let mapped = map_tool_result(serde_json::json!({
        "content": [
            { "type": "text", "text": "hello" },
            { "type": "image", "data": "aGVsbG8=", "mimeType": "image/png" }
        ],
        "details": { "exitCode": 0 },
        "is_error": true
    }))
    .unwrap();

    assert!(matches!(
        mapped.content.first(),
        Some(bb_core::types::ContentBlock::Text { text }) if text == "hello"
    ));
    assert!(matches!(
        mapped.content.get(1),
        Some(bb_core::types::ContentBlock::Image { mime_type, .. }) if mime_type == "image/png"
    ));
    assert_eq!(mapped.details, Some(serde_json::json!({ "exitCode": 0 })));
    assert!(mapped.is_error);
    assert_eq!(mapped.artifact_path, None);
}

#[test]
fn plugin_tool_result_mapping_falls_back_to_pretty_json_when_needed() {
    let mapped = map_tool_result(serde_json::json!({
        "details": { "status": "ok" },
        "unexpected": true
    }))
    .unwrap();

    assert!(matches!(
        mapped.content.first(),
        Some(bb_core::types::ContentBlock::Text { text }) if text.contains("\"unexpected\": true")
    ));
}

#[tokio::test]
async fn empty_plugin_runtime_returns_defaults() {
    let cwd = tempdir().unwrap();
    let (tools, commands, extensions) = build_plugin_runtime(cwd.path(), false, &[]).await.unwrap();

    assert!(tools.is_empty());
    assert!(!commands.is_registered("/anything"));
    assert!(extensions.extensions.is_empty());
    assert!(extensions.registered_commands.is_empty());
    assert!(extensions.registered_tools.is_empty());
}

#[test]
fn classifies_package_sources() {
    assert!(matches!(
        classify_package_source("npm:demo"),
        PackageSource::Npm(_)
    ));
    assert!(matches!(
        classify_package_source("git:https://x"),
        PackageSource::Git(_)
    ));
    assert!(matches!(
        classify_package_source("./local"),
        PackageSource::LocalPath(_)
    ));
}

#[test]
fn extension_bootstrap_splits_package_sources_from_paths() {
    let cwd = tempdir().unwrap();
    let bootstrap = ExtensionBootstrap::from_cli_values(
        cwd.path(),
        &[
            "npm:demo-skill".to_string(),
            "./local-ext".to_string(),
            "https://example.com/ext.tgz".to_string(),
        ],
    );

    assert_eq!(
        bootstrap.package_sources,
        vec![
            "npm:demo-skill".to_string(),
            "https://example.com/ext.tgz".to_string(),
        ]
    );
    assert_eq!(bootstrap.paths.len(), 1);
    assert!(bootstrap.paths[0].ends_with("local-ext"));
}

#[test]
fn discovers_package_resources_from_manifest() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("demo-package");
    fs::create_dir_all(package_dir.join("pkg-extensions")).unwrap();
    fs::create_dir_all(package_dir.join("pkg-skills")).unwrap();
    fs::create_dir_all(package_dir.join("pkg-prompts")).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{
                "name": "demo-package",
                "bb": {
                    "extensions": ["./pkg-extensions"],
                    "skills": ["./pkg-skills"],
                    "prompts": ["./pkg-prompts"]
                }
            }"#,
    )
    .unwrap();

    let resources = discover_package_resources(&package_dir, cwd.path()).unwrap();
    assert_eq!(
        resources.extensions,
        vec![normalize_path(package_dir.join("pkg-extensions"))]
    );
    assert_eq!(
        resources.skills,
        vec![normalize_path(package_dir.join("pkg-skills"))]
    );
    assert_eq!(
        resources.prompts,
        vec![normalize_path(package_dir.join("pkg-prompts"))]
    );
}

#[tokio::test]
async fn loads_package_skills_and_prompts_from_settings() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("skills-package");
    fs::create_dir_all(package_dir.join("skills/review")).unwrap();
    fs::create_dir_all(package_dir.join("prompts")).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{
                "name": "skills-package",
                "bb": {
                    "skills": ["./skills"],
                    "prompts": ["./prompts"]
                }
            }"#,
    )
    .unwrap();
    fs::write(
        package_dir.join("skills/review/SKILL.md"),
        "---\nname: package-review\ndescription: package review skill\n---\nReview carefully.",
    )
    .unwrap();
    fs::write(
        package_dir.join("prompts/summarize.md"),
        "Summarize the package state.",
    )
    .unwrap();

    let settings = Settings {
        packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
        ..Settings::default()
    };
    let support =
        load_runtime_extension_support(cwd.path(), &settings, &ExtensionBootstrap::default())
            .await
            .unwrap();

    assert!(
        support
            .session_resources
            .skills
            .iter()
            .any(|skill| skill.info.name == "package-review")
    );
    assert!(
        support
            .session_resources
            .prompts
            .iter()
            .any(|prompt| prompt.info.name == "summarize")
    );
}

#[tokio::test]
async fn disabled_skills_are_excluded_from_runtime_resources() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("skills-package");
    fs::create_dir_all(package_dir.join("skills/alpha")).unwrap();
    fs::create_dir_all(package_dir.join("skills/beta")).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{
                "name": "skills-package",
                "bb": { "skills": ["./skills"] }
            }"#,
    )
    .unwrap();
    fs::write(
        package_dir.join("skills/alpha/SKILL.md"),
        "---\nname: alpha\ndescription: alpha skill\n---\nBody.",
    )
    .unwrap();
    fs::write(
        package_dir.join("skills/beta/SKILL.md"),
        "---\nname: beta\ndescription: beta skill\n---\nBody.",
    )
    .unwrap();

    // Load with no disabled list first — both should be visible.
    let settings_all = Settings {
        packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
        ..Settings::default()
    };
    let support_all =
        load_runtime_extension_support(cwd.path(), &settings_all, &ExtensionBootstrap::default())
            .await
            .unwrap();
    let names_all: Vec<String> = support_all
        .session_resources
        .skills
        .iter()
        .map(|s| s.info.name.clone())
        .collect();
    assert!(names_all.iter().any(|n| n == "alpha"));
    assert!(names_all.iter().any(|n| n == "beta"));

    // Now disable `alpha` — source file is still on disk, but it must not
    // show up in the session resources.
    let settings_disabled = Settings {
        packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
        disabled_skills: vec!["alpha".to_string()],
        ..Settings::default()
    };
    let support_disabled = load_runtime_extension_support(
        cwd.path(),
        &settings_disabled,
        &ExtensionBootstrap::default(),
    )
    .await
    .unwrap();
    let names_disabled: Vec<String> = support_disabled
        .session_resources
        .skills
        .iter()
        .map(|s| s.info.name.clone())
        .collect();
    assert!(!names_disabled.iter().any(|n| n == "alpha"));
    assert!(names_disabled.iter().any(|n| n == "beta"));
    assert!(
        package_dir.join("skills/alpha/SKILL.md").exists(),
        "disable must not delete the source file"
    );
}

#[test]
fn project_scoped_package_settings_round_trip() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("local-package");
    fs::create_dir_all(&package_dir).unwrap();

    install_package(
        package_dir.to_str().unwrap(),
        SettingsScope::Project,
        cwd.path(),
    )
    .unwrap();

    let listed = list_packages(Some(SettingsScope::Project), cwd.path());
    assert_eq!(listed, vec![package_dir.display().to_string()]);

    let updated = update_packages(Some(SettingsScope::Project), cwd.path()).unwrap();
    assert_eq!(updated, vec![package_dir.display().to_string()]);

    assert!(
        remove_package(
            package_dir.to_str().unwrap(),
            SettingsScope::Project,
            cwd.path(),
        )
        .unwrap()
    );
    assert!(list_packages(Some(SettingsScope::Project), cwd.path()).is_empty());
}

#[test]
fn package_identity_controls_remove_and_listing() {
    let cwd = tempdir().unwrap();
    // Use Settings::merge to test package dedup
    let global = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".to_string())],
        ..Settings::default()
    };
    let project = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".to_string())],
        ..Settings::default()
    };
    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.packages.len(), 1);
    assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");

    let settings = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".to_string())],
        ..Settings::default()
    };
    settings.save_project(cwd.path()).unwrap();

    assert!(remove_package("npm:@demo/pkg", SettingsScope::Project, cwd.path()).unwrap());
    assert!(list_packages(Some(SettingsScope::Project), cwd.path()).is_empty());
}

#[test]
fn update_skips_pinned_package_sources() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("local-package");
    fs::create_dir_all(&package_dir).unwrap();

    let settings = Settings {
        packages: vec![
            PackageEntry::Simple(package_dir.display().to_string()),
            PackageEntry::Simple("npm:@demo/pinned@1.2.3".to_string()),
            PackageEntry::Simple("git:https://example.com/repo@v1".to_string()),
        ],
        ..Settings::default()
    };
    settings.save_project(cwd.path()).unwrap();

    let updated = update_packages(Some(SettingsScope::Project), cwd.path()).unwrap();
    assert!(updated.contains(&package_dir.display().to_string()));
    assert!(!updated.contains(&"npm:@demo/pinned@1.2.3".to_string()));
    assert!(!updated.contains(&"git:https://example.com/repo@v1".to_string()));
}

#[tokio::test]
async fn package_loaded_extension_command_executes_with_context() {
    if !node_available() {
        eprintln!("Skipping test: node not available");
        return;
    }

    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("command-package");
    fs::create_dir_all(package_dir.join("extensions")).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{
                "name": "command-package",
                "bb": {
                    "extensions": ["./extensions"]
                }
            }"#,
    )
    .unwrap();
    fs::write(
            package_dir.join("extensions/hello.js"),
            r#"
                module.exports = function(bb) {
                    bb.registerCommand('pkghello', {
                        description: 'package hello',
                        handler: async (args, ctx) => ({
                            message: [
                                `pkg:${args}`,
                                `ui:${ctx.hasUI}`,
                                `cwd:${ctx.cwd}`,
                                `entries:${ctx.sessionManager.getEntries().length}`,
                                `branch:${ctx.sessionManager.getBranch().length}`,
                                `leaf:${ctx.sessionManager.getLeafId()}`,
                                `label:${ctx.sessionManager.getLabel(ctx.sessionManager.getEntries()[0]?.id)}`,
                                `session:${ctx.sessionManager.getSessionId()}`,
                            ].join('|'),
                        }),
                    });
                };
            "#,
        )
        .unwrap();

    let conn = bb_session::store::open_db(&cwd.path().join("sessions.db")).unwrap();
    let session_id =
        bb_session::store::create_session(&conn, cwd.path().to_str().unwrap()).unwrap();
    let root = bb_core::types::SessionEntry::Message {
        base: bb_core::types::EntryBase {
            id: bb_core::types::EntryId::generate(),
            parent_id: None,
            timestamp: chrono::Utc::now(),
        },
        message: bb_core::types::AgentMessage::User(bb_core::types::UserMessage {
            content: vec![bb_core::types::ContentBlock::Text {
                text: "hello".to_string(),
            }],
            timestamp: chrono::Utc::now().timestamp_millis(),
        }),
    };
    let root_id = root.base().id.to_string();
    bb_session::store::append_entry(&conn, &session_id, &root).unwrap();
    let label = bb_core::types::SessionEntry::Label {
        base: bb_core::types::EntryBase {
            id: bb_core::types::EntryId::generate(),
            parent_id: Some(bb_core::types::EntryId(root_id.clone())),
            timestamp: chrono::Utc::now(),
        },
        target_id: bb_core::types::EntryId(root_id.clone()),
        label: Some("root-label".to_string()),
    };
    bb_session::store::append_entry(&conn, &session_id, &label).unwrap();

    let settings = Settings {
        packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
        ..Settings::default()
    };
    let mut support = load_runtime_extension_support_with_ui(
        cwd.path(),
        &settings,
        &ExtensionBootstrap::default(),
        true,
    )
    .await
    .unwrap();
    support.commands.bind_session_context(
        crate::turn_runner::open_sibling_conn(&conn).unwrap(),
        session_id.clone(),
        None,
    );

    assert!(support.commands.is_registered("/pkghello world"));
    let output = support
        .commands
        .execute_text("/pkghello world")
        .await
        .unwrap();
    let output = output.unwrap();
    assert!(output.contains("pkg:world"));
    assert!(output.contains("ui:true"));
    assert!(output.contains(cwd.path().to_str().unwrap()));
    assert!(output.contains("entries:2"));
    assert!(output.contains("branch:2"));
    assert!(output.contains(&format!("leaf:{}", label.base().id)));
    assert!(output.contains("label:root-label"));
    assert!(output.contains(&format!("session:{session_id}")));
}

#[tokio::test]
async fn extension_command_timeout_returns_error_instead_of_hanging() {
    if !node_available() {
        eprintln!("Skipping test: node not available");
        return;
    }

    let cwd = tempdir().unwrap();
    let extension_path = cwd.path().join("slow.js");
    fs::write(
        &extension_path,
        r#"
                module.exports = function(bb) {
                    bb.registerCommand('slow', {
                        description: 'slow command',
                        handler: async () => {
                            await new Promise((resolve) => setTimeout(resolve, 60000));
                            return { message: 'done' };
                        },
                    });
                };
            "#,
    )
    .unwrap();

    let support = load_runtime_extension_support_with_ui(
        cwd.path(),
        &Settings::default(),
        &ExtensionBootstrap {
            paths: vec![extension_path],
            package_sources: Vec::new(),
        },
        true,
    )
    .await
    .unwrap();

    let err = support
        .commands
        .execute_text_structured("/slow")
        .await
        .expect_err("slow extension command should time out");
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn reload_reloads_extension_command_output() {
    if !node_available() {
        eprintln!("Skipping test: node not available");
        return;
    }

    let cwd = tempdir().unwrap();
    let extension_path = cwd.path().join("reload.js");
    fs::write(
        &extension_path,
        r#"
                module.exports = function(bb) {
                    bb.registerCommand('hello', {
                        description: 'hello',
                        handler: async () => ({ message: 'v1' }),
                    });
                };
            "#,
    )
    .unwrap();

    let bootstrap = ExtensionBootstrap {
        paths: vec![extension_path.clone()],
        package_sources: Vec::new(),
    };
    let settings = Settings::default();
    let support_v1 = load_runtime_extension_support(cwd.path(), &settings, &bootstrap)
        .await
        .unwrap();
    assert_eq!(
        support_v1.commands.execute_text("/hello").await.unwrap(),
        Some("v1".to_string())
    );

    fs::write(
        &extension_path,
        r#"
                module.exports = function(bb) {
                    bb.registerCommand('hello', {
                        description: 'hello',
                        handler: async () => ({ message: 'v2' }),
                    });
                };
            "#,
    )
    .unwrap();

    let support_v2 = load_runtime_extension_support(cwd.path(), &settings, &bootstrap)
        .await
        .unwrap();
    assert_eq!(
        support_v2.commands.execute_text("/hello").await.unwrap(),
        Some("v2".to_string())
    );
}

#[test]
fn filter_matches_patterns() {
    let root = Path::new("/pkg");

    // None filter = include all
    assert!(filter_matches(Path::new("/pkg/ext/a.ts"), root, None));

    // Empty filter = include none
    assert!(!filter_matches(Path::new("/pkg/ext/a.ts"), root, Some(&[])));

    // Exact positive match
    assert!(filter_matches(
        Path::new("/pkg/ext/a.ts"),
        root,
        Some(&["ext/a.ts".to_string()])
    ));

    // No match
    assert!(!filter_matches(
        Path::new("/pkg/ext/b.ts"),
        root,
        Some(&["ext/a.ts".to_string()])
    ));

    // Glob exclusion
    assert!(!filter_matches(
        Path::new("/pkg/ext/legacy.ts"),
        root,
        Some(&["ext/*".to_string(), "!ext/legacy*".to_string()])
    ));

    // Force include overrides exclusion
    assert!(filter_matches(
        Path::new("/pkg/ext/legacy.ts"),
        root,
        Some(&["!ext/legacy*".to_string(), "+ext/legacy.ts".to_string()])
    ));

    // Force exclude
    assert!(!filter_matches(
        Path::new("/pkg/ext/a.ts"),
        root,
        Some(&["ext/*".to_string(), "-ext/a.ts".to_string()])
    ));

    // Glob: *.ts matches .ts files
    assert!(filter_matches(
        Path::new("/pkg/ext/a.ts"),
        root,
        Some(&["ext/*.ts".to_string()])
    ));

    // Glob: *.ts should NOT match .js files
    assert!(!filter_matches(
        Path::new("/pkg/ext/a.js"),
        root,
        Some(&["ext/*.ts".to_string()])
    ));

    // Glob: **/*.md matches nested .md files
    assert!(filter_matches(
        Path::new("/pkg/skills/review/SKILL.md"),
        root,
        Some(&["**/*.md".to_string()])
    ));

    // Glob: **/*.md matches top-level .md files too
    assert!(filter_matches(
        Path::new("/pkg/README.md"),
        root,
        Some(&["**/*.md".to_string()])
    ));

    // Glob: **/*.md should NOT match .ts files
    assert!(!filter_matches(
        Path::new("/pkg/ext/a.ts"),
        root,
        Some(&["**/*.md".to_string()])
    ));
}

#[tokio::test]
async fn filtered_package_loads_only_matching_resources() {
    let cwd = tempdir().unwrap();
    let package_dir = cwd.path().join("filtered-pkg");
    fs::create_dir_all(package_dir.join("skills/review")).unwrap();
    fs::create_dir_all(package_dir.join("skills/debug")).unwrap();
    fs::create_dir_all(package_dir.join("prompts")).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{
                "name": "filtered-pkg",
                "bb": {
                    "skills": ["./skills"],
                    "prompts": ["./prompts"]
                }
            }"#,
    )
    .unwrap();
    fs::write(
        package_dir.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: review skill\n---\nReview.",
    )
    .unwrap();
    fs::write(
        package_dir.join("skills/debug/SKILL.md"),
        "---\nname: debug\ndescription: debug skill\n---\nDebug.",
    )
    .unwrap();
    fs::write(package_dir.join("prompts/summarize.md"), "Summarize.").unwrap();
    fs::write(package_dir.join("prompts/fixtest.md"), "Fix tests.").unwrap();

    // Load with filter: only review skill, no prompts
    let settings = Settings {
        packages: vec![PackageEntry::Filtered(bb_core::settings::PackageFilter {
            source: package_dir.display().to_string(),
            extensions: None,
            skills: Some(vec!["**/review/**".to_string()]),
            prompts: Some(vec![]),
        })],
        ..Settings::default()
    };
    let support =
        load_runtime_extension_support(cwd.path(), &settings, &ExtensionBootstrap::default())
            .await
            .unwrap();

    // Only review skill should be loaded
    let skill_names: Vec<&str> = support
        .session_resources
        .skills
        .iter()
        .map(|s| s.info.name.as_str())
        .collect();
    assert!(skill_names.contains(&"review"), "review should be loaded");
    assert!(
        !skill_names.contains(&"debug"),
        "debug should be filtered out"
    );

    // No prompts should be loaded (empty filter)
    assert!(
        support.session_resources.prompts.is_empty(),
        "prompts should be empty"
    );
}

#[tokio::test]
async fn extension_ui_notify_and_confirm_plumbing() {
    if !node_available() {
        eprintln!("Skipping test: node not available");
        return;
    }

    let cwd = tempdir().unwrap();
    let ext_path = cwd.path().join("ui-ext.js");
    fs::write(
        &ext_path,
        r#"
                module.exports = function(bb) {
                    bb.registerCommand('ui-demo', {
                        description: 'demo UI methods',
                        handler: async (args, ctx) => {
                            ctx.ui.notify('extension says hi', 'info');
                            ctx.ui.setStatus('demo', 'active');
                            const ok = await ctx.ui.confirm('Title', 'Sure?');
                            const picked = await ctx.ui.select('Pick', ['a','b']);
                            return { message: `ok=${ok} picked=${picked}` };
                        },
                    });
                };
            "#,
    )
    .unwrap();

    let bootstrap = ExtensionBootstrap {
        paths: vec![ext_path],
        package_sources: Vec::new(),
    };
    let settings = Settings::default();
    // Load with has_ui=true to get an ExtensionUiHandler
    let support = load_runtime_extension_support_with_ui(cwd.path(), &settings, &bootstrap, true)
        .await
        .unwrap();

    // Get the interactive handler to verify stored notifications
    let handler = support
        .commands
        .ui_handler
        .as_ref()
        .expect("should have ui handler");
    // Downcast to ExtensionUiHandler
    let interactive_handler = handler
        .as_ref()
        .as_any()
        .downcast_ref::<ExtensionUiHandler>()
        .expect("should be ExtensionUiHandler");

    let output = support
        .commands
        .execute_text("/ui-demo")
        .await
        .unwrap()
        .unwrap();
    // Dialogs return defaults: confirm=false, select=cancelled(undefined)
    assert_eq!(output, "ok=false picked=undefined");

    // Verify notifications were captured
    let notifications = interactive_handler.drain_notifications().await;
    assert!(!notifications.is_empty());
    assert_eq!(notifications[0].message, "extension says hi");
    assert_eq!(notifications[0].kind, "info");

    // Verify status was captured
    let statuses = interactive_handler.get_statuses().await;
    assert_eq!(statuses.get("demo"), Some(&Some("active".to_string())));
}

#[test]
fn auto_install_skips_local_and_already_installed() {
    let cwd = tempdir().unwrap();
    let local_dir = cwd.path().join("local-pkg");
    fs::create_dir_all(&local_dir).unwrap();

    // Settings with a local path — should be silently skipped.
    let settings = Settings {
        packages: vec![PackageEntry::Simple(local_dir.display().to_string())],
        ..Settings::default()
    };

    // Should not panic or error — local paths are skipped.
    auto_install_missing_packages(cwd.path(), &settings);
}

#[test]
fn resolve_package_directory_prefers_project_root_install_from_nested_cwd() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    let nested = repo.path().join("crates").join("inner");
    fs::create_dir_all(&nested).unwrap();

    let spec = "@demo/pkg";
    let package_name = npm_package_name(spec).unwrap();
    let install_root = package_install_root("npm", spec, SettingsScope::Project, &nested);
    let package_dir = install_root.join("node_modules").join(&package_name);
    fs::create_dir_all(&package_dir).unwrap();

    let resolved = resolve_package_directory(&nested, &format!("npm:{spec}")).unwrap();
    assert_eq!(resolved, package_dir);
    assert!(resolved.starts_with(repo.path().join(".bb-agent")));
}

#[test]
fn auto_install_identifies_missing_npm_package_dir() {
    // Verify that an npm: package whose install directory does not exist
    // is recognised as needing installation (the actual npm install will
    // fail in the test environment, but the function handles the error
    // gracefully via tracing::warn).
    let cwd = tempdir().unwrap();

    // Use a unique spec so previous test runs can't leave stale dirs.
    let unique = format!("@test/nonexistent-{}", std::process::id());
    let source = format!("npm:{unique}");

    // Check the project-scoped root (under temp cwd) — guaranteed fresh.
    let root = package_install_root("npm", &unique, SettingsScope::Project, cwd.path());
    assert!(
        !root.exists(),
        "install root should not exist before auto-install"
    );

    let settings = Settings {
        packages: vec![PackageEntry::Simple(source)],
        ..Settings::default()
    };

    // Should attempt install and handle the failure without panicking.
    auto_install_missing_packages(cwd.path(), &settings);
}

#[test]
fn build_skill_section_includes_skills_and_prompts() {
    let resources = SessionResourceBootstrap {
        skills: vec![SkillDefinition {
            info: SkillInfo {
                name: "demo-review".to_string(),
                description: "Review code carefully".to_string(),
                source_info: SourceInfo {
                    path: "/skills/demo-review/SKILL.md".to_string(),
                    source: "settings:project".to_string(),
                },
            },
            content: "Review the code.".to_string(),
        }],
        prompts: vec![PromptTemplateDefinition {
            info: PromptTemplateInfo {
                name: "fix-tests".to_string(),
                description: "Fix all failing tests".to_string(),
                source_info: SourceInfo {
                    path: "/prompts/fix-tests.md".to_string(),
                    source: "settings:project".to_string(),
                },
            },
            content: "Fix tests.".to_string(),
        }],
        ..SessionResourceBootstrap::default()
    };
    let section = build_skill_system_prompt_section(&resources);
    assert!(section.contains("<available_skills>"));
    assert!(section.contains("demo-review"));
    assert!(section.contains("Review code carefully"));
    assert!(section.contains("/fix-tests"));
    assert!(section.contains("Fix all failing tests"));
}

#[test]
fn build_skill_section_empty_when_no_resources() {
    let resources = SessionResourceBootstrap::default();
    let section = build_skill_system_prompt_section(&resources);
    assert!(section.is_empty());
}
