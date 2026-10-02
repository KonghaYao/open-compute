use super::*;
use std::fs;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn preflight_recognizes_only_project_operations_and_real_flags() {
    for command in [
        vec!["deploy"],
        vec!["--mode", "production", "deploy"],
        vec!["--prebuilt=false", "deploy"],
        vec!["--quiet", "false", "deploy"],
        vec!["--local", "true", "deploy"],
        vec!["deploy", "--help", "false"],
        vec!["dev"],
        vec!["build"],
        vec!["workers", "versions", "create"],
        vec!["workers", "triggers", "deploy"],
        vec!["workers", "check"],
        vec!["previews", "deploy"],
        vec!["deploy", "--message", "--prebuilt"],
        vec!["deploy", "--message", "--help"],
        vec!["deploy", "--", "--prebuilt"],
        vec!["deploy", "--prebuilt=false"],
        vec!["deploy", "--prebuilt", "false"],
        vec!["deploy", "--prebuilt", "--no-prebuilt"],
        vec!["deploy", "--help=false"],
        vec!["dev", "--prebuilt"],
    ] {
        assert!(requires_config(&args(&command)), "{command:?}");
    }
    for command in [
        vec!["deploy", "--prebuilt"],
        vec!["--prebuilt", "deploy"],
        vec![
            "--mode",
            "production",
            "workers",
            "versions",
            "create",
            "--prebuilt",
        ],
        vec!["deploy", "--version"],
        vec!["-v", "deploy"],
        vec!["deploy", "--prebuilt=true"],
        vec!["deploy", "--prebuilt", "true"],
        vec!["deploy", "--help"],
        vec!["deploy", "-h"],
        vec!["workers", "versions", "create", "--prebuilt"],
        vec!["d1", "list"],
        vec!["cli", "search", "deploy"],
        vec!["schema", "deploy"],
        vec!["--help"],
        vec!["unknown"],
        vec!["workers"],
    ] {
        assert!(!requires_config(&args(&command)), "{command:?}");
    }
}

#[test]
fn preflight_diagnoses_all_legacy_files_without_editing_or_searching_parents() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("worker");
    fs::create_dir(&project).unwrap();
    fs::write(temp.path().join("cloudflare.config.ts"), "parent").unwrap();
    assert_eq!(
        preflight(&project, &args(&["deploy"]), &mut Vec::new())
            .unwrap_err()
            .code(),
        ErrorCode::CfInvalid
    );
    for name in ["wrangler.json", "wrangler.jsonc", "wrangler.toml"] {
        fs::write(project.join(name), "unchanged").unwrap();
        let mut diagnostic = Vec::new();
        let error = preflight(&project, &args(&["deploy"]), &mut diagnostic).unwrap_err();
        assert_eq!(error.code(), ErrorCode::WranglerProjectUnsupported);
        assert!(String::from_utf8(diagnostic).unwrap().contains(name));
        assert!(error.to_string().contains("--bundler vite"));
        assert_eq!(fs::read_to_string(project.join(name)).unwrap(), "unchanged");
    }
    assert!(preflight(&project, &args(&["deploy", "--prebuilt"]), &mut Vec::new()).is_ok());
    assert!(preflight(&project, &args(&["d1", "list"]), &mut Vec::new()).is_ok());
    fs::write(
        project.join("cloudflare.config.ts"),
        "throw new Error('migration TODO')",
    )
    .unwrap();
    assert!(preflight(&project, &args(&["deploy"]), &mut Vec::new()).is_ok());
}
