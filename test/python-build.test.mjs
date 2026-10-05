import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import {
  access,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  symlink,
  writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import { findPyWrangler } from "../scripts/build-python.ts";
import { repository } from "../scripts/workerd-archive.ts";

const execute = promisify(execFile);

async function project() {
  const parent = join(repository, ".temp/python-build-tests");
  await mkdir(parent, { recursive: true });
  const dir = await mkdtemp(join(parent, "project-"));
  await writeFile(join(dir, "package.json"), '{"type":"module"}\n');
  await writeFile(
    join(dir, "pyproject.toml"),
    '[project]\nname = "build-test"\nversion = "0.0.0"\n',
  );
  await writeFile(
    join(dir, "cloudflare.config.ts"),
    `export default ({ mode }) => ({ worker: {
    name: mode ?? "python-build-test", entrypoint: "src/main.py",
    compatibilityDate: "2026-09-08", compatibilityFlags: ["python_workers"],
  } });\n`,
  );
  return dir;
}

async function fakeTool(dir, fail = "") {
  const bin = join(dir, ".venv/bin");
  await mkdir(bin, { recursive: true });
  await writeFile(join(bin, "package.json"), '{"type":"commonjs"}\n');
  await writeFile(
    join(bin, "pywrangler"),
    `#!${process.execPath}
const fs = require("node:fs");
const args = process.argv.slice(2);
fs.appendFileSync("calls.jsonl", JSON.stringify(args) + "\\n");
if (args[0] === "sync") {
  fs.copyFileSync("wrangler.jsonc", "sync-input.json");
} else if (args[0] === "build") {
  if (fs.existsSync("wrangler.jsonc")) process.exit(99);
  fs.copyFileSync("wrangler.config.ts", "build-input.ts");
  fs.mkdirSync(".cloudflare/output/v0", { recursive: true });
  // Launcher fixture only: this is never used as SDK/Gate qualification input.
  fs.writeFileSync(".cloudflare/output/v0/config.json", "{}");
} else process.exit(98); // Reject version probes and every deployment command.
if (args[0] === ${JSON.stringify(fail)}) process.exit(23);
`,
    { mode: 0o700 },
  );
  return join(bin, "pywrangler");
}

async function build(dir, ...args) {
  return execute(
    process.execPath,
    [join(repository, "scripts/build-python.ts"), dir, ...args],
    {
      env: { ...process.env, PATH: "" },
    },
  );
}

test("Python build reports missing user PyWrangler without installing anything", async () => {
  const dir = await project();
  await assert.rejects(findPyWrangler(dir, ""), /uv tool install workers-py/);
  await assert.rejects(build(dir), /PyWrangler is not installed/);
  await assert.rejects(access(join(dir, "wrangler.jsonc")));
  await assert.rejects(access(join(dir, "wrangler.config.ts")));
});

test("Python build rejects project-root or external source before dependency preparation", async () => {
  for (const entrypoint of ["main.py", "../outside/main.py"]) {
    const dir = await project();
    await fakeTool(dir);
    await writeFile(
      join(dir, "cloudflare.config.ts"),
      `export default {worker: {name: "python", entrypoint: ${JSON.stringify(entrypoint)}, compatibilityDate: "2026-09-08", compatibilityFlags: ["python_workers"]}};\n`,
    );
    await assert.rejects(build(dir), /separate application directory/);
    await assert.rejects(access(join(dir, "calls.jsonl")));
    await assert.rejects(access(join(dir, "wrangler.jsonc")));
    await assert.rejects(access(join(dir, "wrangler.config.ts")));
  }
});

test("Python build uses installed PyWrangler without probing its version", async () => {
  const dir = await project();
  const tool = await fakeTool(dir);
  assert.equal(await findPyWrangler(dir, "/not/a/tool"), tool);
  assert.equal(
    await findPyWrangler(await project(), join(dir, ".venv/bin")),
    tool,
  );
  await build(dir, "staging");
  assert.deepEqual(
    (await readFile(join(dir, "calls.jsonl"), "utf8"))
      .trim()
      .split("\n")
      .map(JSON.parse),
    [
      ["sync"],
      [
        "build",
        "--experimental-new-config",
        "--experimental-cf-build-output",
        "--env",
        "staging",
      ],
    ],
  );
  assert.deepEqual(
    JSON.parse(await readFile(join(dir, "sync-input.json"), "utf8")),
    {
      compatibility_date: "2026-09-08",
      compatibility_flags: ["python_workers"],
    },
  );
  await assert.rejects(access(join(dir, "wrangler.jsonc")));
  await assert.rejects(access(join(dir, "wrangler.config.ts")));
});

test("Python sync failure removes its disposable config and never starts build", async () => {
  const dir = await project();
  await fakeTool(dir, "sync");
  await assert.rejects(build(dir), /PyWrangler sync failed/);
  assert.equal(await readFile(join(dir, "calls.jsonl"), "utf8"), '["sync"]\n');
  await assert.rejects(access(join(dir, "wrangler.jsonc")));
  await assert.rejects(access(join(dir, "wrangler.config.ts")));
});

test("Python assets build selects files without moving routing out of cf config", async () => {
  const dir = await project();
  await fakeTool(dir);
  await mkdir(join(dir, "public"));
  const config = await readFile(join(dir, "cloudflare.config.ts"), "utf8");
  await build(dir, "staging", "--assets-directory", "public");
  const input = await readFile(join(dir, "build-input.ts"), "utf8");
  const options = JSON.parse(
    input.slice("export default ".length).trim().replace(/;$/, ""),
  );
  assert.equal(options.assetsDirectory, await realpath(join(dir, "public")));
  assert.equal(options.assets, undefined);
  assert.deepEqual(Object.keys(options).sort(), ["assetsDirectory", "rules"]);
  assert.equal(
    await readFile(join(dir, "cloudflare.config.ts"), "utf8"),
    config,
  );
  const calls = (await readFile(join(dir, "calls.jsonl"), "utf8"))
    .trim()
    .split("\n")
    .map(JSON.parse);
  assert.deepEqual(calls, [
    ["sync"],
    [
      "build",
      "--experimental-new-config",
      "--experimental-cf-build-output",
      "--env",
      "staging",
    ],
  ]);
  await assert.rejects(access(join(dir, "wrangler.jsonc")));
  await assert.rejects(access(join(dir, "wrangler.config.ts")));
});

test("Python assets build rejects invalid directory input before dependency preparation", async () => {
  const dir = await project();
  await fakeTool(dir);
  await writeFile(join(dir, "file.txt"), "not a directory");
  await symlink(await project(), join(dir, "external"));
  for (const directory of [".", "file.txt", "external", "missing"]) {
    await assert.rejects(build(dir, "--assets-directory", directory));
    await assert.rejects(access(join(dir, "calls.jsonl")));
    await assert.rejects(access(join(dir, "wrangler.jsonc")));
    await assert.rejects(access(join(dir, "wrangler.config.ts")));
  }
  for (const args of [
    ["--assets-directory"],
    ["--assets-directory", "public", "--assets-directory", "public"],
  ]) {
    await assert.rejects(build(dir, ...args), /Usage:/);
    await assert.rejects(access(join(dir, "calls.jsonl")));
  }
});

test("Python build failure propagates even with existing Build Output", async () => {
  const dir = await project();
  await fakeTool(dir, "build");
  await assert.rejects(build(dir), /PyWrangler build failed/);
  await assert.rejects(access(join(dir, "wrangler.jsonc")));
  await assert.rejects(access(join(dir, "wrangler.config.ts")));
});

test("Python build preserves existing config and rejects non-Python projects", async () => {
  for (const name of [
    "wrangler.jsonc",
    "wrangler.toml",
    "wrangler.json",
    "wrangler.config.ts",
  ]) {
    const dir = await project();
    await fakeTool(dir);
    await writeFile(join(dir, name), "preserve user bytes\n");
    await assert.rejects(build(dir));
    assert.equal(
      await readFile(join(dir, name), "utf8"),
      "preserve user bytes\n",
    );
    await assert.rejects(access(join(dir, "calls.jsonl")));
  }
  const dir = await project();
  await fakeTool(dir);
  await writeFile(
    join(dir, "cloudflare.config.ts"),
    'export default {worker: {name: "js", entrypoint: "main.ts", compatibilityDate: "2026-09-08", compatibilityFlags: []}};\n',
  );
  await assert.rejects(build(dir), /only a Python entrypoint/);
  await assert.rejects(access(join(dir, "calls.jsonl")));
});
