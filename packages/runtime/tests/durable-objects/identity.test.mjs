import assert from "node:assert/strict";
import test from "node:test";
import { importRuntime } from "../compiled-runtime.mjs";

const {
  objectIdentity,
  encodeObjectIdentity,
  identityFromHeaders,
  OBJECT_IDENTITY_HEADER,
} = await importRuntime("durable-objects/identity.ts");
const value = "a".repeat(64);

test("named and jurisdiction IDs cross ASCII headers without losing Unicode", () => {
  const identity = { value, name: "µ☁/\r\n", jurisdiction: "eu" };
  const headers = new Headers({
    [OBJECT_IDENTITY_HEADER]: encodeObjectIdentity(identity),
  });
  assert.deepEqual(identityFromHeaders(headers, value), identity);
  assert.equal(Object.isFrozen(objectIdentity(identity)), true);
  assert.deepEqual(identityFromHeaders(new Headers(), value), {
    value,
    name: undefined,
    jurisdiction: undefined,
  });
});

test("malformed, oversized and mismatched identities fail closed", () => {
  for (const input of [
    null,
    [],
    {},
    { value: "invalid" },
    { value, name: 1 },
    { value, name: "☁".repeat(342) },
    { value, jurisdiction: "unknown" },
  ]) {
    assert.throws(() => objectIdentity(input), /DO_ID_INVALID/);
  }
  for (const wire of [
    "%",
    "null",
    "x".repeat(10_001),
    encodeObjectIdentity({
      value: "b".repeat(64),
      name: undefined,
      jurisdiction: undefined,
    }),
  ]) {
    assert.throws(
      () =>
        identityFromHeaders(
          new Headers({ [OBJECT_IDENTITY_HEADER]: wire }),
          value,
        ),
      /DO_ID_INVALID/,
    );
  }
  assert.equal(
    objectIdentity({ value, name: "☁".repeat(341) }).name.length,
    341,
  );
});
