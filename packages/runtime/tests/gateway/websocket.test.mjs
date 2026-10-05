import assert from "node:assert/strict";
import test from "node:test";
import { Ingress } from "./runtime.mjs";

test("private ingress forwards authenticated WebSocket upgrades only on dispatch", async () => {
  const forwarded = [];
  const env = {
    INTERNAL_TOKEN: "test-generation",
    LOADER_HOST: {
      async fetch(request) {
        forwarded.push(request);
        return new Response("accepted");
      },
    },
  };
  const gateway = new Ingress({}, env);
  const make = (path, token, upgrade = "websocket") =>
    new Request(`http://private${path}`, {
      headers: { "x-open-compute-internal-token": token, upgrade },
    });
  for (const request of [
    make("/internal/dispatch", "old-generation"),
    make("/internal/dispatch?extra=1", "test-generation"),
    make("/internal/validate", "test-generation"),
    make("/internal/dispatch", "test-generation", "other"),
  ])
    assert.equal((await gateway.fetch(request)).status, 404);
  assert.equal(forwarded.length, 0);
  assert.equal(
    (await gateway.fetch(make("/internal/dispatch", "test-generation"))).status,
    200,
  );
  assert.equal(forwarded[0].method, "GET");
  assert.equal(forwarded[0].headers.get("upgrade"), "websocket");
});
