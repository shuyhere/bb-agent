use super::*;
use std::fs;
use std::path::Path;
use uuid::Uuid;

fn make_temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bb-settings-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_settings_default() {
    let s = Settings::default();
    assert_eq!(s.resolved_execution_mode(), ExecutionMode::Yolo);
    assert!(s.compaction.enabled);
    assert_eq!(s.compaction.reserve_tokens, 16384);
    assert_eq!(s.compaction.keep_recent_tokens, 20000);
    assert!(s.default_provider.is_none());
    assert!(s.default_model.is_none());
    assert_eq!(s.execution_mode, None);
    assert_eq!(s.resolved_execution_mode(), ExecutionMode::Yolo);
    assert!(s.extensions.is_empty());
    assert!(s.skills.is_empty());
    assert!(s.prompts.is_empty());
    assert!(s.enable_skill_commands);
    assert!(s.update_check.enabled);
    assert_eq!(s.update_check.ttl_hours, 24);
}

#[test]
fn test_settings_deserialize() {
    let json = r#"{
            "default_provider": "anthropic",
            "default_model": "sonnet",
            "executionMode": "yolo",
            "compaction": {
                "enabled": false,
                "reserve_tokens": 8000
            },
            "skills": ["./skills"],
            "enableSkillCommands": false,
            "updateCheck": {
                "enabled": false,
                "ttlHours": 6
            },
            "models": [
                {
                    "id": "my-model",
                    "provider": "custom",
                    "context_window": 32000
                }
            ]
        }"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.default_provider.as_deref(), Some("anthropic"));
    assert_eq!(s.execution_mode, Some(ExecutionMode::Yolo));
    assert_eq!(s.resolved_execution_mode(), ExecutionMode::Yolo);
    assert!(!s.compaction.enabled);
    assert_eq!(s.compaction.reserve_tokens, 8000);
    assert_eq!(s.compaction.keep_recent_tokens, 20000);
    assert_eq!(s.skills, vec!["./skills"]);
    assert!(!s.enable_skill_commands);
    assert!(!s.update_check.enabled);
    assert_eq!(s.update_check.ttl_hours, 6);
    assert_eq!(s.models.as_ref().unwrap().len(), 1);
    assert_eq!(s.models.as_ref().unwrap()[0].id, "my-model");
}

#[test]
fn test_settings_merge() {
    let global = Settings {
        execution_mode: Some(ExecutionMode::Safety),
        default_provider: Some("openai".into()),
        default_model: Some("gpt-4o".into()),
        compaction: CompactionConfig {
            enabled: true,
            reserve_tokens: 8192,
            keep_recent_tokens: 20000,
        },
        extensions: vec!["./global-extension.ts".into()],
        skills: vec!["./global-skills".into()],
        enable_skill_commands: true,
        update_check: UpdateCheckSettings {
            enabled: true,
            ttl_hours: 48,
        },
        models: Some(vec![ModelOverride {
            id: "custom-1".into(),
            name: Some("Custom 1".into()),
            provider: "custom".into(),
            api: None,
            base_url: None,
            context_window: Some(32000),
            max_tokens: None,
            reasoning: None,
        }]),
        ..Default::default()
    };

    let project = Settings {
        execution_mode: Some(ExecutionMode::Yolo),
        default_provider: Some("anthropic".into()),
        extensions: vec![
            "./project-extension.ts".into(),
            "./global-extension.ts".into(),
        ],
        prompts: vec!["./project-prompts".into()],
        enable_skill_commands: false,
        update_check: UpdateCheckSettings {
            enabled: false,
            ttl_hours: 12,
        },
        models: Some(vec![ModelOverride {
            id: "custom-2".into(),
            name: Some("Custom 2".into()),
            provider: "local".into(),
            api: None,
            base_url: Some("http://localhost:8080".into()),
            context_window: Some(16000),
            max_tokens: None,
            reasoning: None,
        }]),
        ..Default::default()
    };

    let merged = Settings::merge(&global, &project);

    assert_eq!(merged.resolved_execution_mode(), ExecutionMode::Yolo);
    assert_eq!(merged.default_provider.as_deref(), Some("anthropic"));
    assert_eq!(merged.default_model.as_deref(), Some("gpt-4o"));
    assert_eq!(merged.execution_mode, Some(ExecutionMode::Yolo));
    assert_eq!(merged.compaction.reserve_tokens, 8192);
    assert_eq!(
        merged.extensions,
        vec!["./global-extension.ts", "./project-extension.ts"]
    );
    assert_eq!(merged.skills, vec!["./global-skills"]);
    assert_eq!(merged.prompts, vec!["./project-prompts"]);
    assert!(!merged.enable_skill_commands);
    assert!(!merged.update_check.enabled);
    assert_eq!(merged.update_check.ttl_hours, 12);
    let models = merged.models.unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "custom-1");
    assert_eq!(models[1].id, "custom-2");
}

#[test]
fn test_settings_merge_model_override() {
    let global = Settings {
        models: Some(vec![ModelOverride {
            id: "my-model".into(),
            name: Some("Old Name".into()),
            provider: "openai".into(),
            api: None,
            base_url: None,
            context_window: Some(32000),
            max_tokens: None,
            reasoning: None,
        }]),
        ..Default::default()
    };

    let project = Settings {
        models: Some(vec![ModelOverride {
            id: "my-model".into(),
            name: Some("New Name".into()),
            provider: "openai".into(),
            api: None,
            base_url: Some("http://localhost".into()),
            context_window: Some(64000),
            max_tokens: None,
            reasoning: None,
        }]),
        ..Default::default()
    };

    let merged = Settings::merge(&global, &project);
    let models = merged.models.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name.as_deref(), Some("New Name"));
    assert_eq!(models[0].context_window, Some(64000));
}

#[test]
fn test_settings_merge_execution_mode_uses_global_when_project_unset() {
    let global = Settings {
        execution_mode: Some(ExecutionMode::Yolo),
        ..Default::default()
    };

    let project = Settings::default();

    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.resolved_execution_mode(), ExecutionMode::Yolo);
}

#[test]
fn test_settings_merge_execution_mode_can_override_back_to_safety() {
    let global = Settings {
        execution_mode: Some(ExecutionMode::Yolo),
        ..Default::default()
    };

    let project = Settings {
        execution_mode: Some(ExecutionMode::Safety),
        ..Default::default()
    };

    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.execution_mode, Some(ExecutionMode::Safety));
    assert_eq!(merged.resolved_execution_mode(), ExecutionMode::Safety);
}

#[test]
fn test_settings_parse_execution_mode_defaults_to_yolo() {
    let s = Settings::parse("{}");
    assert_eq!(s.resolved_execution_mode(), ExecutionMode::Yolo);
}

#[test]
fn test_load_nonexistent_file() {
    let s = Settings::load_from_file(Path::new("/nonexistent/path/settings.json"));
    assert!(s.compaction.enabled);
    assert!(s.default_provider.is_none());
}

#[test]
fn test_parse_valid_json() {
    let json = r#"{"default_provider": "anthropic", "compaction": {"enabled": false}}"#;
    let s = Settings::parse(json);
    assert_eq!(s.default_provider.as_deref(), Some("anthropic"));
    assert!(!s.compaction.enabled);
    assert!(s.enable_skill_commands);
    assert!(s.update_check.enabled);
    assert_eq!(s.update_check.ttl_hours, 24);
}

#[test]
fn test_parse_invalid_json_returns_default() {
    let s = Settings::parse("not valid json");
    assert!(s.compaction.enabled);
    assert!(s.default_provider.is_none());
}

#[test]
fn test_parse_result_invalid_json_errors() {
    let err = Settings::parse_result("not valid json").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn test_parse_empty_object() {
    let s = Settings::parse("{}");
    assert!(s.compaction.enabled);
    assert_eq!(s.compaction.reserve_tokens, 16384);
    assert!(s.update_check.enabled);
    assert_eq!(s.update_check.ttl_hours, 24);
}

#[test]
fn test_package_entry_simple_string() {
    let json = r#"{"packages": ["npm:demo", "./local"]}
"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.packages.len(), 2);
    assert_eq!(s.packages[0].source(), "npm:demo");
    assert_eq!(s.packages[1].source(), "./local");
    assert!(s.packages[0].extensions_filter().is_none());
}

#[test]
fn test_package_entry_filtered_object() {
    let json = r#"{
            "packages": [
                "npm:simple",
                {
                    "source": "npm:filtered-pkg",
                    "extensions": ["ext/*.ts", "!ext/legacy.ts"],
                    "skills": [],
                    "prompts": ["prompts/review.md"]
                }
            ]
        }"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(s.packages.len(), 2);

    assert!(matches!(&s.packages[0], PackageEntry::Simple(src) if src == "npm:simple"));

    let filtered = match &s.packages[1] {
        PackageEntry::Filtered(f) => f,
        _ => panic!("expected filtered entry"),
    };
    assert_eq!(filtered.source, "npm:filtered-pkg");
    assert_eq!(
        filtered.extensions,
        Some(vec!["ext/*.ts".to_string(), "!ext/legacy.ts".to_string()])
    );
    assert_eq!(filtered.skills, Some(vec![]));
    assert_eq!(
        filtered.prompts,
        Some(vec!["prompts/review.md".to_string()])
    );
}

#[test]
fn test_package_merge_dedup_by_identity() {
    let global = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".into())],
        ..Settings::default()
    };
    let project = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".into())],
        ..Settings::default()
    };
    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.packages.len(), 1);
    assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");
}

#[test]
fn test_package_merge_different_packages_preserved() {
    let global = Settings {
        packages: vec![PackageEntry::Simple("npm:pkg-a".into())],
        ..Settings::default()
    };
    let project = Settings {
        packages: vec![PackageEntry::Simple("npm:pkg-b".into())],
        ..Settings::default()
    };
    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.packages.len(), 2);
    assert_eq!(merged.packages[0].source(), "npm:pkg-a");
    assert_eq!(merged.packages[1].source(), "npm:pkg-b");
}

#[test]
fn test_package_merge_filtered_overrides_simple() {
    let global = Settings {
        packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".into())],
        ..Settings::default()
    };
    let project = Settings {
        packages: vec![PackageEntry::Filtered(PackageFilter {
            source: "npm:@demo/pkg@2.0.0".into(),
            extensions: Some(vec!["ext/main.ts".into()]),
            skills: Some(vec![]),
            prompts: None,
        })],
        ..Settings::default()
    };
    let merged = Settings::merge(&global, &project);
    assert_eq!(merged.packages.len(), 1);
    assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");
    assert_eq!(merged.packages[0].skills_filter(), Some([].as_slice()));
}

#[test]
fn test_package_entry_roundtrip() {
    let entry = PackageEntry::Filtered(PackageFilter {
        source: "npm:test".into(),
        extensions: Some(vec!["a.ts".into()]),
        skills: None,
        prompts: Some(vec![]),
    });
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: PackageEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, entry);
}

#[test]
fn test_load_from_file_result_invalid_json_errors() {
    let root = make_temp_dir();
    let path = root.join("settings.json");
    fs::write(&path, "not valid json").unwrap();

    let err = Settings::load_from_file_result(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_project_uses_detected_ancestor_project_root() {
    let root = make_temp_dir();
    fs::write(root.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    fs::create_dir_all(root.join(".bb-agent")).unwrap();
    fs::write(
        root.join(".bb-agent").join("settings.json"),
        r#"{"default_model":"ancestor-model"}"#,
    )
    .unwrap();
    let nested = root.join("src").join("nested");
    fs::create_dir_all(&nested).unwrap();

    let loaded = Settings::load_project(&nested);
    assert_eq!(loaded.default_model.as_deref(), Some("ancestor-model"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn save_project_writes_to_detected_ancestor_project_root() {
    let root = make_temp_dir();
    fs::write(root.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    let nested = root.join("src").join("nested");
    fs::create_dir_all(&nested).unwrap();

    let settings = Settings {
        default_provider: Some("anthropic".into()),
        ..Default::default()
    };
    settings.save_project(&nested).unwrap();

    let saved = Settings::load_from_file(&root.join(".bb-agent").join("settings.json"));
    assert_eq!(saved.default_provider.as_deref(), Some("anthropic"));
    assert!(!nested.join(".bb-agent").join("settings.json").exists());

    let _ = fs::remove_dir_all(root);
}
