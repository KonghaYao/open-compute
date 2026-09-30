import assert from "node:assert/strict";
import test from "node:test";
import { importRuntime, moduleUrl } from "../compiled-runtime.mjs";

const { resolveSnapshot, snapshotWorkerCode } = await importRuntime(
  "loader/shared.ts",
  {
    "./snapshot.js": moduleUrl("export function assertSnapshot() {}"),
  },
);

test("dynamic Worker validation delegates experimental flags to workerd", () => {
  assert.deepEqual(
    snapshotWorkerCode({
      compatibilityDate: "2026-09-08",
      compatibilityFlags: ["experimental"],
      limits: { cpuMs: 30_000, subRequests: 1_000 },
    }),
    {
      compatibilityDate: "2026-09-08",
      compatibilityFlags: ["experimental"],
      allowExperimental: true,
      limits: { cpuMs: 30_000, subRequests: 1_000 },
    },
  );
});

test("runtime snapshot rejects a route generation changed during source resolution", async () => {
  let generation = 2;
  const requests = [];
  const env = {
    RUNTIME_SOURCE: {
      fetch: async (_url, init) => {
        requests.push(init);
        return Response.json({
          loaderKey:
            "019c0000000070008000000000000001/019c0000-0000-7000-8000-000000000002/019c0000-0000-7000-8000-000000000003",
          workerCodeSha256: "a".repeat(64),
          routeGeneration: generation,
        });
      },
    },
  };
  const envelope = {
    loaderKey:
      "019c0000000070008000000000000001/019c0000-0000-7000-8000-000000000002/019c0000-0000-7000-8000-000000000003",
    expected: "a".repeat(64),
    routeGeneration: 1,
  };
  await assert.rejects(
    resolveSnapshot(env, envelope, false, false, "generation"),
    /VERSION_INVARIANT_VIOLATION/,
  );
  generation = 1;
  assert.equal(
    (await resolveSnapshot(env, envelope, false, false, "generation"))
      .routeGeneration,
    1,
  );
  assert.equal(
    requests[0].headers["x-open-compute-startup-generation"],
    "generation",
  );
  assert.equal(JSON.parse(requests[0].body).startupGeneration, undefined);
});
