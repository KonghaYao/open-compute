//! Shared Artifacts extension authority via Python native FFI and JavaScript.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use serde_json::{Value, json};

pub(super) const NAMESPACE: &str = "python-runtime-apps";
pub(super) const ISOLATED: &str = "python-runtime-isolated";
const CALLERS: [(&str, &str); 3] = [(PEER, "javascript"), (PYTHON, "sdk"), (PYTHON, "ffi")];

pub(super) async fn create_namespaces(fixture: &Fixture) {
    for namespace in [NAMESPACE, ISOLATED] {
        let created = fixture
            .api(
                "/artifacts/namespaces",
                "POST",
                Some(json!({"namespace":namespace})),
            )
            .await;
        assert_eq!(created["namespace"], namespace);
    }
}

pub(super) async fn initialize(fixture: &Fixture) {
    for (script, caller, name) in [
        (PYTHON, "sdk", "python-sdk"),
        (PYTHON, "ffi", "python-ffi"),
        (PEER, "javascript", "javascript"),
    ] {
        let result = call(
            fixture,
            script,
            caller,
            json!({"operation":"create","name":name}),
        )
        .await;
        assert_eq!(result["name"], name);
        assert_eq!(result["description"], "Python binding parity");
        assert_eq!(result["defaultBranch"], "main");
        assert_eq!(result["tokenShapeValid"], true);
        assert!(result.get("token").is_none());
    }
    let forked = call(
        fixture,
        PYTHON,
        "ffi",
        json!({"operation":"fork","target":"temporary-fork"}),
    )
    .await;
    assert_eq!(forked["name"], "temporary-fork");
    assert_eq!(forked["tokenShapeValid"], true);
    assert_eq!(
        call(
            fixture,
            PEER,
            "javascript",
            json!({"operation":"delete","name":"temporary-fork"})
        )
        .await,
        true
    );
    let mut revoke = None;
    for (script, caller, scope) in [
        (PYTHON, "sdk", "read"),
        (PYTHON, "ffi", "write"),
        (PEER, "javascript", "read"),
    ] {
        let token = call(
            fixture,
            script,
            caller,
            json!({"operation":"token","scope":scope}),
        )
        .await;
        assert_eq!(token["scope"], scope);
        assert_eq!(token["tokenShapeValid"], true);
        assert!(token.get("plaintext").is_none());
        assert!(!token["id"].as_str().unwrap().is_empty());
        if caller == "sdk" {
            revoke = Some(token["id"].clone());
        }
    }
    assert_eq!(
        call(
            fixture,
            PEER,
            "javascript",
            json!({"operation":"revoke","tokenId":revoke.unwrap()})
        )
        .await,
        true
    );
}

pub(super) async fn update(fixture: &Fixture) {
    let created = call(
        fixture,
        PYTHON,
        "ffi",
        json!({"operation":"create","name":"second-version","description":"second"}),
    )
    .await;
    assert_eq!(created["name"], "second-version");
    assert_eq!(created["description"], "second");
    assert_eq!(created["tokenShapeValid"], true);
}

pub(super) async fn retained(fixture: &Fixture, second: bool) -> Value {
    let mut first = None;
    for (script, caller) in CALLERS {
        let listed = call(fixture, script, caller, json!({"operation":"list"})).await;
        let repos = listed["repos"].as_array().unwrap();
        assert_eq!(listed["total"], if second { 4 } else { 3 });
        assert_eq!(repos.len(), if second { 4 } else { 3 });
        let mut names: Vec<_> = repos
            .iter()
            .map(|repo| repo["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            if second {
                vec!["javascript", "python-ffi", "python-sdk", "second-version"]
            } else {
                vec!["javascript", "python-ffi", "python-sdk"]
            }
        );
        let inspected = call(fixture, script, caller, json!({"operation":"inspect"})).await;
        let repo = &inspected["repo"];
        assert_eq!(repo["name"], "python-sdk");
        assert_eq!(repo["description"], "Python binding parity");
        assert_eq!(repo["defaultBranch"], "main");
        assert_eq!(repo["readOnly"], false);
        assert!(repo["lastPushAt"].is_null() && repo["source"].is_null());
        assert!(
            repo["remote"]
                .as_str()
                .unwrap()
                .ends_with(&format!("/{NAMESPACE}/python-sdk.git"))
        );
        let tokens = inspected["tokens"]["tokens"].as_array().unwrap();
        assert_eq!(inspected["tokens"]["total"], 4);
        assert_eq!(tokens.len(), 4);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token["state"] == "active")
                .count(),
            3
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token["state"] == "revoked")
                .count(),
            1
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token["scope"] == "read")
                .count(),
            2
        );
        assert!(tokens.iter().all(|token| token.get("plaintext").is_none()));
        let result = json!({"listed":listed,"inspected":inspected});
        if let Some(first) = &first {
            assert_eq!(&result, first, "Artifacts changed across language callers");
        } else {
            first = Some(result);
        }
        pagination(fixture, script, caller, &names).await;
        let before = call(fixture, script, caller, json!({"operation":"inspect"})).await;
        let errors = call(fixture, script, caller, json!({"operation":"errors"})).await;
        for (failure, code, numeric) in [
            ("duplicate", "ALREADY_EXISTS", 10201),
            ("missing", "NOT_FOUND", 10200),
            ("name", "INVALID_REPO_NAME", 10101),
            ("ttl", "INVALID_TTL", 10103),
            ("import", "INVALID_INPUT", 10100),
        ] {
            let error = &errors["rejected"][failure];
            assert_eq!(error["code"], code);
            assert_eq!(error["numericCode"], numeric);
            assert_eq!(error["message"], code);
        }
        assert_eq!(
            call(fixture, script, caller, json!({"operation":"inspect"})).await,
            before
        );
    }
    let isolation = call(
        fixture,
        PEER,
        "javascript",
        json!({"operation":"isolation"}),
    )
    .await;
    assert_eq!(
        isolation,
        json!({"code":"NOT_FOUND","numericCode":10200,"message":"NOT_FOUND"})
    );
    readonly_authorization(fixture).await;
    first.unwrap()
}

async fn pagination(fixture: &Fixture, script: &str, caller: &str, expected: &[&str]) {
    let mut cursor = None;
    let mut names = Vec::new();
    loop {
        let mut options = json!({"limit":1});
        if let Some(cursor) = cursor {
            options["cursor"] = cursor;
        }
        let page = call(
            fixture,
            script,
            caller,
            json!({"operation":"list","options":options}),
        )
        .await;
        let repos = page["repos"].as_array().unwrap();
        assert_eq!(page["total"], expected.len());
        assert_eq!(repos.len(), 1);
        names.push(repos[0]["name"].as_str().unwrap().to_owned());
        assert!(
            names.len() <= expected.len(),
            "Artifacts cursor failed to make progress"
        );
        let Some(next) = page.get("cursor") else {
            break;
        };
        let next = next.as_str().unwrap();
        assert!(!next.is_empty());
        cursor = Some(json!(next));
    }
    names.sort_unstable();
    assert_eq!(names, expected);
}

async fn readonly_authorization(fixture: &Fixture) {
    let path = format!(
        "/client/v4/accounts/{}/artifacts/namespaces/{NAMESPACE}/repos",
        fixture.public_account
    );
    let (status, _, bytes) = fixture
        .request(
            &path,
            "GET",
            "application/json",
            Vec::new(),
            RequestTarget::ReadOnly,
        )
        .await;
    assert_eq!(status, 200);
    let before: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(before["success"], true);
    let (status, _, bytes) = fixture
        .request(
            &path,
            "POST",
            "application/json",
            br#"{"name":"unauthorized"}"#.to_vec(),
            RequestTarget::ReadOnly,
        )
        .await;
    assert_eq!(status, 403);
    let rejected: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rejected["errors"][0]["code"], 9_100_002);
    let (status, _, bytes) = fixture
        .request(
            &path,
            "GET",
            "application/json",
            Vec::new(),
            RequestTarget::ReadOnly,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), before);
}

async fn call(fixture: &Fixture, script: &str, caller: &str, payload: Value) -> Value {
    let (status, _, bytes) = fixture
        .request(
            &format!("/artifacts?caller={caller}"),
            "POST",
            "application/json",
            serde_json::to_vec(&payload).unwrap(),
            RequestTarget::Worker(script),
        )
        .await;
    assert_eq!(status, 200, "Artifacts binding invocation failed");
    // Operator or transport credentials must never escape even through a
    // successful token operation. The fixture validates their shape internally.
    assert!(!bytes.windows(7).any(|value| value == b"art_v1_"));
    serde_json::from_slice(&bytes).unwrap()
}
