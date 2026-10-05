import assert from "node:assert/strict";
import test from "node:test";
import { importRuntime } from "../compiled-runtime.mjs";

test("asset facade preserves fetch URL, method, and headers across the RPC boundary", async () => {
  const calls = [];
  const { Fetcher } = await importRuntime("assets/facade.ts");
  const binding = new Fetcher({
    async fetchAsset(request) {
      calls.push(request);
      return new Response("asset");
    },
  });
  const response = await binding.fetch(
    "https://assets.example.test/static.txt",
    {
      method: "HEAD",
      headers: { "if-none-match": '"digest"' },
    },
  );
  // The official Python SDK selects its Fetcher wrapper by this public name.
  assert.equal(binding.constructor.name, "Fetcher");
  assert.equal(await response.text(), "asset");
  assert.deepEqual(calls, [
    {
      url: "https://assets.example.test/static.txt",
      method: "HEAD",
      headers: [["if-none-match", '"digest"']],
    },
  ]);
  assert.throws(() => new Fetcher({}), /ASSET_BINDING_UNAVAILABLE/);
});
