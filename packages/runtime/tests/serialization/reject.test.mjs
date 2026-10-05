import assert from "node:assert/strict";
import test from "node:test";
import { codec, encode } from "./load.mjs";

function rejects(value, code = "WORKFLOW_SERIALIZATION_UNSUPPORTED") {
  assert.throws(() => encode(value), { name: "Error", message: code });
  assert.equal(
    codec.durableValueErrorCode(
      (() => {
        try {
          encode(value);
        } catch (error) {
          return error;
        }
      })(),
    ),
    code,
  );
}

test("unsupported values fail closed with Workflow codes and are not coerced", () => {
  class Box {}
  class SubArray extends Array {}
  class SubMap extends Map {}
  const fn = () => "nope";
  for (const value of [
    Symbol("x"),
    fn,
    { [Symbol("s")]: 1 },
    {
      get x() {
        return 1;
      },
    },
    {
      set x(value) {
        void value;
      },
    },
    new Box(),
    new SubArray(1),
    new SubMap([[1, 2]]),
    new Number(1),
    new String("x"),
    new Boolean(true),
    Object(1n),
    Promise.resolve(1),
    new WeakMap(),
    new WeakSet(),
    new Request("https://example.com/"),
    new Response("x"),
    new Headers({ a: "b" }),
    new ReadableStream(),
    new WritableStream(),
    new TransformStream(),
    new URL("https://example.com/"),
  ]) {
    rejects(value);
  }
  const accessor = {};
  Object.defineProperty(accessor, "x", { get: fn, enumerable: true });
  rejects(accessor);
  const method = { a: 1, toJSON: fn };
  rejects(method);
  const arrayMethod = [1];
  arrayMethod.push(fn);
  rejects(arrayMethod);
  rejects(new Map([[fn, 1]]));
  rejects(new Set([Symbol("x")]));
  rejects({ nested: { fn } });
});

test("unsafe buffers, transferables, and host streams are rejected before persistence", () => {
  if (typeof SharedArrayBuffer === "function") {
    try {
      rejects(new SharedArrayBuffer(8));
      rejects(new Uint8Array(new SharedArrayBuffer(8)));
    } catch (error) {
      if (
        !(error instanceof Error) ||
        !/SharedArrayBuffer|secure context/.test(error.message)
      )
        throw error;
    }
  }
  const resizable = new ArrayBuffer(8, { maxByteLength: 16 });
  if (resizable.resizable) {
    rejects(resizable);
    rejects(new Uint8Array(resizable));
    rejects(new DataView(resizable));
  }
  const buffer = new ArrayBuffer(8);
  if (typeof buffer.transfer === "function") {
    const detached = buffer.transfer();
    void detached;
    rejects(buffer);
    try {
      rejects(new Uint8Array(buffer));
    } catch {
      rejects(buffer);
    }
  }
});

test("over-depth and over-node bounds use the Workflow too-large code", () => {
  const limits = codec.DURABLE_VALUE_LIMITS;
  let deep = null;
  for (let index = 0; index < limits.maxDepth; index++) deep = [deep];
  encode(deep);
  assert.throws(() => encode([deep]), {
    message: "WORKFLOW_RESULT_TOO_LARGE",
  });
  const nodes = Array.from({ length: 30_000 }, () => ({}));
  encode(nodes);
});
