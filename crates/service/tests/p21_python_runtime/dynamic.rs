//! Current formal-pin negative baseline, kept separate from ordinary prepared execution.

use super::python_support::fixture::Fixture;
use serde_json::json;
use std::time::Instant;

pub(super) const SCRIPT: &str = "python-dynamic-baseline";
const SOURCE: &str = r#"
import { loadWorker, getWorker } from "open-compute:worker-loader";
let factories = 0;
const python = `from workers import WorkerEntrypoint
from js import Response
CALLS = 0
class Default(WorkerEntrypoint):
    async def fetch(self, request):
        global CALLS
        CALLS += 1
        return Response.new("dynamic:" + str(CALLS))
`;
function code() {
  return {compatibilityDate: "2026-09-08", mainModule: "child.py",
    modules: {"child.py": {py: python}}, globalOutbound: null};
}
export default {
  async fetch(request, env) {
    if (new URL(request.url).pathname === "/healthy") return Response.json({healthy: true});
    const attempts = [];
    const factoriesBefore = factories;
    for (const mode of ["new", "new", "cached-key", "cached-key"]) {
      const start = Date.now();
      try {
        const child = mode === "new" ? loadWorker(env.LOADER, code()) :
          getWorker(env.LOADER, "minimal-python", () => { ++factories; return code(); });
        const response = await child.getEntrypoint().fetch("https://dynamic.invalid/");
        attempts.push({mode, success: true, body: await response.text(), wallMillis: Date.now()-start});
      } catch (error) {
        attempts.push({mode, success: false, errorType: error.name, errorMessage: String(error.message), wallMillis: Date.now()-start});
      }
    }
    return Response.json({attempts, factories, factoriesBefore});
  }
};
"#;

pub(super) async fn deploy(fixture: &Fixture) -> String {
    let version = fixture
        .upload_javascript(
            SCRIPT,
            SOURCE,
            &[json!({"name": "LOADER", "type": "worker_loader"})],
            None,
            None,
        )
        .await;
    fixture.promote(SCRIPT, &version).await;
    version
}

pub(super) async fn baseline(fixture: &Fixture, phase: &str) {
    let start = Instant::now();
    let result = fixture.invoke(SCRIPT, "/baseline").await;
    let attempts = result["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 4);
    for (attempt, mode) in attempts
        .iter()
        .zip(["new", "new", "cached-key", "cached-key"])
    {
        assert_eq!(attempt["mode"], mode);
        assert_eq!(
            attempt["success"], false,
            "formal-pin Dynamic limitation changed: {attempt}"
        );
        assert!(attempt["errorType"].as_str().is_some());
        assert!(
            attempt["errorMessage"]
                .as_str()
                .unwrap()
                .contains("Worker exceeded CPU time limit."),
            "{attempt}"
        );
    }
    assert_eq!(
        result["factories"].as_u64().unwrap() - result["factoriesBefore"].as_u64().unwrap(),
        2
    );
    assert_eq!(
        fixture.invoke(SCRIPT, "/healthy").await,
        json!({"healthy": true})
    );
    println!(
        "python-dynamic-baseline: {}",
        json!({"phase": phase, "result": result, "wallMillis": start.elapsed().as_millis(), "startupCpuLimitMillis": 1000, "startupCpuMeasuredSeparately": false, "warmedSuccessfulChild": "unavailable: all initialization attempts failed", "preparedArtifactInjected": false})
    );
}
