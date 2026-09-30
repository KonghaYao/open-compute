use super::*;

#[test]
fn helper_io_and_open_url_paths() {
    const CHILD: &str = "OPEN_COMPUTE_FAKE_BROWSER_CHILD";
    const URL: &str = "https://example.invalid/";
    if std::env::var_os(CHILD).is_some() {
        open_url_in_browser(URL).unwrap();
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let opener = temp.path().join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    write_mode(
        &opener,
        "#!/bin/sh\n[ \"$1\" = \"https://example.invalid/\" ]\n",
        0o700,
    );
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "instance_ops::tests::helper_io_and_open_url_paths::helper_io_and_open_url_paths",
        ])
        .env(CHILD, "1")
        .env("PATH", temp.path())
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(io_failed().code(), ErrorCode::ConfigInvalid);
    let mut invocation = None;
    open_url_with(URL, |program, url| {
        invocation = Some((program.to_owned(), url.to_owned()));
        Ok(true)
    })
    .unwrap();
    let (program, url) = invocation.unwrap();
    assert_eq!(
        program,
        if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        }
    );
    assert_eq!(url, URL);
    assert_eq!(
        open_url_with(URL, |_, _| Ok(false)).unwrap_err().code(),
        ErrorCode::Internal
    );
    assert_eq!(
        open_url_with(URL, |_, _| { Err(std::io::Error::other("rejected")) })
            .unwrap_err()
            .code(),
        ErrorCode::Internal
    );
}
