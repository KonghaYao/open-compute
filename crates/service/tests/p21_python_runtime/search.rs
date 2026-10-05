//! Markdown conversion and asynchronous Vectorize authority shared by all callers.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(super) const INDEX: &str = "python-runtime-vectors";
const CALLERS: [(&str, &str); 3] = [(PEER, "javascript"), (PYTHON, "sdk"), (PYTHON, "ffi")];

pub(super) async fn create_index(fixture: &Fixture) {
    let created = fixture
        .api(
            "/vectorize/v2/indexes",
            "POST",
            Some(json!({
                "name":INDEX,"config":{"dimensions":3,"metric":"cosine"}
            })),
        )
        .await;
    assert_eq!(created["name"], INDEX);
    fixture
        .api(
            &format!("/vectorize/v2/indexes/{INDEX}/metadata_index/create"),
            "POST",
            Some(json!({
                "propertyName":"kind","indexType":"string"
            })),
        )
        .await;
}

pub(super) async fn initialize(fixture: &Fixture) {
    for (script, caller, id, values) in [
        (PYTHON, "sdk", "alpha", [1, 0, 0]),
        (PYTHON, "ffi", "beta", [0, 1, 0]),
        (PEER, "javascript", "temporary", [0, 0, 1]),
    ] {
        mutate(fixture, script, caller, "insert", json!([
            {"id":id,"values":values,"namespace":"shared","metadata":{"kind":"keep","revision":"first"}}
        ])).await;
    }
    // Official insert semantics preserve an existing ID. Wait for this exact
    // mutation frontier before observing it, including for the no-op insert.
    mutate(
        fixture,
        PYTHON,
        "sdk",
        "insert",
        json!([
            {"id":"alpha","values":[0,0,1],"namespace":"shared","metadata":{"kind":"wrong"}}
        ]),
    )
    .await;
    mutate(
        fixture,
        PEER,
        "javascript",
        "deleteByIds",
        json!(["temporary"]),
    )
    .await;
}

pub(super) async fn update(fixture: &Fixture) {
    mutate(fixture, PYTHON, "ffi", "upsert", json!([
        {"id":"alpha","values":[1,0,0],"namespace":"shared","metadata":{"kind":"keep","revision":"second"}}
    ])).await;
}

async fn mutate(fixture: &Fixture, script: &str, caller: &str, operation: &str, value: Value) {
    let receipt = call(
        fixture,
        script,
        caller,
        "/vectors",
        &json!({"operation":operation,"value":value}),
    )
    .await;
    let mutation = receipt["mutationId"].as_str().unwrap();
    uuid::Uuid::parse_str(mutation).unwrap();
    // Cover one durable 30-second claim recovery and the configured SQLite busy windows.
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let progress = fixture
            .api(&format!("/vectorize/v2/indexes/{INDEX}/info"), "GET", None)
            .await;
        if progress["processedUpToMutation"] == mutation {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Vectorize mutation {mutation} failed to reach its durable frontier: {progress}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(super) async fn retained(fixture: &Fixture, revision: &str) {
    markdown(fixture).await;
    let mut first = None;
    for (script, caller) in CALLERS {
        let description = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"describe"}),
        )
        .await;
        assert_eq!(description["dimensions"], 3);
        assert_eq!(description["vectorCount"], 2);
        assert!(description["processedUpToMutation"].as_u64().unwrap() >= 5);
        let mut get = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"getByIds","value":["alpha","beta","temporary"]}),
        )
        .await;
        assert_eq!(get.as_array().unwrap().len(), 2);
        for vector in get.as_array_mut().unwrap() {
            normalize_vector_values(vector);
        }
        let alpha = get
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == "alpha")
            .unwrap();
        assert_eq!(alpha["values"], json!([1.0, 0.0, 0.0]));
        assert_eq!(
            alpha["metadata"],
            json!({"kind":"keep","revision":revision})
        );
        let beta = get
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == "beta")
            .unwrap();
        assert_eq!(beta["values"], json!([0.0, 1.0, 0.0]));
        assert_eq!(beta["metadata"]["revision"], "first");
        let options = json!({"topK":2,"namespace":"shared","returnValues":true,"returnMetadata":"all","filter":{"kind":{"$eq":"keep"}}});
        let mut query = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"query","value":[1,0,0],"options":options}),
        )
        .await;
        let mut by_id = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"queryById","value":"alpha","options":options}),
        )
        .await;
        normalize_matches(&mut query);
        normalize_matches(&mut by_id);
        assert_eq!(query, by_id);
        assert_eq!(query["count"], 2);
        assert_eq!(query["matches"][0]["id"], "alpha");
        assert!((query["matches"][0]["score"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(query["matches"][0]["metadata"]["revision"], revision);
        let isolated = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"query","value":[1,0,0],"options":{"namespace":"empty"}}),
        )
        .await;
        assert_eq!(isolated, json!({"matches":[],"count":0}));
        let result = json!({"description":description,"get":get,"query":query});
        if let Some(first) = &first {
            assert_eq!(&result, first, "Vectorize changed across language callers");
        } else {
            first = Some(result);
        }
        let errors = call(
            fixture,
            script,
            caller,
            "/vectors",
            &json!({"operation":"errors"}),
        )
        .await;
        errors_contain(
            &errors,
            &[
                ("vector", "VECTORIZE_INPUT_INVALID"),
                ("topK", "VECTORIZE_LIMIT_EXCEEDED"),
                ("batch", "VECTORIZE_LIMIT_EXCEEDED"),
            ],
        );
    }
    let before = fixture
        .api(&format!("/vectorize/v2/indexes/{INDEX}/info"), "GET", None)
        .await;
    let python = fixture.active(PYTHON);
    let javascript = fixture.active(PEER);
    let (status, _, body) = fixture
        .request(
            &format!(
                "/client/v4/accounts/{}/vectorize/v2/indexes/{INDEX}",
                fixture.public_account
            ),
            "DELETE",
            "application/json",
            Vec::new(),
            RequestTarget::Admin,
        )
        .await;
    assert_eq!(status, 409);
    let rejected: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(rejected["errors"][0]["code"], 9_100_006);
    assert_eq!(
        fixture
            .api(&format!("/vectorize/v2/indexes/{INDEX}/info"), "GET", None)
            .await,
        before
    );
    assert_eq!(fixture.active(PYTHON), python);
    assert_eq!(fixture.active(PEER), javascript);
}

// JSON permits both integer and fractional spellings of the same number.
// Compare the documented numeric values without depending on the caller's
// serializer, while rejecting missing, nonnumeric, or nonfinite values.
fn normalize_vector_values(vector: &mut Value) {
    for value in vector["values"].as_array_mut().unwrap() {
        let number = value.as_f64().unwrap();
        assert!(number.is_finite());
        *value = json!(number);
    }
}

fn normalize_matches(response: &mut Value) {
    for matched in response["matches"].as_array_mut().unwrap() {
        normalize_vector_values(matched);
        let score = matched["score"].as_f64().unwrap();
        assert!(score.is_finite());
        matched["score"] = json!(score);
    }
}

async fn markdown(fixture: &Fixture) {
    for operation in ["single", "batch", "transform", "text", "supported"] {
        let mut javascript = None;
        for (script, caller) in CALLERS {
            let mut response = call(
                fixture,
                script,
                caller,
                "/ai",
                &json!({"operation":operation}),
            )
            .await;
            assert!(response["aiGatewayLogId"].is_null());
            let result = &mut response["result"];
            if operation == "supported" {
                assert!(
                    result
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["extension"] == ".pdf")
                );
            } else if let Some(batch) = result.as_array_mut() {
                assert_eq!(batch.len(), 2);
                conversion(&mut batch[0], "markdown");
                conversion(&mut batch[1], "error");
            } else {
                conversion(
                    result,
                    if operation == "text" {
                        "text"
                    } else {
                        "markdown"
                    },
                );
                assert!(
                    result["data"]
                        .as_str()
                        .unwrap()
                        .contains(if operation == "transform" {
                            "handle transform"
                        } else {
                            "markdown bridge"
                        })
                );
            }
            if let Some(javascript) = &javascript {
                assert_eq!(
                    &response, javascript,
                    "Markdown Conversion changed across callers"
                );
            } else {
                javascript = Some(response);
            }
        }
    }
    for (script, caller) in CALLERS {
        let errors = call(
            fixture,
            script,
            caller,
            "/ai",
            &json!({"operation":"errors"}),
        )
        .await;
        errors_contain(
            &errors,
            &[
                ("document", "AI_DOCUMENT_INVALID"),
                ("options", "AI_OPTION_UNSUPPORTED"),
                ("inference", "AI_UNSUPPORTED"),
            ],
        );
    }
}

fn conversion(result: &mut Value, format: &str) {
    uuid::Uuid::parse_str(result["id"].as_str().unwrap()).unwrap();
    assert_eq!(result["format"], format);
    assert!(result["name"].as_str().unwrap().len() < 256);
    if format == "error" {
        let error = result["error"].as_str().unwrap();
        assert!(!error.is_empty());
        sanitized(error);
    } else {
        assert!(result["tokens"].as_u64().is_some());
    }
    // Each real conversion owns a fresh ID; the ID's shape is validated above.
    // Compare document content and metadata without requiring repeated UUIDs.
    result.as_object_mut().unwrap().remove("id");
}

fn errors_contain(result: &Value, expected: &[(&str, &str)]) {
    for (operation, code) in expected {
        let error = result["rejected"][operation].as_str().unwrap();
        assert!(
            error.contains(code),
            "unexpected binding rejection: {error}"
        );
        sanitized(error);
    }
}

fn sanitized(error: &str) {
    for private in [
        "binding-backend",
        "x-open-compute",
        "descriptorSha256",
        "/internal/",
        "startup-generation",
    ] {
        assert!(
            !error.contains(private),
            "binding error exposed private authority"
        );
    }
}

async fn call(fixture: &Fixture, script: &str, caller: &str, path: &str, payload: &Value) -> Value {
    let (status, _, body) = fixture
        .request(
            &format!("{path}?caller={caller}"),
            "POST",
            "application/json",
            serde_json::to_vec(payload).unwrap(),
            RequestTarget::Worker(script),
        )
        .await;
    assert_eq!(status, 200, "binding invocation failed for {caller}/{path}");
    serde_json::from_slice(&body).unwrap()
}
