//! Interactive setup through a PTY without acquiring the controlling terminal.

use super::package_scope;
use std::fs;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

pub(super) async fn interactive_instance_setup(
    binary: &Path,
    config: &Path,
    data: &Path,
    confirm: bool,
) -> Output {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let slave_path = ptsname(&master, Vec::new()).unwrap();
    let slave = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
        .open(slave_path.to_str().unwrap())
        .unwrap();
    let mut process = tokio::process::Command::new(binary);
    process
        .current_dir(binary.parent().unwrap())
        .env_clear()
        .env("PATH", "")
        .env("HOME", binary.parent().unwrap().join("home"))
        .args([
            "instance",
            "setup",
            "--name",
            "pty",
            "--config",
            config.to_str().unwrap(),
            "--data-dir",
            data.to_str().unwrap(),
            "--autostart=false",
            "--start=false",
        ])
        .stdin(Stdio::from(slave))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !package_scope::enabled() {
        process.env(
            "OPEN_COMPUTE_TEST_OCD_ROOT",
            binary.parent().unwrap().join("test-ocd"),
        );
    }
    let child = process.spawn().unwrap();
    let mut master = fs::File::from(master);
    let answers: &[u8] = if confirm {
        b"\n\n\n\n\nyes\n"
    } else {
        b"\n\n\n\n\nno\n"
    };
    std::io::Write::write_all(&mut master, answers).unwrap();
    tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .expect("interactive instance setup timed out")
        .unwrap()
}
