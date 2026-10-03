//! Release qualification driver for the production upgrade workflow.
//! Only transport and the installed executable identity differ from the CLI.

use clap::Parser;
use open_compute_service::install_receipt::receipt_path_in;
use open_compute_service::instance_registry::{InstanceRegistry, ServiceScope};
use open_compute_service::release_upgrade::{
    LiveReleaseHttp, UpgradeOptions, host_release_target, run_upgrade,
};
use open_compute_service::service_manager::host_service_manager;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    download_base: String,
    #[arg(long)]
    version: String,
    #[arg(long)]
    system: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse();
    let binary = args.binary.canonicalize()?;
    let installed = Command::new(&binary)
        .args(["--no-update-check", "--version"])
        .output()?;
    if !installed.status.success() {
        return Err("installed executable version check failed".into());
    }
    let version_output = String::from_utf8(installed.stdout)?;
    let current_version = version_output
        .split_whitespace()
        .nth(1)
        .ok_or("installed executable did not report its version")?;
    semver::Version::parse(current_version)?;
    let scope = if args.system {
        ServiceScope::System
    } else {
        ServiceScope::User
    };
    let registry = InstanceRegistry::production()?;
    let options = UpgradeOptions {
        scope,
        version: Some(args.version),
        dry_run: false,
        no_restart: false,
        receipt_path: receipt_path_in(registry.root_for(scope)),
        staging_dir: binary.parent().ok_or("binary has no parent")?.to_owned(),
        binary_path: binary,
        download_base: args.download_base,
        target: host_release_target()?,
        current_version: current_version.to_owned(),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run_upgrade(
        &options,
        &LiveReleaseHttp::new()?,
        &registry,
        host_service_manager().as_ref(),
        &mut std::io::stdout(),
    ));
    if let Err(error) = result {
        eprintln!("{}: {}", error.code(), error.message());
        std::process::exit(1);
    }
    Ok(())
}
