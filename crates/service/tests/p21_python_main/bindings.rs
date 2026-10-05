//! JavaScript peer against the same persisted KV, D1 and R2 authority as Python.

use super::SCRIPT;
use super::python_support::capture::Bindings;
use super::python_support::fixture::Fixture;
use super::python_support::fixture::RequestTarget;
use open_compute_service::http::REQUEST_ID_HEADER;
use serde_json::{Value, json};

const PEER: &str = "python-main-javascript";
const SOURCE: &str = r#"
export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname;
    if (path === '/write') {
      const value = env.REVISION;
      await env.KV.put('value', value);
      await env.DB.prepare('CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL)').run();
      await env.DB.prepare("INSERT INTO state (key, value) VALUES ('value', ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(value).run();
      await env.BUCKET.put('value', value);
      return Response.json({written: value});
    }
    if (path === '/delete') {
      await env.KV.delete('value');
      await env.DB.prepare("DELETE FROM state WHERE key='value'").run();
      await env.BUCKET.delete('value');
      return Response.json({deleted: true});
    }
    if (path === '/read') {
      const kv = await env.KV.get('value');
      const d1 = await env.DB.prepare("SELECT value FROM state WHERE key='value'").first('value');
      const object = await env.BUCKET.get('value');
      const r2 = object === null ? null : await object.text();
      return Response.json({kv, d1, r2, revision: env.REVISION});
    }
    return new Response('missing route', {status: 404});
  }
};
"#;

pub(super) async fn verify_shared_bindings(fixture: &Fixture, bindings: &Bindings<'_>) -> String {
    let version = fixture
        .upload_javascript(
            PEER,
            SOURCE,
            &[
                json!({"name":"KV","type":"kv_namespace","namespace_id":bindings.kv}),
                json!({"name":"DB","type":"d1","id":bindings.d1}),
                json!({"name":"BUCKET","type":"r2_bucket","bucket_name":bindings.r2}),
                json!({"name":"REVISION","type":"plain_text","text":"javascript"}),
            ],
            None,
            None,
        )
        .await;
    fixture.promote(PEER, &version).await;

    assert_peer_state(fixture, Some("first")).await;
    assert_eq!(
        fixture.invoke(PEER, "/write").await,
        json!({"written":"javascript"})
    );
    assert_eq!(
        fixture.invoke(SCRIPT, "/read").await,
        json!({"kv":"javascript","d1":"javascript","r2":"javascript","revision":"first"})
    );
    assert_peer_state(fixture, Some("javascript")).await;
    assert_eq!(
        fixture.invoke(PEER, "/delete").await,
        json!({"deleted":true})
    );
    assert_peer_state(fixture, None).await;
    assert_eq!(
        fixture.invoke(SCRIPT, "/read").await,
        json!({"kv":null,"d1":null,"r2":null,"revision":"first"})
    );
    // An empty bucket avoids accepting an unrelated bucket-not-empty conflict
    // as proof that the retained binding prevents resource deletion.
    assert_bound_resources_retained(fixture, bindings).await;
    assert_eq!(fixture.invoke(SCRIPT, "/write").await["written"], "first");
    assert_peer_state(fixture, Some("first")).await;
    version
}

pub(super) async fn assert_peer_state(fixture: &Fixture, stored: Option<&str>) {
    assert_eq!(
        fixture.invoke(PEER, "/read").await,
        json!({"kv":stored,"d1":stored,"r2":stored,"revision":"javascript"})
    );
}

pub(super) async fn assert_bound_resources_retained(fixture: &Fixture, bindings: &Bindings<'_>) {
    let authority = || {
        let connection = fixture.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, state, spec_generation, updated_at_ms, deleted_at_ms,
                    (SELECT count(*) FROM resource_referrers WHERE resource_id = resources.id)
                 FROM resources WHERE kind IN ('kv_namespace', 'd1_database', 'r2_bucket')
                 ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok(json!({
                    "id":row.get::<_, String>(0)?,
                    "state":row.get::<_, String>(1)?,
                    "generation":row.get::<_, i64>(2)?,
                    "updated":row.get::<_, i64>(3)?,
                    "deleted":row.get::<_, Option<i64>>(4)?,
                    "referrers":row.get::<_, i64>(5)?,
                }))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let before = authority();
    assert_eq!(before.len(), 3);
    for resource in &before {
        assert_eq!(resource["state"], "ready");
        assert!(resource["referrers"].as_i64().unwrap() > 0);
    }
    let objects = fixture.mock.object_count();
    let active = fixture.active(SCRIPT);
    for suffix in [
        format!("storage/kv/namespaces/{}", bindings.kv),
        format!("d1/database/{}", bindings.d1),
        format!("r2/buckets/{}", bindings.r2),
    ] {
        let path = format!("/client/v4/accounts/{}/{suffix}", fixture.public_account);
        let (status, headers, bytes) = fixture
            .request(
                &path,
                "DELETE",
                "application/json",
                Vec::new(),
                RequestTarget::Admin,
            )
            .await;
        assert_eq!(status, 409, "retained bindings must prevent deletion");
        assert!(headers.contains_key(REQUEST_ID_HEADER));
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response["success"], false);
        assert!(response["result"].is_null());
        assert_eq!(response["errors"][0]["code"], 9_100_006);
        assert_eq!(authority(), before);
        assert_eq!(fixture.active(SCRIPT), active);
        assert_eq!(fixture.mock.object_count(), objects);
    }
}
