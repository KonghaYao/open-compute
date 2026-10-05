import assert from "node:assert/strict";
import test from "node:test";
import { importRuntime } from "../compiled-runtime.mjs";

const { sanitizeDoError } = await importRuntime("durable-objects/errors.ts");

test("native reset classification survives sanitization without upstream details", () => {
  const upstream = Object.assign(
    new Error("private-path DO_STORAGE_UNAVAILABLE"),
    {
      durableObjectReset: true,
      durableObjectId: "private-id",
    },
  );
  for (const code of [undefined, "DO_RUNTIME_EXCEPTION"]) {
    const safe = sanitizeDoError(upstream, code);
    assert.equal(safe.message, code ?? "DO_STORAGE_UNAVAILABLE");
    assert.equal(safe.stableCode, safe.message);
    assert.equal(safe.stack, "Error: " + safe.message);
    assert.equal(safe.durableObjectReset, true);
    assert.equal(safe.durableObjectId, undefined);
    assert.equal(JSON.stringify(safe).includes("private"), false);
  }
});

test("authority boundaries replace a tenant-selected error code without inventing reset metadata", () => {
  const ordinary = sanitizeDoError(
    new Error("DO_TENANT_SELECTED"),
    "DO_RUNTIME_EXCEPTION",
  );
  assert.equal(ordinary.message, "DO_RUNTIME_EXCEPTION");
  assert.equal(ordinary.durableObjectReset, undefined);
});

test("untrusted exceptions cannot run accessors or primitive conversion during sanitization", () => {
  const hostile = {
    get message() {
      throw Error("accessed message");
    },
    get durableObjectReset() {
      throw Error("accessed reset");
    },
    [Symbol.toPrimitive]() {
      throw Error("coerced exception");
    },
  };
  const proxy = new Proxy(
    {},
    {
      getOwnPropertyDescriptor() {
        throw Error("reflection denied");
      },
    },
  );
  for (const value of [
    null,
    undefined,
    false,
    "DO_SECRET",
    () => {},
    {},
    hostile,
    proxy,
    new Error("sensitive exception"),
    { message: 42, durableObjectReset: "true" },
  ]) {
    const safe = sanitizeDoError(value);
    assert.equal(safe.message, "DO_RUNTIME_EXCEPTION");
    assert.equal(safe.durableObjectReset, undefined);
  }
});
