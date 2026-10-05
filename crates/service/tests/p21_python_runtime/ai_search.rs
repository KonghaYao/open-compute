//! Ordinary AI Search extension, nested handles and streaming through native FFI.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use open_compute_core::ErrorCode;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const INSTANCE: &str = "python-runtime-search";
pub(super) const ISOLATED: &str = "python-runtime-isolated";
const CONTENT: &str = "alpha beta gamma\n";
const CALLERS: [(&str, &str); 3] = [(PEER, "javascript"), (PYTHON, "sdk"), (PYTHON, "ffi")];

pub(super) struct State {
    items: BTreeMap<String, String>,
    job: String,
}

pub(super) async fn create_resources(fixture: &Fixture) {
    for name in ["default", ISOLATED] {
        let namespace = fixture
            .api("/ai-search/namespaces", "POST", Some(json!({"name":name})))
            .await;
        assert_eq!(namespace["name"], name);
    }
    let instance = fixture
        .api(
            "/ai-search/namespaces/default/instances",
            "POST",
            Some(json!({
                "id":INSTANCE,"index_method":{"vector":true,"keyword":true},"score_threshold":0,
                "chunk":false,"custom_metadata":[{"field_name":"kind","data_type":"text"}]
            })),
        )
        .await;
    assert_eq!(instance["id"], INSTANCE);
}

pub(super) async fn initialize(fixture: &Fixture) -> State {
    let job = call(fixture, PYTHON, "sdk", json!({"operation":"job_create"})).await;
    assert_eq!(job["end_reason"], "completed");
    let job = job["id"].as_str().unwrap().to_owned();
    let cancelled = call(
        fixture,
        PEER,
        "javascript",
        json!({"operation":"job_cancel","jobId":job}),
    )
    .await;
    assert_eq!(cancelled["id"], job);
    assert!(matches!(
        cancelled["end_reason"].as_str().unwrap(),
        "completed" | "cancelled"
    ));
    let mut state = State {
        items: BTreeMap::new(),
        job,
    };
    for (script, caller, kind) in [
        (PYTHON, "sdk", "string"),
        (PYTHON, "ffi", "blob"),
        (PEER, "javascript", "stream"),
    ] {
        upload(fixture, &mut state, script, caller, kind, caller).await;
    }
    // Exercise nested namespace CRUD without adding an unbound side channel.
    for (script, caller) in CALLERS {
        let name = format!("temporary-{caller}");
        let created = call(
            fixture,
            script,
            caller,
            json!({"operation":"namespace_create","name":name}),
        )
        .await;
        assert_eq!(created["id"], name);
        assert!(
            call(
                fixture,
                script,
                caller,
                json!({"operation":"namespace_delete","name":name})
            )
            .await
            .is_null()
        );
    }
    state
}

pub(super) async fn update(fixture: &Fixture, state: &mut State) {
    upload(fixture, state, PYTHON, "ffi", "blob", "second").await;
}

async fn upload(
    fixture: &Fixture,
    state: &mut State,
    script: &str,
    caller: &str,
    kind: &str,
    key: &str,
) {
    let filename = format!("{key}.txt");
    let item = call(fixture,script,caller,json!({"operation":"upload","filename":filename,"content":CONTENT,"kind":key,"contentKind":kind})).await;
    assert_eq!(item["key"], filename);
    assert_eq!(item["status"], "completed");
    assert_eq!(item["file_size"], CONTENT.len());
    assert_eq!(item["chunks_count"], 1);
    assert_eq!(item["metadata"]["kind"], key);
    let id = item["id"].as_str().unwrap().to_owned();
    assert!(!id.is_empty());
    assert!(state.items.insert(filename, id).is_none());
}

pub(super) async fn retained(fixture: &Fixture, state: &State) -> Value {
    let namespace = fixture
        .api("/ai-search/namespaces/default", "GET", None)
        .await;
    assert_eq!(namespace["name"], "default");
    let namespace_id: String = fixture
        .connection()
        .query_row(
            "SELECT id FROM resources WHERE kind='ai_search_namespace' AND name=?1 AND state='ready'",
            ["default"],
            |row| row.get(0),
        )
        .unwrap();
    uuid::Uuid::parse_str(&namespace_id).unwrap();
    let mut baseline = None;
    for (script, caller) in CALLERS {
        let listed = call(
            fixture,
            script,
            caller,
            json!({"operation":"namespace_list"}),
        )
        .await;
        let instances = listed["result"].as_array().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0]["id"], INSTANCE);
        let mut observations = None;
        for target in ["namespace", "direct"] {
            let info = call(
                fixture,
                script,
                caller,
                json!({"operation":"info","target":target}),
            )
            .await;
            assert_eq!(info["id"], INSTANCE);
            assert_eq!(info["namespace"], namespace_id);
            let stats = call(
                fixture,
                script,
                caller,
                json!({"operation":"stats","target":target}),
            )
            .await;
            assert_eq!(stats["completed"], state.items.len());
            assert_eq!(stats["queued"], 0);
            assert_eq!(stats["running"], 0);
            assert_eq!(stats["error"], 0);
            let listed_items = call(
                fixture,
                script,
                caller,
                json!({"operation":"items","target":target}),
            )
            .await;
            let items = listed_items["result"].as_array().unwrap();
            assert_eq!(items.len(), state.items.len());
            let actual: BTreeMap<_, _> = items
                .iter()
                .map(|item| {
                    (
                        item["key"].as_str().unwrap().to_owned(),
                        item["id"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            assert_eq!(actual, state.items);
            let mut inspected = BTreeMap::new();
            for (key, id) in &state.items {
                let item = call(
                    fixture,
                    script,
                    caller,
                    json!({"operation":"item","target":target,"itemId":id}),
                )
                .await;
                assert_eq!(item["info"]["id"], *id);
                assert_eq!(item["info"]["key"], *key);
                assert_eq!(item["info"]["status"], "completed");
                assert_eq!(item["download"]["body"], CONTENT);
                assert_eq!(item["download"]["filename"], *key);
                assert_eq!(item["download"]["size"], CONTENT.len());
                assert!(!item["download"]["contentType"].as_str().unwrap().is_empty());
                assert!(!item["logs"]["result"].as_array().unwrap().is_empty());
                assert_eq!(item["chunks"]["result"].as_array().unwrap().len(), 1);
                inspected.insert(key.clone(), item["info"].clone());
            }
            let job = call(
                fixture,
                script,
                caller,
                json!({"operation":"job","target":target,"jobId":state.job}),
            )
            .await;
            assert_eq!(job["info"]["id"], state.job);
            assert!(job["info"]["end_reason"].as_str().is_some());
            let jobs = call(
                fixture,
                script,
                caller,
                json!({"operation":"jobs","target":target}),
            )
            .await;
            assert!(
                jobs["result"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == state.job)
            );
            let queries = query_and_chat(fixture, script, caller, target, state.items.len()).await;
            let stable = json!({"info":info,"items":inspected,"job":job["info"],"queries":queries});
            if let Some(first) = &observations {
                assert_eq!(&stable, first, "namespace and direct binding diverged");
            } else {
                observations = Some(stable);
            }
        }
        let stable = observations.unwrap();
        if let Some(first) = &baseline {
            assert_eq!(&stable, first, "AI Search changed across language callers");
        } else {
            baseline = Some(stable);
        }
        let errors = call(fixture, script, caller, json!({"operation":"errors"})).await;
        for (name, code) in [
            ("query", "AI_SEARCH_INPUT_INVALID"),
            ("update", "AI_SEARCH_OPTION_UNSUPPORTED"),
            ("missing", ErrorCode::ResourceNotFound.as_str()),
            ("isolation", ErrorCode::ResourceNotFound.as_str()),
        ] {
            let error = errors["rejected"][name].as_str().unwrap();
            assert!(
                error.contains(code),
                "unexpected AI Search rejection: {error}"
            );
            for private in [
                "binding-backend",
                "/Users/",
                "control.sqlite",
                "Bearer ",
                "__OPEN_COMPUTE",
            ] {
                assert!(!error.contains(private));
            }
        }
    }
    management_authorization(fixture).await;
    baseline.unwrap()
}

async fn query_and_chat(
    fixture: &Fixture,
    script: &str,
    caller: &str,
    target: &str,
    expected: usize,
) -> Value {
    let request = json!({"query":"alpha","ai_search_options":{"retrieval":{"retrieval_type":"vector","match_threshold":0,"max_num_results":10}}});
    let vector = call(
        fixture,
        script,
        caller,
        json!({"operation":"search","target":target,"request":request}),
    )
    .await;
    assert_eq!(vector["search_query"], "alpha");
    assert_eq!(vector["query_kind"], "text");
    assert_eq!(vector["chunks"].as_array().unwrap().len(), expected);
    for chunk in vector["chunks"].as_array().unwrap() {
        assert!(chunk["text"].as_str().unwrap().contains("alpha"));
        assert!((0.0..=1.0).contains(&chunk["score"].as_f64().unwrap()));
    }
    let filtered = call(fixture,script,caller,json!({"operation":"search","target":target,"request":{"query":"alpha","ai_search_options":{"retrieval":{"retrieval_type":"keyword","match_threshold":0,"filters":{"kind":{"$eq":"sdk"}}}}}})).await;
    assert_eq!(filtered["chunks"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["chunks"][0]["item"]["key"], "sdk.txt");
    let multi = call(fixture,script,caller,json!({"operation":"multi_search","request":{"query":"alpha","ai_search_options":{"instance_ids":[INSTANCE],"retrieval":{"retrieval_type":"vector","match_threshold":0,"max_num_results":10}}}})).await;
    assert_eq!(multi["chunks"].as_array().unwrap().len(), expected);
    assert!(
        multi["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|chunk| chunk["instance_id"] == INSTANCE)
    );
    let mut chat = call(fixture,script,caller,json!({"operation":"chat","target":target,"request":{"messages":[{"role":"user","content":"alpha"}],"ai_search_options":{"retrieval":{"match_threshold":0,"max_num_results":10}}}})).await;
    uuid::Uuid::parse_str(chat["id"].as_str().unwrap()).unwrap();
    chat.as_object_mut().unwrap().remove("id");
    assert_eq!(chat["choices"][0]["message"]["content"], "native answer");
    assert_eq!(chat["chunks"].as_array().unwrap().len(), expected);
    let stream = call(fixture,script,caller,json!({"operation":"stream","target":target,"request":{"messages":[{"role":"user","content":"alpha"}],"stream":true,"ai_search_options":{"retrieval":{"match_threshold":0,"max_num_results":10}}}})).await;
    let body = stream["body"].as_str().unwrap();
    assert!(body.starts_with("event: chunks\n"));
    assert!(body.ends_with("data: [DONE]\n\n"));
    let mut text = String::new();
    let mut ids = BTreeMap::new();
    let mut events = Vec::new();
    let mut retrieved = None;
    let mut created = None;
    for block in body.split("\n\n").filter(|part| !part.is_empty()) {
        let data = block
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap();
        if data == "[DONE]" {
            continue;
        }
        let mut event: Value = serde_json::from_str(data).unwrap();
        if block.starts_with("event: chunks") {
            assert_eq!(event.as_array().unwrap().len(), expected);
            assert!(retrieved.replace(event).is_none());
            continue;
        }
        let id = event["id"].as_str().unwrap();
        uuid::Uuid::parse_str(id).unwrap();
        ids.insert(id.to_owned(), ());
        let timestamp = event["created"].as_i64().unwrap();
        assert!(timestamp > 0);
        if let Some(first) = created.replace(timestamp) {
            assert_eq!(first, timestamp);
        }
        assert_eq!(event["object"], "chat.completion.chunk");
        if let Some(delta) = event["choices"][0]["delta"]["content"].as_str() {
            text.push_str(delta);
        }
        let object = event.as_object_mut().unwrap();
        object.remove("id");
        object.remove("created");
        events.push(event);
    }
    assert_eq!(ids.len(), 1);
    assert_eq!(text, "native answer");
    let mut observations = json!({"vector":vector,"filtered":filtered,"multi":multi,"chat":chat,"stream":{"chunks":retrieved.unwrap(),"events":events}});
    normalize_scores(&mut observations);
    observations
}

// Preserve every response field while comparing JSON numeric values across FFI.
fn normalize_scores(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_scores),
        Value::Object(fields) => {
            for (name, value) in fields {
                if name == "score" && value.is_number() {
                    let score = value.as_f64().unwrap();
                    assert!(score.is_finite());
                    *value = json!(score);
                } else {
                    normalize_scores(value);
                }
            }
        }
        _ => {}
    }
}

async fn management_authorization(fixture: &Fixture) {
    let path = format!(
        "/client/v4/accounts/{}/ai-search/namespaces/default/instances/{INSTANCE}",
        fixture.public_account
    );
    let before = fixture
        .api(
            &format!("/ai-search/namespaces/default/instances/{INSTANCE}"),
            "GET",
            None,
        )
        .await;
    let (status, _, bytes) = fixture
        .request(
            &path,
            "PUT",
            "application/json",
            br#"{"paused":true}"#.to_vec(),
            RequestTarget::ReadOnly,
        )
        .await;
    assert_eq!(status, 403);
    let rejected: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rejected["errors"][0]["code"], 9_100_002);
    let (status, _, bytes) = fixture
        .request(
            &path,
            "DELETE",
            "application/json",
            Vec::new(),
            RequestTarget::Admin,
        )
        .await;
    assert_eq!(status, 409);
    let rejected: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rejected["errors"][0]["code"], 9_100_006);
    assert_eq!(
        fixture
            .api(
                &format!("/ai-search/namespaces/default/instances/{INSTANCE}"),
                "GET",
                None
            )
            .await,
        before
    );
}

async fn call(fixture: &Fixture, script: &str, caller: &str, payload: Value) -> Value {
    let (status, _, bytes) = fixture
        .request(
            &format!("/search?caller={caller}"),
            "POST",
            "application/json",
            serde_json::to_vec(&payload).unwrap(),
            RequestTarget::Worker(script),
        )
        .await;
    assert_eq!(status, 200, "AI Search extension invocation failed");
    serde_json::from_slice(&bytes).unwrap()
}
