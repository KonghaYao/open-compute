//! Bounded public lifecycle observations and read-only persisted step identity.

use super::python_support::fixture::Fixture;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::time::{Duration, Instant};

pub(super) async fn status(fixture: &Fixture, script: &str, id: &str, expected: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let value = fixture.invoke(script, &format!("/status?id={id}")).await;
        if value["status"] == expected {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "Workflow {id} did not become {expected}; last {value}"
        );
        assert!(
            !matches!(
                value["status"].as_str(),
                Some("complete" | "errored" | "terminated")
            ),
            "Workflow {id} reached unexpected terminal status {value}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(super) async fn create(fixture: &Fixture, script: &str, id: &str, mode: &str) {
    assert_eq!(
        fixture
            .invoke(script, &format!("/create?id={id}&mode={mode}"))
            .await,
        json!({"id":id})
    );
}

pub(super) async fn effects(fixture: &Fixture, reader: &str, id: &str, count: u64, revision: &str) {
    assert_eq!(
        fixture.invoke(reader, &format!("/effects?id={id}")).await,
        json!({"calls":count,"revision":revision})
    );
}

pub(super) fn output(revision: &str, received: Option<&Value>) -> Value {
    json!({"prepared":{"value":43,"revision":revision},"received":received,"nested":[null,true,42,1.25]})
}

pub(super) fn prepare_record(
    fixture: &Fixture,
    workflow: &str,
    id: &str,
) -> (String, String, u64, String) {
    let (worker, state, attempt, bytes): (String, String, u64, Vec<u8>) = fixture.scheduler_connection().query_row(
        "SELECT i.worker_version_id,s.state,s.attempt,s.output_json FROM workflow_instances i
         JOIN workflow_steps s ON s.instance_id=i.id AND s.instance_generation=i.instance_generation
         WHERE i.definition_name=?1 AND i.external_instance_id=?2 AND s.kind='do' AND s.name='prepare'",
        rusqlite::params![workflow,id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).unwrap();
    assert_eq!(state, "complete");
    (worker, state, attempt, hex::encode(Sha256::digest(bytes)))
}

pub(super) fn no_prepare_upload(fixture: &Fixture) {
    assert!(fixture.mock.recorded().iter().all(|request| {
        !request.path.contains("/system/artifacts/v1/sha256/")
            || (request.method != "PUT" && request.method != "POST")
    }));
}
