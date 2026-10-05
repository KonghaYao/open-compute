//! Real asset sessions, immutable publication and Python/JavaScript Fetcher parity.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::fs;

pub(super) const FIRST: &str =
    include_str!("../../../../test/applications/python-runtime/public/message.txt");
pub(super) const SECOND: &str = "second-static-asset\n";

pub(super) async fn upload(fixture: &Fixture, script: &str, bytes: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut digest = blake3::Hasher::new();
    digest.update(encoded.as_bytes());
    digest.update(b"txt");
    let hash = digest.finalize().to_hex()[..32].to_owned();
    if bytes == FIRST {
        let root = super::python_support::repo_root().join("test/fixtures/python-runtime");
        let captured: Value =
            serde_json::from_slice(&fs::read(root.join("assets-session.json")).unwrap()).unwrap();
        assert_eq!(
            captured["manifest"]["/message.txt"],
            json!({"hash":hash,"size":bytes.len()})
        );
        let wire: Value =
            serde_json::from_slice(&fs::read(root.join("assets-upload.json")).unwrap()).unwrap();
        assert_eq!(wire["entries"].as_array().unwrap().len(), 1);
        assert_eq!(wire["entries"][0]["paths"], json!(["/message.txt"]));
        assert_eq!(wire["entries"][0]["hash"], hash);
        assert_eq!(
            wire["entries"][0]["sha256"],
            hex::encode(Sha256::digest(bytes))
        );
        assert_eq!(
            wire["sha256"],
            hex::encode(Sha256::digest(
                fs::read(root.join("assets-upload.multipart")).unwrap()
            ))
        );
    }
    let created = fixture
        .api(
            &format!("/workers/scripts/{script}/assets-upload-session"),
            "POST",
            Some(json!({
                "manifest":{"/message.txt":{"hash":hash,"size":bytes.len()}}
            })),
        )
        .await;
    let token = created["jwt"].as_str().unwrap();
    let buckets = created["buckets"].as_array().unwrap();
    if buckets.is_empty() {
        return token.to_owned();
    }
    assert_eq!(buckets, &vec![json!([hash])]);
    let boundary = "python-runtime-assets-upload";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{hash}\"; filename=\"{hash}\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{encoded}\r\n--{boundary}--\r\n"
    );
    let (status, _, body) = fixture
        .request(
            &format!(
                "/client/v4/accounts/{}/workers/assets/upload?base64=true",
                fixture.public_account
            ),
            "POST",
            &format!("multipart/form-data; boundary={boundary}"),
            body.into_bytes(),
            RequestTarget::AssetUpload(token),
        )
        .await;
    assert_eq!(status, 201);
    let completed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(completed["success"], true);
    completed["result"]["jwt"].as_str().unwrap().to_owned()
}

pub(super) async fn parity(fixture: &Fixture, python: &str) {
    let mut first = None;
    for (script, caller, expected) in [
        (PYTHON, "sdk", python),
        (PYTHON, "ffi", python),
        (PEER, "javascript", FIRST),
    ] {
        let path = format!("/assets?caller={caller}");
        let get = fixture.invoke(script, &path).await;
        assert_eq!(get["status"], 200);
        assert_eq!(get["body"], expected);
        assert_eq!(get["headers"]["content-type"], "text/plain; charset=utf-8");
        assert_eq!(get["headers"]["content-length"], expected.len().to_string());
        assert!(get["headers"]["content-range"].is_null());
        let etag = get["headers"]["etag"].as_str().unwrap();
        assert!(!etag.is_empty());
        let mut query = url::Url::parse("https://fixture.invalid/assets").unwrap();
        query
            .query_pairs_mut()
            .append_pair("caller", caller)
            .append_pair("etag", etag);
        let conditional = fixture
            .invoke(script, &format!("/assets?{}", query.query().unwrap()))
            .await;
        assert_eq!(conditional["status"], 304);
        assert_eq!(conditional["body"], "");
        assert_eq!(conditional["headers"]["etag"], etag);
        let head = fixture.invoke(script, &format!("{path}&method=HEAD")).await;
        assert_eq!(head["status"], 200);
        assert_eq!(head["body"], "");
        assert_eq!(head["headers"], get["headers"]);
        let range = fixture
            .invoke(script, &format!("{path}&range=bytes%3D0-2"))
            .await;
        // The pinned upstream Asset Worker ignores Range on the binding path:
        // asset-worker/src/handler.ts resolveAssetIntentToResponse and
        // worker.ts unstableGetByETagImpl return the complete stored stream.
        assert_eq!(range["status"], 200);
        assert_eq!(range["body"], expected);
        assert_eq!(
            range["headers"]["content-length"],
            expected.len().to_string()
        );
        assert!(range["headers"]["content-range"].is_null());
        assert_eq!(
            fixture
                .invoke(script, &format!("{path}&path=/missing.txt"))
                .await["status"],
            404
        );
        assert_eq!(
            fixture.invoke(script, &format!("{path}&method=POST")).await["status"],
            405
        );
        if expected == FIRST {
            if let Some(first) = &first {
                assert_eq!(&get, first);
            } else {
                first = Some(get);
            }
        }
    }
}
