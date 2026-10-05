//! Shared public observations of real Python and JavaScript Durable Objects.

use super::python_support::fixture::{Fixture, RequestTarget};
use super::{PEER, PYTHON};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(super) async fn state(fixture: &Fixture, caller: &str, count: u64, revision: &str) -> Value {
    let value = fixture.invoke(caller, "/read").await;
    assert_eq!(value["count"], count);
    assert_eq!(value["kv"], count);
    assert_eq!(value["name"], "shared");
    assert_eq!(value["revision"], revision);
    assert!(value["boot"].as_u64().unwrap() >= 1);
    let id = value["id"].as_str().unwrap();
    assert_eq!(id.len(), 64);
    assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    value
}

pub(super) async fn values(fixture: &Fixture, count: u64, revision: &str) {
    for (caller, count, revision) in [(PYTHON, count, revision), (PEER, 2, "javascript")] {
        assert_eq!(
            fixture.invoke(caller, "/echo").await,
            json!({"unicode":"µ☁","nested":[null,true,42,1.25]})
        );
        assert_eq!(
            fixture.invoke(caller, "/invalid").await,
            json!({"rejected":true})
        );
        assert_eq!(
            fixture.invoke(caller, "/failure").await,
            json!({"rejected":true})
        );
        let (status, headers, body) = fixture
            .request(
                "/fetch",
                "GET",
                "application/json",
                Vec::new(),
                RequestTarget::Worker(caller),
            )
            .await;
        assert_eq!(status, 201);
        assert_eq!(headers["x-actor"], "counter");
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["count"], count);
        assert_eq!(value["kv"], count);
        assert_eq!(value["revision"], revision);
        assert_eq!(value["method"], "POST");
        assert_eq!(value["body"], "object-body");
        assert_eq!(value["query"], json!(["one", "two"]));
        assert_eq!(value["header"], "caller");
        assert_eq!(value["host"], "object.example");
    }
}

pub(super) async fn alarms(fixture: &Fixture) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let python = fixture.invoke(PYTHON, "/read").await;
        let javascript = fixture.invoke(PEER, "/read").await;
        if python["alarms"].as_u64().unwrap() >= 1 && javascript["alarms"].as_u64().unwrap() >= 1 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "persisted DO alarms were not delivered"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) fn no_prepare_upload(fixture: &Fixture) {
    assert!(fixture.mock.recorded().iter().all(|request| {
        !request.path.contains("/system/artifacts/v1/sha256/")
            || (request.method != "PUT" && request.method != "POST")
    }));
}

pub(super) fn scheduled(fixture: &Fixture, id: &str, due: i64) {
    let job: (i64, String, String) = fixture
        .scheduler_connection()
        .query_row(
            "SELECT due_at_ms, state, kind FROM scheduled_jobs WHERE object_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(job, (due, "scheduled".to_owned(), "do_alarm".to_owned()));
}

pub(super) async fn retired(fixture: &Fixture) {
    assert_eq!(
        fixture.invoke(PYTHON, "/").await,
        json!({"retired":true,"revision":"retired"})
    );
    state(fixture, PEER, 2, "javascript").await;
    let lifecycle: String = fixture
        .connection()
        .query_row(
            "SELECT n.lifecycle_state FROM do_namespaces n JOIN workers w ON w.id=n.owner_worker_id
         WHERE w.name=?1 AND n.class_name='Counter'",
            [PYTHON],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(lifecycle, "retired");
}
