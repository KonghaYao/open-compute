import assert from "node:assert/strict";
import {
  mkdir,
  mkdtemp,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import test from "node:test";
import { sha256 } from "../scripts/workerd-archive.ts";
import {
  completeAssetToken,
  PythonAssetCapture,
} from "./conformance/applications/python-assets-capture.ts";

const hash = "0123456789abcdef0123456789abcdef";
const source = "capture-static-file\n";

async function fixture(t) {
  await mkdir(".temp/python-assets-capture-unit", { recursive: true });
  const root = await mkdtemp(".temp/python-assets-capture-unit/run-");
  const assets = join(root, "assets");
  const captured = join(root, "captured");
  await mkdir(assets);
  await mkdir(captured);
  await writeFile(join(assets, "message.txt"), source);
  const capture = new PythonAssetCapture(
    assets,
    captured,
    "/session",
    "/upload",
    "capture-account-only",
  );
  const server = createServer(async (request, response) => {
    try {
      if (!(await capture.handle(request, response)))
        response.writeHead(404).end();
    } catch (error) {
      response.writeHead(400).end(error.message);
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}`;
  t.after(async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(root, { recursive: true });
  });
  return {
    root,
    assets,
    captured,
    capture,
    async session(
      manifest = { "/message.txt": { hash, size: Buffer.byteLength(source) } },
      token = "capture-account-only",
    ) {
      return fetch(`${url}/session`, {
        method: "POST",
        headers: {
          authorization: `Bearer ${token}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({ manifest }),
      });
    },
    async upload(token, parts = [[hash, source]]) {
      const form = new FormData();
      for (const [name, bytes] of parts) {
        form.append(
          name,
          new File([Buffer.from(bytes).toString("base64")], name, {
            type: "text/plain",
          }),
        );
      }
      return fetch(`${url}/upload?base64=true`, {
        method: "POST",
        headers: { authorization: `Bearer ${token}` },
        body: form,
      });
    },
  };
}

test("asset capture retains actual protocol and exact builder bytes", async (t) => {
  const f = await fixture(t);
  const session = await f.session();
  assert.equal(session.status, 200);
  const { result } = await session.json();
  assert.deepEqual(result.buckets, [[hash]]);
  assert.throws(() =>
    f.capture.assertComplete({ assets: { jwt: completeAssetToken } }),
  );
  const upload = await f.upload(result.jwt);
  assert.equal(upload.status, 200);
  assert.equal((await upload.json()).result.jwt, completeAssetToken);
  f.capture.assertComplete({ assets: { jwt: completeAssetToken } });
  assert.throws(() =>
    f.capture.assertComplete({ assets: { jwt: result.jwt } }),
  );
  const manifest = JSON.parse(
    await readFile(join(f.captured, "assets-session.json")),
  );
  assert.deepEqual(manifest.manifest, {
    "/message.txt": { hash, size: Buffer.byteLength(source) },
  });
  const wire = JSON.parse(
    await readFile(join(f.captured, "assets-upload.json")),
  );
  assert.equal(
    wire.sha256,
    sha256(await readFile(join(f.captured, "assets-upload.multipart"))),
  );
  assert.deepEqual(wire.entries, [
    {
      paths: ["/message.txt"],
      hash,
      mime: "text/plain",
      size: Buffer.byteLength(source),
      sha256: sha256(Buffer.from(source)),
    },
  ]);
  assert.equal((await f.session()).status, 400);
  assert.equal((await f.upload(result.jwt)).status, 400);
});

test("asset capture deduplicates identical files with distinct paths", async (t) => {
  const f = await fixture(t);
  await writeFile(join(f.assets, "same.txt"), source);
  const session = await f.session({
    "/message.txt": { hash, size: source.length },
    "/same.txt": { hash, size: source.length },
  });
  const { result } = await session.json();
  assert.deepEqual(result.buckets, [[hash]]);
  assert.equal((await f.upload(result.jwt)).status, 200);
  const wire = JSON.parse(
    await readFile(join(f.captured, "assets-upload.json")),
  );
  assert.deepEqual(wire.entries[0].paths, ["/message.txt", "/same.txt"]);
});

test("asset capture rejects traversal, symlink escape, malformed size and collisions", async (t) => {
  for (const kind of [
    "traversal",
    "symlink",
    "size",
    "collision",
    "hash",
    "empty",
  ]) {
    const f = await fixture(t);
    let manifest = { "/message.txt": { hash, size: source.length } };
    if (kind === "traversal")
      manifest = { "/../outside.txt": { hash, size: source.length } };
    if (kind === "symlink") {
      await writeFile(join(f.root, "outside.txt"), source);
      await symlink("../outside.txt", join(f.assets, "escaped.txt"));
      manifest = { "/escaped.txt": { hash, size: source.length } };
    }
    if (kind === "size") manifest["/message.txt"].size += 1;
    if (kind === "hash") manifest["/message.txt"].hash = "invalid";
    if (kind === "empty") manifest = {};
    if (kind === "collision") {
      await writeFile(join(f.assets, "other.txt"), "x".repeat(source.length));
      manifest["/other.txt"] = { hash, size: source.length };
    }
    assert.equal((await f.session(manifest)).status, 400, kind);
    assert.throws(() =>
      f.capture.assertComplete({ assets: { jwt: completeAssetToken } }),
    );
  }
});

test("asset capture rejects unauthorized, changed, missing and duplicate upload parts", async (t) => {
  for (const kind of ["auth", "changed", "missing", "duplicate"]) {
    const f = await fixture(t);
    assert.equal((await f.session(undefined, "wrong-account")).status, 400);
    const { result } = await (await f.session()).json();
    const token = kind === "auth" ? "wrong-upload-token" : result.jwt;
    let parts = [[hash, source]];
    if (kind === "changed") parts = [[hash, "wrong-file"]];
    if (kind === "missing") parts = [];
    if (kind === "duplicate") parts.push([hash, source]);
    assert.equal((await f.upload(token, parts)).status, 400, kind);
    assert.throws(() =>
      f.capture.assertComplete({ assets: { jwt: completeAssetToken } }),
    );
  }
});

test("asset capture refuses to overwrite retained evidence", async (t) => {
  const f = await fixture(t);
  const existing = join(f.captured, "assets-session.json");
  await writeFile(existing, "retained-evidence");
  assert.equal((await f.session()).status, 400);
  assert.equal(await readFile(existing, "utf8"), "retained-evidence");
});
