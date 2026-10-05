//! Native Cache API parity, Worker/namespace isolation and shared version state.

use super::python_support::fixture::Fixture;
use super::{PEER, PYTHON};
use serde_json::{Value, json};

pub(super) async fn matrix(fixture: &Fixture) {
    for namespace in ["default", "named"] {
        for (script, caller, value) in [
            (PYTHON, "sdk", "cache-sdk"),
            (PYTHON, "ffi", "cache-ffi"),
            (PEER, "javascript", "cache-javascript"),
        ] {
            assert_eq!(
                result(fixture, script, caller, namespace, "op=match").await,
                json!({"found":false})
            );
            assert_eq!(
                result(
                    fixture,
                    script,
                    caller,
                    namespace,
                    &format!("op=put&value={value}"),
                )
                .await,
                json!({"stored":true})
            );
            // The common URL/name must not share entries between Workers.
            let other = if script == PYTHON { PEER } else { PYTHON };
            assert_eq!(
                result(fixture, other, "ffi", namespace, "op=match").await,
                json!({"found":false})
            );
            assert_match(
                &result(fixture, script, caller, namespace, "op=match").await,
                value,
            );
            let conditional = result(
                fixture,
                script,
                caller,
                namespace,
                &format!("op=match&etag=%22{value}%22"),
            )
            .await;
            assert_eq!(conditional["found"], true);
            assert_eq!(conditional["status"], 304);
            assert_eq!(conditional["body"], "");
            assert_eq!(conditional["headers"]["etag"], format!("\"{value}\""));
            let range = result(
                fixture,
                script,
                caller,
                namespace,
                "op=match&range=bytes%3D0-2",
            )
            .await;
            assert_eq!(range["found"], true);
            assert_eq!(range["status"], 206);
            assert_eq!(range["body"], &value[..3]);
            assert_eq!(range["headers"]["content-length"], "3");
            assert_eq!(
                range["headers"]["content-range"],
                format!("bytes 0-2/{}", value.len())
            );
            assert_eq!(
                result(fixture, script, caller, namespace, "op=match&method=POST").await,
                json!({"found":false})
            );
            assert_match(
                &result(
                    fixture,
                    script,
                    caller,
                    namespace,
                    "op=match&method=POST&ignore_method=true",
                )
                .await,
                value,
            );
            assert_eq!(
                result(fixture, script, caller, namespace, "op=errors").await,
                json!({"method":true,"partial":true,"vary":true})
            );
            assert_eq!(
                result(fixture, script, caller, namespace, "op=delete&method=POST").await,
                json!({"deleted":false})
            );
            assert_match(
                &result(fixture, script, caller, namespace, "op=match").await,
                value,
            );
            assert_eq!(
                result(
                    fixture,
                    script,
                    caller,
                    namespace,
                    "op=delete&method=POST&ignore_method=true",
                )
                .await,
                json!({"deleted":true})
            );
            assert_eq!(
                result(fixture, script, caller, namespace, "op=match").await,
                json!({"found":false})
            );
            assert_eq!(
                result(fixture, script, caller, namespace, "op=delete").await,
                json!({"deleted":false})
            );
        }
        for (script, value) in [
            (PYTHON, "cache-python-retained"),
            (PEER, "cache-javascript-retained"),
        ] {
            assert_eq!(
                result(
                    fixture,
                    script,
                    "sdk",
                    namespace,
                    &format!("op=put&value={value}")
                )
                .await,
                json!({"stored":true})
            );
        }
    }
    retained(fixture, "cache-python-retained").await;
}

pub(super) async fn retained(fixture: &Fixture, python_default: &str) {
    for namespace in ["default", "named"] {
        let python_value = if namespace == "default" {
            python_default
        } else {
            "cache-python-retained"
        };
        for (script, caller, value) in [
            (PYTHON, "sdk", python_value),
            (PYTHON, "ffi", python_value),
            (PEER, "javascript", "cache-javascript-retained"),
        ] {
            assert_match(
                &result(fixture, script, caller, namespace, "op=match").await,
                value,
            );
        }
    }
}

pub(super) async fn new_version(fixture: &Fixture) {
    // Cache API state belongs to the Worker, independently of automatic caching.
    retained(fixture, "cache-python-retained").await;
    assert_eq!(
        result(
            fixture,
            PYTHON,
            "ffi",
            "default",
            "op=put&value=cache-second"
        )
        .await,
        json!({"stored":true})
    );
    assert_match(
        &result(fixture, PYTHON, "sdk", "default", "op=match").await,
        "cache-second",
    );
}

async fn result(
    fixture: &Fixture,
    script: &str,
    caller: &str,
    namespace: &str,
    query: &str,
) -> Value {
    fixture
        .invoke(
            script,
            &format!("/cache?caller={caller}&namespace={namespace}&{query}"),
        )
        .await
}

fn assert_match(value: &Value, expected: &str) {
    assert_eq!(
        value,
        &json!({
            "found":true,"status":200,"body":expected,
            "headers":{
                "content-type":"text/plain","cache-control":"public, max-age=3600",
                "etag":format!("\"{expected}\""),"content-length":expected.len().to_string(),
                "content-range":null,
            },
        })
    );
}
