//! Deployment authentication and read-only role admission before upload or preparation.

use super::SCRIPT;
use super::python_support::capture::Capture;
use super::python_support::fixture::{Fixture, RequestTarget};
use open_compute_service::http::REQUEST_ID_HEADER;
use serde_json::Value;

pub(super) async fn assert_upload_authorization(fixture: &Fixture, body: &[u8]) {
    let authority_counts = || {
        fixture
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM workers), (SELECT count(*) FROM worker_versions),
                    (SELECT count(*) FROM version_python_prepared)",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap()
    };
    let before = authority_counts();
    let objects = fixture.mock.object_count();
    let path = format!(
        "/client/v4/accounts/{}/workers/scripts/{SCRIPT}/versions",
        fixture.public_account
    );
    for (target, status, code) in [
        (RequestTarget::Unauthenticated, 401, 10_000),
        (RequestTarget::ReadOnly, 403, 9_100_002),
    ] {
        // Valid, complete official Python input must fail authentication before
        // any worker/version reservation, object upload or preparation publication.
        let (actual, headers, bytes) = fixture
            .request(
                &path,
                "POST",
                &Capture::content_type(),
                body.to_vec(),
                target,
            )
            .await;
        assert_eq!(actual, status);
        assert!(headers.contains_key(REQUEST_ID_HEADER));
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["success"], false);
        assert!(value["result"].is_null());
        assert_eq!(value["errors"][0]["code"], code);
        assert_eq!(authority_counts(), before);
        assert_eq!(fixture.mock.object_count(), objects);
    }
    let (status, _, bytes) = fixture
        .request(
            "/client/v4/accounts",
            "GET",
            "application/json",
            Vec::new(),
            RequestTarget::ReadOnly,
        )
        .await;
    assert_eq!(status, 200, "read-only credentials must retain read access");
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["success"], true);
    assert_eq!(value["result"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"][0]["id"], fixture.public_account);
}
