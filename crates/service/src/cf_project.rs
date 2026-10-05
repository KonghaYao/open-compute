//! File-only project preflight before credentials or upstream autoconfiguration.

use open_compute_core::{ErrorCode, PlatformError};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

pub(super) fn preflight(
    project: &Path,
    arguments: &[OsString],
    diagnostic: &mut impl Write,
) -> Result<(), PlatformError> {
    if !requires_config(arguments) || project.join("cloudflare.config.ts").is_file() {
        return Ok(());
    }
    let legacy = ["wrangler.json", "wrangler.jsonc", "wrangler.toml"]
        .into_iter()
        .filter(|name| project.join(name).is_file())
        .collect::<Vec<_>>();
    if legacy.is_empty() {
        return Err(PlatformError::new(
            ErrorCode::CfInvalid,
            "cloudflare.config.ts is missing; run cf init or configure the official builder explicitly. No files were changed by ocd.",
        ));
    }
    for file in legacy {
        writeln!(
            diagnostic,
            "Found ./{file}; migrate explicitly with cf migrate ./{file} --bundler vite"
        )
        .map_err(|_| {
            PlatformError::new(ErrorCode::CfInvalid, "failed to write project diagnostics")
        })?;
    }
    Err(PlatformError::new(
        ErrorCode::WranglerProjectUnsupported,
        "cloudflare.config.ts is missing; select an exact Wrangler file and run cf migrate <exact-file> --bundler vite. Complete the required follow-up steps, then retry. No files were changed by ocd.",
    ))
}

fn requires_config(arguments: &[OsString]) -> bool {
    let prefixes: &[&[&str]] = &[
        &["deploy"],
        &["dev"],
        &["build"],
        &["workers", "versions", "create"],
        &["workers", "triggers", "deploy"],
        &["workers", "check"],
        &["previews", "deploy"],
    ];
    let mut words = Vec::new();
    let mut prebuilt = false;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        let Some(argument) = argument.to_str() else {
            continue;
        };
        if argument == "--" {
            break;
        }
        let (flag, value) = argument
            .split_once('=')
            .map_or((argument, None), |(k, v)| (k, Some(v)));
        match flag {
            "--mode"
            | "-m"
            | "--profile"
            | "--zone"
            | "-z"
            | "--persist-to"
            | "--tag"
            | "--message"
            | "--secrets-file"
            | "--worker"
            | "--dispatch-namespace"
            | "--containers-rollout" => {
                if value.is_none() {
                    args.next();
                }
            }
            "--help" | "-h" | "--version" | "-v" | "--quiet" | "-q" | "--local" => {
                let mut enabled = value != Some("false");
                if value.is_none()
                    && args
                        .clone()
                        .next()
                        .is_some_and(|s| s == "false" || s == "true")
                {
                    enabled = args.next().is_some_and(|s| s == "true");
                }
                if enabled && matches!(flag, "--help" | "-h" | "--version" | "-v") {
                    return false;
                }
            }
            "--prebuilt" => {
                prebuilt = value != Some("false");
                if value.is_none()
                    && args
                        .clone()
                        .next()
                        .is_some_and(|s| s == "false" || s == "true")
                {
                    prebuilt = args.next().is_some_and(|s| s == "true");
                }
            }
            "--no-prebuilt" => prebuilt = false,
            _ if !argument.starts_with('-') && words.len() < 3 => words.push(argument),
            _ => {}
        }
    }
    let Some(prefix) = prefixes.iter().find(|prefix| words.starts_with(prefix)) else {
        return false;
    };
    let accepts_prebuilt = !matches!(prefix[0], "dev" | "build") && *prefix != ["workers", "check"];
    !(accepts_prebuilt && prebuilt)
}

#[cfg(test)]
#[path = "cf_project_tests.rs"]
mod tests;
