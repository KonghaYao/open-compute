import assert from "node:assert/strict";
import test from "node:test";
import {
  compileRuntime,
  importRuntime,
  moduleUrl,
} from "../compiled-runtime.mjs";

const { authorityFromHeaders, cancelOrderedOperation, ordered } =
  await importRuntime("durable-objects/host-protocol.ts", {
    "./identity.js": moduleUrl(
      await compileRuntime("durable-objects/identity.ts"),
    ),
    "../loader/shared.js": moduleUrl(
      "export const bindingError = (code) => new Error(code);",
    ),
  });

test("DO host authority accepts only the canonical instance ID", () => {
  const headers = new Headers({
    "x-open-compute-instance-id": "019c0000000070008000000000000001",
    "x-open-compute-worker-id": "019c0000-0000-7000-8000-000000000002",
    "x-open-compute-version-id": "019c0000-0000-7000-8000-000000000003",
    "x-open-compute-worker-code-sha256": "a".repeat(64),
    "x-open-compute-object-id": "b".repeat(64),
    "x-open-compute-namespace-resource-id":
      "019c0000-0000-7000-8000-000000000004",
    "x-open-compute-class-name": "Object",
    "x-open-compute-route-generation": "1",
    "x-open-compute-object-generation": "1",
  });
  assert.equal(
    authorityFromHeaders(headers).instanceId,
    "019c0000000070008000000000000001",
  );
  headers.set(
    "x-open-compute-instance-id",
    "019c0000-0000-7000-8000-000000000001",
  );
  assert.throws(
    () => authorityFromHeaders(headers),
    /DO_INTERNAL_PROTOCOL_ERROR/,
  );
});

test("cancelOrderedOperation skips a queued sequence without blocking the channel", async () => {
  const states = new Map();
  const channelId = "a".repeat(32);
  let firstStarted;
  const firstGate = new Promise((resolve) => {
    firstStarted = resolve;
  });
  const first = ordered(states, { channelId, sequence: 0 }, async () => {
    await firstGate;
    return "first";
  });
  const second = ordered(
    states,
    { channelId, sequence: 1 },
    async () => "second",
  );
  await new Promise((resolve) => setImmediate(resolve));
  cancelOrderedOperation(states, { channelId, sequence: 1 });
  firstStarted();
  assert.equal(await first, "first");
  const third = await ordered(
    states,
    { channelId, sequence: 2 },
    async () => "third",
  );
  assert.equal(third, "third");
  await second.catch(() => undefined);
});

test("ordered rejects duplicate pending sequences with DO_RUNTIME_EXCEPTION", async () => {
  const states = new Map();
  const channelId = "b".repeat(32);
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  void ordered(states, { channelId, sequence: 0 }, async () => {
    await gate;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.throws(
    () => ordered(states, { channelId, sequence: 0 }, async () => undefined),
    /DO_RUNTIME_EXCEPTION/,
  );
  release();
});
