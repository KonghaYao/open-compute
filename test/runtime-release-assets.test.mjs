import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { loadCaddyPin, verifyCaddyBinary } from "../scripts/caddy-archive.ts";
import { hostTarget, loadPin, sha256 } from "../scripts/workerd-archive.ts";

const root = fileURLToPath(new URL("../", import.meta.url));
const targets = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"];

test("formal runtime locks point to complete immutable GitHub releases", async () => {
  for (const target of targets) {
    const workerd = await loadPin(target);
    assert.equal(
      workerd.archiveUrl,
      `https://github.com/elliothux/workerd/releases/download/${workerd.release}/${workerd.archiveName}`,
    );
    const caddy = await loadCaddyPin(target);
    assert.equal(
      caddy.archiveUrl,
      `${caddy.sourceRepository}/releases/download/${caddy.release}/${caddy.archiveName}`,
    );
  }
  const caddy = await loadCaddyPin();
  const gitlink = execFileSync(
    "git",
    ["ls-files", "--stage", "third_party/caddy"],
    { cwd: root, encoding: "utf8" },
  ).trim();
  assert.equal(gitlink.split(/\s+/)[1], caddy.revision);
});

test("prepared host binaries match both formal locks", async () => {
  const workerdPath = process.env.OPEN_COMPUTE_TEST_WORKERD;
  const caddyPath = process.env.OPEN_COMPUTE_BUILD_CADDY;
  assert.ok(workerdPath, "OPEN_COMPUTE_TEST_WORKERD is required");
  assert.ok(caddyPath, "OPEN_COMPUTE_BUILD_CADDY is required");
  assert.ok((await stat(workerdPath)).isFile());
  assert.equal(
    sha256(await readFile(workerdPath)),
    (await loadPin(hostTarget())).binarySha256,
  );
  await verifyCaddyBinary(caddyPath, await loadCaddyPin(hostTarget()));
});
