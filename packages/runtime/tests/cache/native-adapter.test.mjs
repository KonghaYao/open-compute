import assert from "node:assert/strict";
import test from "node:test";
import { gzipSync } from "node:zlib";
import { importRuntime, moduleUrl } from "../compiled-runtime.mjs";

const shared = moduleUrl(`
  export const bindingError = code => Object.assign(new Error(code), { stableCode: code });
`);
const { nativeCacheRequest, nativeCacheResponse } = await importRuntime(
  "cache/native-adapter.ts",
  {
    "../loader/shared.js": shared,
  },
);
const bytes = (value) =>
  typeof value === "string" ? new TextEncoder().encode(value) : value;
const request = (chunks, options = {}) =>
  new Request("https://cache.test/key", {
    method: "PUT",
    duplex: "half",
    ...options,
    body: new ReadableStream({
      start(controller) {
        for (const part of chunks) controller.enqueue(bytes(part));
        controller.close();
      },
    }),
  });

test("native Cache payload preserves compressed bytes across split headers", async () => {
  const compressed = gzipSync("compressed-cache-body");
  const response = await nativeCacheResponse(
    request([
      "HTTP/1.1 200 OK\r\ncontent-encoding: gzip\r\ncontent-length: ",
      String(compressed.length),
      "\r\nset-cookie: a=1\r\nset-cookie: b=2\r\n\r",
      "\n",
      compressed,
    ]),
  );
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-encoding"), "gzip");
  assert.deepEqual(response.headers.getSetCookie(), ["a=1", "b=2"]);
  assert.deepEqual(Buffer.from(await response.arrayBuffer()), compressed);
});

test("native chunked header carries unframed streaming bytes", async () => {
  const response = await nativeCacheResponse(
    request([
      "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nraw-",
      "body",
    ]),
  );
  assert.equal(response.headers.get("transfer-encoding"), null);
  assert.equal(response.headers.get("connection"), null);
  assert.equal(await response.text(), "raw-body");
});

test("native empty-status payload validates framing without constructing a body", async () => {
  for (const status of [204, 205, 304]) {
    const response = await nativeCacheResponse(
      request([`HTTP/1.1 ${status} Empty\r\nContent-Length: 0\r\n\r\n`]),
    );
    assert.equal(response.status, status);
    assert.equal(response.body, null);
  }
  await assert.rejects(
    nativeCacheResponse(request(["HTTP/1.1 204 Empty\r\n\r\nnot-empty"])),
    /CACHE_PROTOCOL_ERROR/,
  );
});

test("native payload rejects malformed status, headers, framing, and oversized headers", async () => {
  for (const payload of [
    "",
    "HTTP/1.1 200 OK\r\ntruncated",
    "HTTP/1.1 101 Upgrade\r\n\r\n",
    "HTTP/1.0 200 OK\r\n\r\n",
    "HTTP/1.1 200 OK\r\nBad Header: value\r\n\r\n",
    "HTTP/1.1 200 OK\r\nfolded: good\r\n bad\r\n\r\n",
    "HTTP/1.1 200 OK\r\nx-bad: \0private\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 01\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 9999999999999999\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n",
    new Uint8Array([
      72, 84, 84, 80, 47, 49, 46, 49, 32, 50, 48, 48, 32, 255, 13, 10, 13, 10,
    ]),
  ])
    await assert.rejects(
      nativeCacheResponse(request([payload])),
      /CACHE_PROTOCOL_ERROR/,
    );
  await assert.rejects(
    nativeCacheResponse(
      request(["HTTP/1.1 200 OK\r\nx-long: " + "x".repeat(65536)]),
    ),
    /CACHE_LIMIT_EXCEEDED/,
  );
  await assert.rejects(
    nativeCacheResponse(new Request("https://cache.test/key")),
    /CACHE_PROTOCOL_ERROR/,
  );
});

test("native body lengths reject truncation and excess while cancelling the producer", async () => {
  for (const body of ["short", "too-long-body"]) {
    const response = await nativeCacheResponse(
      request(["HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n", body]),
    );
    await assert.rejects(response.text(), /CACHE_PROTOCOL_ERROR/);
  }
  let cancelled = 0;
  const input = new Request("https://cache.test/key", {
    method: "PUT",
    duplex: "half",
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(bytes("HTTP/1.1 200 OK\r\n\r\nbody"));
      },
      cancel() {
        cancelled++;
      },
    }),
  });
  const response = await nativeCacheResponse(input);
  await response.body.cancel();
  assert.equal(cancelled, 1);
});

test("native header and body stream errors remain sanitized", async () => {
  for (const afterHeader of [false, true]) {
    let calls = 0;
    const input = new Request("https://cache.test/key", {
      method: "PUT",
      duplex: "half",
      body: new ReadableStream({
        pull(controller) {
          if (afterHeader && calls++ === 0)
            controller.enqueue(bytes("HTTP/1.1 200 OK\r\n\r\n"));
          else controller.error(new Error("private-upstream-location"));
        },
      }),
    });
    if (afterHeader) {
      const response = await nativeCacheResponse(input);
      await assert.rejects(response.text(), /^Error: CACHE_PROTOCOL_ERROR$/);
    } else
      await assert.rejects(
        nativeCacheResponse(input),
        /^Error: CACHE_PROTOCOL_ERROR$/,
      );
  }
});

test("native Cache operations route default and Unicode named namespaces to one authority", async () => {
  const calls = [];
  const authority = {
    async match(...args) {
      calls.push(["match", ...args]);
      return { status: "HIT", response: new Response("hit", { status: 504 }) };
    },
    async storeEncoded(...args) {
      calls.push(["put", ...args.slice(0, 3), await args[3].text()]);
    },
    async delete(...args) {
      calls.push(["delete", ...args]);
      return true;
    },
  };
  const hit = await nativeCacheRequest(
    authority,
    new Request("https://cache.test/key"),
  );
  assert.equal(hit.status, 504);
  assert.equal(hit.headers.get("cf-cache-status"), "HIT");
  assert.equal(await hit.text(), "hit");
  const name = "reports:中文 /";
  const put = await nativeCacheRequest(
    authority,
    request(["HTTP/1.1 200 OK\r\n\r\nstored"], {
      headers: { "cf-cache-namespace": encodeURIComponent(name) },
    }),
  );
  assert.equal(put.status, 204);
  const deleted = await nativeCacheRequest(
    authority,
    new Request("https://cache.test/key", {
      method: "PURGE",
      headers: { "cf-cache-namespace": encodeURIComponent(name) },
    }),
  );
  assert.equal(deleted.status, 200);
  assert.deepEqual(
    calls.map(([operation, namespace, name, key]) => [
      operation,
      namespace,
      name,
      key.url,
      key.method,
    ]),
    [
      ["match", "default", undefined, "https://cache.test/key", "GET"],
      ["put", "named", name, "https://cache.test/key", "GET"],
      ["delete", "named", name, "https://cache.test/key", "GET"],
    ],
  );
  assert.equal(calls[1][4], "stored");
});

test("native miss, expiration, and absent delete map to Cache HTTP results", async () => {
  for (const status of ["MISS", "EXPIRED"]) {
    const response = await nativeCacheRequest(
      { match: async () => ({ status }) },
      new Request("https://cache.test/key"),
    );
    assert.equal(response.status, 504);
    assert.equal(response.headers.get("cf-cache-status"), status);
  }
  const response = await nativeCacheRequest(
    { delete: async () => false },
    new Request("https://cache.test/key", { method: "PURGE" }),
  );
  assert.equal(response.status, 404);
  await assert.rejects(
    nativeCacheRequest(
      {},
      new Request("https://cache.test/key", { method: "POST" }),
    ),
    /CACHE_PROTOCOL_ERROR/,
  );
  await assert.rejects(
    nativeCacheRequest(
      {},
      new Request("https://cache.test/key", {
        headers: { "cf-cache-namespace": "%zz" },
      }),
    ),
    /CACHE_KEY_INVALID/,
  );
  await assert.rejects(
    nativeCacheRequest(
      { match: async () => ({ status: "UNKNOWN" }) },
      new Request("https://cache.test/key"),
    ),
    /CACHE_PROTOCOL_ERROR/,
  );
  let cancelled = false;
  await assert.rejects(
    nativeCacheRequest(
      {
        match: async () => ({
          status: "MISS",
          response: new Response(
            new ReadableStream({
              cancel() {
                cancelled = true;
              },
            }),
          ),
        }),
      },
      new Request("https://cache.test/key"),
    ),
    /CACHE_PROTOCOL_ERROR/,
  );
  assert.equal(cancelled, true);
});

test("failed cache writes cancel the serialized response stream", async () => {
  let cancelled = 0;
  const input = new Request("https://cache.test/key", {
    method: "PUT",
    duplex: "half",
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(bytes("HTTP/1.1 200 OK\r\n\r\nbody"));
      },
      cancel() {
        cancelled++;
      },
    }),
  });
  await assert.rejects(
    nativeCacheRequest(
      {
        storeEncoded: async () => {
          throw new Error("CACHE_CORRUPT");
        },
      },
      input,
    ),
    /CACHE_CORRUPT/,
  );
  assert.equal(cancelled, 1);
});
