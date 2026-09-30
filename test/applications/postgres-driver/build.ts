import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compileWorker } from "../../../packages/toolchain/src/build-worker.ts";

const root = dirname(fileURLToPath(import.meta.url));
const output = resolve(root, "dist");
const worker = await compileWorker({
  project: root,
  entry: "src/index.ts",
  tsconfig: "tsconfig.worker.json",
});
if (
  worker.modules.length !== 1 ||
  worker.modules[0]?.name !== worker.mainModule
)
  throw new Error("PostgreSQL fixture must compile to one Worker module");
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await writeFile(resolve(output, "worker.js"), worker.modules[0].bytes);
