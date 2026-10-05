use super::*;

pub(super) async fn exercise_kv(command: &CfCommand<'_>, project: &Path) {
    assert_success(
        &command
            .run(&["kv", "namespaces", "create", "--title", KV_NAME])
            .await,
    );
    let listed = command.run(&["kv", "namespaces", "list"]).await;
    assert_success(&listed);
    let namespaces = json_stdout(&listed);
    let id = namespaces
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["title"] == KV_NAME)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    write_config(project, command.account_id, Some(id), None);
    assert_success(
        &command
            .run(&[
                "kv",
                "keys",
                "put",
                "greeting",
                "--body",
                "你好 🌍",
                "--namespace-id",
                id,
            ])
            .await,
    );
    let keys = command
        .run(&["kv", "keys", "list", "--namespace-id", id])
        .await;
    assert_success(&keys);
    assert!(json_contains(
        &json_stdout(&keys),
        "name",
        &Value::from("greeting")
    ));
    let value = command
        .run(&["kv", "keys", "get", "greeting", "--namespace-id", id])
        .await;
    assert_success(&value);
    assert_eq!(value.stdout, "你好 🌍".as_bytes());
    assert_success(
        &command
            .run(&[
                "kv",
                "keys",
                "delete",
                "greeting",
                "--namespace-id",
                id,
                "--force",
            ])
            .await,
    );
    let keys = command
        .run(&["kv", "keys", "list", "--namespace-id", id])
        .await;
    assert_success(&keys);
    assert!(!json_contains(
        &json_stdout(&keys),
        "name",
        &Value::from("greeting")
    ));
    assert_success(
        &command
            .run(&["kv", "namespaces", "delete", id, "--force"])
            .await,
    );
    let remaining = command.run(&["kv", "namespaces", "list"]).await;
    assert_success(&remaining);
    assert!(!json_contains(
        &json_stdout(&remaining),
        "title",
        &Value::from(KV_NAME)
    ));
}

pub(super) async fn exercise_d1(command: &CfCommand<'_>, project: &Path) {
    assert_success(&command.run(&["d1", "create", "--name", D1_NAME]).await);
    let listed = command.run(&["d1", "list"]).await;
    assert_success(&listed);
    let databases = json_stdout(&listed);
    let id = databases
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == D1_NAME)
        .unwrap()["uuid"]
        .as_str()
        .unwrap();
    write_config(project, command.account_id, None, Some(id));
    fs::create_dir(project.join("migrations")).unwrap();
    fs::write(
        project.join("migrations/0001_create_items.sql"),
        "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
    )
    .unwrap();
    let info = command.run(&["d1", "get", id]).await;
    assert_success(&info);
    assert!(json_contains(
        &json_stdout(&info),
        "name",
        &Value::from(D1_NAME)
    ));
    let answer = command
        .run(&["d1", "query", id, "--sql", "SELECT 42 AS answer"])
        .await;
    assert_success(&answer);
    assert!(json_contains(
        &json_stdout(&answer),
        "answer",
        &Value::from(42)
    ));
    assert_success(
        &command
            .run(&["d1", "migrations", "apply", id, "--dir", "migrations"])
            .await,
    );
    let migrated = command
        .run(&[
            "d1",
            "query",
            id,
            "--sql",
            "SELECT name FROM sqlite_master WHERE type='table' AND name='items'",
        ])
        .await;
    assert_success(&migrated);
    assert!(json_contains(
        &json_stdout(&migrated),
        "name",
        &Value::from("items")
    ));
    assert_success(&command.run(&["d1", "delete", id, "--force"]).await);
    let remaining = command.run(&["d1", "list"]).await;
    assert_success(&remaining);
    assert!(!json_contains(
        &json_stdout(&remaining),
        "name",
        &Value::from(D1_NAME)
    ));
}

pub(super) async fn exercise_r2(
    command: &CfCommand<'_>,
    project: &Path,
    public_addr: SocketAddr,
    internal_account: &str,
) {
    assert_success(
        &command
            .run(&["r2", "buckets", "create", "--name", R2_NAME])
            .await,
    );
    let listed = command.run(&["r2", "buckets", "list"]).await;
    assert_success(&listed);
    assert!(json_contains(
        &json_stdout(&listed),
        "name",
        &Value::from(R2_NAME)
    ));
    fs::write(project.join("r2-input.bin"), b"fixed-cf-r2\0payload").unwrap();
    assert_success(
        &command
            .run(&[
                "r2",
                "objects",
                "put",
                "folder/object.bin",
                "--bucket-name",
                R2_NAME,
                "--file",
                "r2-input.bin",
            ])
            .await,
    );
    let object = command
        .run(&[
            "r2",
            "objects",
            "get",
            "folder/object.bin",
            "--bucket-name",
            R2_NAME,
        ])
        .await;
    assert_success(&object);
    assert_eq!(object.stdout, b"fixed-cf-r2\0payload");
    assert_success(
        &command
            .run(&[
                "r2",
                "objects",
                "delete",
                "folder/object.bin",
                "--bucket-name",
                R2_NAME,
                "--force",
            ])
            .await,
    );
    let deleted = command
        .run(&[
            "r2",
            "objects",
            "get",
            "folder/object.bin",
            "--bucket-name",
            R2_NAME,
        ])
        .await;
    assert!(!deleted.status.success());
    assert_clean_output(&deleted.stderr);
    assert_success(
        &command
            .run(&["r2", "buckets", "delete", R2_NAME, "--force"])
            .await,
    );
    assert_success(
        &command
            .run(&["r2", "buckets", "create", "--name", R2_NAME])
            .await,
    );
    write_config(project, command.account_id, None, None);
    let path = project.join("cloudflare-input.production.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["worker"]["env"]["BUCKET"] = serde_json::json!({"type":"r2", "name":R2_NAME});
    fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    fs::write(project.join("index.ts"), "export default { async fetch(request, env) { if (new URL(request.url).pathname === '/write') { await env.BUCKET.put('recreated.txt', 'fresh'); return new Response('fresh'); } const object = await env.BUCKET.get('recreated.txt'); return new Response(object ? await object.text() : 'missing'); } };").unwrap();
    assert_success(&command.run(&["deploy", "--mode", "production"]).await);
    assert_recreated_r2_worker(public_addr, internal_account, "/write", "fresh").await;
}

pub(super) async fn assert_recreated_r2_worker(
    public_addr: SocketAddr,
    internal_account: &str,
    path: &str,
    expected: &str,
) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let response = client
        .request(
            Request::builder()
                .uri(format!("http://{public_addr}{path}"))
                .header("host", worker_host(internal_account, "p6-cf-resource-gate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let bytes = to_bytes(Body::new(response.into_body()), 1024)
        .await
        .unwrap();
    assert_eq!(std::str::from_utf8(&bytes).unwrap(), expected);
}

pub(super) async fn exercise_queues(command: &CfCommand<'_>) {
    assert_success(
        &command
            .run(&["queues", "create", "--queue-name", QUEUE_NAME])
            .await,
    );
    let listed = command.run(&["queues", "list"]).await;
    assert_success(&listed);
    let queues = json_stdout(&listed);
    let id = queues
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["queue_name"] == QUEUE_NAME)
        .unwrap()["queue_id"]
        .as_str()
        .unwrap();
    assert_success(&command.run(&["queues", "delete", id, "--force"]).await);
    let remaining = command.run(&["queues", "list"]).await;
    assert_success(&remaining);
    assert!(!json_contains(
        &json_stdout(&remaining),
        "queue_name",
        &Value::from(QUEUE_NAME)
    ));
}

pub(super) async fn exercise_workflows(command: &CfCommand<'_>) {
    let listed = command.run(&["workflows", "list"]).await;
    assert_success(&listed);
    assert!(json_contains(
        &json_stdout(&listed),
        "name",
        &Value::from(WORKFLOW_NAME)
    ));
    let described = command.run(&["workflows", "get", WORKFLOW_NAME]).await;
    assert_success(&described);
    let described = String::from_utf8_lossy(&described.stdout);
    assert!(described.contains(WORKFLOW_NAME));
    assert!(described.contains("resource-gate-worker"));
    assert!(described.contains("ResourceFlow"));
    assert_success(
        &command
            .run(&["workflows", "delete", WORKFLOW_NAME, "--force"])
            .await,
    );
    let remaining = command.run(&["workflows", "list"]).await;
    assert_success(&remaining);
    assert!(!json_contains(
        &json_stdout(&remaining),
        "name",
        &Value::from(WORKFLOW_NAME)
    ));
}
