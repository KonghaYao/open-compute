//! Shared real-daemon ownership and immutable cf input for Python product Gates.

#![allow(
    dead_code,
    reason = "each Python integration target consumes a subset of the shared fixture"
)]

#[path = "../workflow_support/platform_process.rs"]
pub(crate) mod platform_process;

pub(crate) mod capture;
pub(crate) mod fixture;

pub(crate) const TOKEN: &str = "workflow-deployer";
pub(crate) const READ_ONLY_TOKEN: &str = "workflow-read-only";
pub(crate) const PYTHON_SECRETS: [&str; 2] = [
    "python-capture-fixture-token",
    "python-capture-fixture-token-second",
];

pub(crate) fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
