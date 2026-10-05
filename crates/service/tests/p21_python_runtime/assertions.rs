//! Public SDK/FFI results, expected upstream stdlib scope and bounded background work.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::time::{Duration, Instant};

pub(super) async fn read(fixture: &Fixture, expected: &str) {
    for (script, path) in [
        (PYTHON, "/read"),
        (PYTHON, "/read?caller=ffi"),
        (PEER, "/read"),
    ] {
        assert_eq!(
            fixture.invoke(script, path).await,
            json!({"kv":expected,"d1":expected,"r2":expected})
        );
    }
}

pub(super) async fn ffi(fixture: &Fixture) {
    let expected = json!({
        "value":{"unicode":"µ☁","nested":[null,true,42,1.25]},"bytes":[0,1,255],
        "jsNull":null,"nullDistinctFromUndefined":true,
        "sha256":hex::encode(Sha256::digest([0u8,1,255])),"mapped":[2,4,6],
    });
    for script in [PYTHON, PEER] {
        let (status, headers, bytes) = fixture
            .request(
                "/ffi",
                "GET",
                "application/json",
                Vec::new(),
                RequestTarget::Worker(script),
            )
            .await;
        assert_eq!(status, 201);
        assert_eq!(headers["x-ffi"], "native");
        let mut result = serde_json::from_slice::<Value>(&bytes).unwrap();
        if script == PYTHON {
            assert_eq!(
                result.as_object_mut().unwrap().remove("released"),
                Some(json!(true))
            );
        }
        assert_eq!(result, expected);
    }
}

pub(super) async fn stdlib(fixture: &Fixture) {
    assert_eq!(
        fixture.invoke(PYTHON, "/stdlib").await,
        json!({
            "decimal":"0.3","base64":"AAH/","sha256":hex::encode(Sha256::digest([0u8,1,255])),
            "zlib":[0,1,255],"integer":42,"timestamp":1767225600.0,"path":"b",
            "contexts":["left","right"],"parent":"parent",
            "excluded":["curses","dbm","ensurepip","grp","idlelib","lib2to3","msvcrt",
                "pwd","resource","syslog","tkinter","turtle","turtledemo","venv","winreg","winsound"],
            "missingDependency":{},
            "threadRejected":true,"multiprocessingImported":true,
        })
    );
}

pub(super) async fn background(fixture: &Fixture, revision: &str) {
    assert_eq!(
        fixture.invoke(PYTHON, "/background").await,
        json!({"scheduled":true})
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let value = fixture.invoke(PEER, "/background-read").await;
        if value == revision {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "waitUntil KV commit was not observed: {value}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        fixture.invoke(PYTHON, "/background-failure").await,
        json!({"scheduled":true})
    );
}

pub(super) async fn outbound(fixture: &Fixture) {
    for (script, path, caller) in [
        (PYTHON, "/outbound", "sdk"),
        (PYTHON, "/outbound?caller=ffi", "ffi"),
        (PEER, "/outbound", "javascript"),
    ] {
        let (status, _, bytes) = fixture
            .request(
                path,
                "GET",
                "application/json",
                Vec::new(),
                RequestTarget::Worker(script),
            )
            .await;
        assert_eq!(status, 202);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"body":"µ☁","caller":caller})
        );
    }
    for script in [PYTHON, PEER] {
        let result = fixture.invoke(script, "/tcp").await;
        let response = result["response"].as_str().unwrap();
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "TCP fixture returned an incomplete response for {script}: {response:?}"
        );
        assert!(
            response.ends_with("python-runtime-tcp"),
            "TCP fixture body was truncated for {script}: {response:?}"
        );
    }
}

pub(super) async fn boundaries(fixture: &Fixture) {
    for caller in ["sdk", "ffi"] {
        assert_eq!(
            fixture
                .invoke(PYTHON, &format!("/errors?caller={caller}"))
                .await,
            json!({"kv":true,"d1":true,"r2":true})
        );
        let env = fixture
            .invoke(PYTHON, &format!("/env?caller={caller}"))
            .await;
        assert_eq!(
            env["keys"],
            json!([
                "AI",
                "ARTIFACTS",
                "ASSETS",
                "BUCKET",
                "DB",
                "DIRECT_SEARCH",
                "IMAGES",
                "ISOLATED_SEARCH",
                "KV",
                "OUTBOUND_URL",
                "REVISION",
                "SEARCH",
                "TOKEN",
                "VECTORS"
            ])
        );
        let repr = env["repr"].as_array().unwrap();
        assert_eq!(repr.len(), 11);
        for value in repr {
            let value = value.as_str().unwrap();
            assert!(!value.is_empty());
            for private in [
                "__OPEN_COMPUTE",
                "RuntimeSource",
                "system/artifacts",
                "control.sqlite",
                "/Users/",
                "Bearer ",
            ] {
                assert!(
                    !value.contains(private),
                    "binding repr exposed private authority"
                );
            }
        }
    }
    assert_eq!(
        fixture.invoke(PYTHON, "/log").await,
        json!({"emitted":true})
    );
    let (status, _, _) = fixture
        .request(
            "/exception",
            "GET",
            "application/json",
            Vec::new(),
            RequestTarget::Worker(PYTHON),
        )
        .await;
    assert_eq!(status, 500);
}

pub(super) fn no_prepare_upload(fixture: &Fixture) {
    assert!(fixture.mock.recorded().iter().all(|request| {
        !request.path.contains("/system/artifacts/v1/sha256/")
            || (request.method != "PUT" && request.method != "POST")
    }));
}
