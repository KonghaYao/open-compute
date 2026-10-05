import assert from "node:assert/strict";
import test from "node:test";
import { importRuntime } from "../compiled-runtime.mjs";

const { moduleValue } = await importRuntime("loader/module-values.ts");
const module = (type, source) => ({
  name: "entry.py",
  type,
  bytesBase64: Buffer.from(source).toString("base64"),
});

test("official Loader module shapes preserve source and package bytes", () => {
  const source = "message = '你好'\n";
  for (const [type, key] of [
    ["python", "py"],
    ["esModule", "js"],
    ["commonJsModule", "cjs"],
    ["text", "text"],
  ]) {
    assert.deepEqual(moduleValue(module(type, source)), { [key]: source });
    assert.throws(() => moduleValue(module(type, [255])), /encoded data/);
  }
  const data = Uint8Array.from([0, 255, 42]);
  assert.deepEqual(
    new Uint8Array(moduleValue(module("data", data)).data),
    data,
  );
  assert.deepEqual(moduleValue(module("wasm", data)).wasm, data);
  assert.deepEqual(moduleValue(module("json", '{"value":42}')), {
    json: { value: 42 },
  });
  assert.throws(() => moduleValue(module("json", "{")), SyntaxError);
  assert.throws(
    () => moduleValue(module("python-requirement", "example")),
    /unsupported module representation/,
  );
});
