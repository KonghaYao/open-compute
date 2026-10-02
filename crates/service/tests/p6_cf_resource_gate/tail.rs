use super::*;

pub(super) async fn exercise_live_tail(
    admin_addr: SocketAddr,
    public_addr: SocketAddr,
    public_account: &str,
    internal_account: &str,
) {
    let root = repo_root();
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new("bun")
            .arg("tests/live-tail-dashboard.mjs")
            .current_dir(root.join("packages/sdk"))
            .env(
                "OPEN_COMPUTE_V4_BASE_URL",
                format!("http://{admin_addr}/client/v4"),
            )
            .env("OPEN_COMPUTE_V4_TOKEN", TOKEN)
            .env("OPEN_COMPUTE_V4_ACCOUNT_ID", public_account)
            .env(
                "OPEN_COMPUTE_P7_PUBLIC_URL",
                format!("http://{public_addr}/live-tail"),
            )
            .env(
                "OPEN_COMPUTE_P7_PUBLIC_HOST",
                worker_host(internal_account, "p6-cf-resource-gate"),
            )
            .env("OPEN_COMPUTE_P7_SECRET", TAIL_SECRET)
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .output(),
    )
    .await
    .expect("Dashboard Live Tail differential timed out")
    .expect("run Dashboard Live Tail differential");
    assert_success(&output);
}

pub(super) fn assert_observability_audit(data: &Path) {
    let connection = rusqlite::Connection::open(data.join("control.sqlite")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT action, CAST(details_json AS TEXT)
             FROM control_audit_events
             WHERE action LIKE 'worker.tail.%' OR action = 'worker.observability.query'
             ORDER BY seq",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|(action, _)| action == "worker.tail.create")
            .count(),
        1
    );
    assert_eq!(
        rows.iter()
            .filter(|(action, _)| action == "worker.tail.delete")
            .count(),
        0
    );
    assert!(
        rows.iter()
            .any(|(action, _)| action == "worker.observability.query")
    );
    for (_, details) in rows {
        assert!(!details.contains(TAIL_SECRET));
        let details: Value = serde_json::from_str(&details).unwrap();
        assert!(
            details
                .as_object()
                .is_some_and(|object| object.keys().all(|key| matches!(
                    key.as_str(),
                    "view" | "fromMs" | "toMs" | "resultCount" | "filterKeys"
                ))),
            "observability audit included content-bearing details: {details}"
        );
    }
}

pub(super) async fn prepare_logged_worker(command: &CfCommand<'_>) {
    fs::write(command.project.join("index.ts"), "export default { fetch(request) { if (new URL(request.url).pathname.endsWith('/error')) { console.error('p7-tail-error'); throw new Error('p7-tail-error'); } console.log('p7-tail-event invoice'); return new Response('tail-ok'); } };").unwrap();
    assert_success(&command.run(&["deploy", "--mode", "production"]).await);
}

pub(super) async fn verify_version_upload_observability_boundary(fixture: &Fixture) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let base = format!(
        "http://{}/client/v4/accounts/{}/workers/scripts/p6-cf-resource-gate",
        fixture.admin_addr, fixture.public_account
    );
    let before = active_version(&client, fixture.admin_addr, &fixture.public_account).await;
    let settings = || {
        Request::builder()
            .uri(format!("{base}/script-settings"))
            .header("authorization", format!("Bearer {READ_ONLY_TOKEN}"))
            .body(Body::empty())
            .unwrap()
    };
    let response = client.request(settings()).await.unwrap();
    assert_eq!(response.status(), 200);
    let previous = to_bytes(Body::new(response.into_body()), 64 * 1024)
        .await
        .unwrap();
    for (sampling, status) in [(1, 200), (2, 400)] {
        let metadata = serde_json::json!({"main_module":"index.js","compatibility_date":"2026-09-08",
            "observability":{"enabled":false,"head_sampling_rate":sampling}});
        let body = format!(
            "--cf-version-boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n--cf-version-boundary\r\nContent-Disposition: form-data; name=\"index.js\"; filename=\"index.js\"\r\nContent-Type: application/javascript+module\r\n\r\nexport default {{fetch(){{return new Response('candidate')}}}};\r\n--cf-version-boundary--\r\n"
        );
        let request = Request::builder()
            .method("POST")
            .uri(format!("{base}/versions"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .header(
                "content-type",
                "multipart/form-data; boundary=cf-version-boundary",
            )
            .body(Body::from(body))
            .unwrap();
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), status);
        let bytes = to_bytes(Body::new(response.into_body()), 64 * 1024)
            .await
            .unwrap();
        assert_clean_output(&bytes);
        let response = client.request(settings()).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            to_bytes(Body::new(response.into_body()), 64 * 1024)
                .await
                .unwrap(),
            previous,
            "inactive Version upload changed Script observability"
        );
    }
    let response = client
        .request(
            Request::builder()
                .uri(format!("{base}/deployments"))
                .header("authorization", format!("Bearer {READ_ONLY_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let result: Value = serde_json::from_slice(
        &to_bytes(Body::new(response.into_body()), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        result["result"]["deployments"][0]["versions"][0]["version_id"], before,
        "inactive Version upload changed traffic"
    );
}

pub(super) async fn active_version(
    client: &platform_process::Client,
    admin_addr: SocketAddr,
    public_account: &str,
) -> String {
    let request = Request::builder()
        .uri(format!("http://{admin_addr}/client/v4/accounts/{public_account}/workers/scripts/p6-cf-resource-gate/versions"))
        .header("authorization", format!("Bearer {READ_ONLY_TOKEN}"))
        .body(Body::empty()).unwrap();
    let response = client.request(request).await.unwrap();
    assert_eq!(response.status(), 200);
    let bytes = to_bytes(Body::new(response.into_body()), 1024 * 1024)
        .await
        .unwrap();
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    envelope["result"]["items"]
        .as_array()
        .and_then(|versions| versions.first())
        .and_then(|version| version["id"].as_str())
        .unwrap()
        .to_owned()
}

pub(super) async fn wait_persisted_tail_log(
    client: &platform_process::Client,
    admin_addr: SocketAddr,
    public_account: &str,
) {
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let body = serde_json::to_vec(&serde_json::json!({
        "queryId": "p7-cf-tail-persistence",
        "timeframe": {"from": now - 5 * 60_000, "to": now + 60_000},
        "parameters": {"datasets": ["cloudflare-workers"], "filters": []},
        "view": "events",
        "limit": 2_000
    }))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "http://{admin_addr}/client/v4/accounts/{public_account}/workers/observability/telemetry/query"
            ))
            .header("authorization", format!("Bearer {READ_ONLY_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), 200);
        let bytes = to_bytes(Body::new(response.into_body()), 8 * 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let encoded = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!encoded.contains(TAIL_SECRET));
        if encoded.contains("p7-tail-event") {
            assert!(encoded.contains("REDACTED"));
            let events = value["result"]["events"]["events"].as_array().unwrap();
            assert!(
                events
                    .iter()
                    .all(|event| event["dataset"] == "cloudflare-workers")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Workers Logs persistence did not commit the tail invocation"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(super) async fn tail_count(
    client: &platform_process::Client,
    admin_addr: SocketAddr,
    public_account: &str,
) -> usize {
    let request = Request::builder()
        .uri(format!(
            "http://{admin_addr}/client/v4/accounts/{public_account}/workers/scripts/p6-cf-resource-gate/tails"
        ))
        .header("authorization", format!("Bearer {READ_ONLY_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(3), client.request(request))
        .await
        .expect("tail session list timed out")
        .expect("tail session list failed");
    assert_eq!(response.status(), 200);
    let bytes = to_bytes(Body::new(response.into_body()), 64 * 1024)
        .await
        .unwrap();
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    envelope["result"].as_array().unwrap().len()
}
