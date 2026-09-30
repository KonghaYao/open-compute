import { mkdir, open, readFile } from "node:fs/promises";
import { join } from "node:path";
import { verifyBundledPyodide } from "./bundled-pyodide.ts";
import { verifyBundledTesseractSources } from "./bundled-tesseract.ts";
import { loadCaddyPin, verifyCaddyBinary } from "./caddy-archive.ts";
import {
  hostTarget,
  loadPin,
  loadPyodidePin,
  repository,
} from "./workerd-archive.ts";
import { compatibilityCatalog } from "./workerd-catalog.ts";

// Validate the complete release manifests without downloading other targets.
for (const target of [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
]) {
  await loadPin(target);
  await loadCaddyPin(target);
}

const hostPin = await loadPin(hostTarget());
const workerd = process.env.OPEN_COMPUTE_TEST_WORKERD;
if (!workerd)
  throw new Error(
    "OPEN_COMPUTE_TEST_WORKERD is required; explicitly prepare the pinned release asset",
  );
const catalog = await compatibilityCatalog(workerd, hostPin);
const catalogDirectory = join(
  repository,
  ".temp",
  "workerd-build",
  "compatibility-catalog",
  hostPin.catalogSha256,
);
await mkdir(catalogDirectory, { recursive: true, mode: 0o700 });
const catalogPath = join(catalogDirectory, "compatibility-catalog.json");
try {
  const existing = await readFile(catalogPath);
  if (!existing.equals(catalog))
    throw new Error("cached workerd compatibility catalog is corrupt");
} catch (error) {
  if (!(error instanceof Error && "code" in error && error.code === "ENOENT"))
    throw error;
  const file = await open(catalogPath, "wx", 0o444);
  try {
    await file.writeFile(catalog);
    await file.sync();
  } finally {
    await file.close();
  }
}
console.log(
  `Verified bundled workerd compatibility catalog: ${hostPin.catalogSha256}`,
);

const caddy = process.env.OPEN_COMPUTE_BUILD_CADDY;
if (!caddy)
  throw new Error(
    "OPEN_COMPUTE_BUILD_CADDY is required; explicitly prepare the pinned release asset",
  );
const caddyPin = await loadCaddyPin();
await verifyCaddyBinary(caddy, caddyPin);
console.log(`Verified Caddy release: ${caddyPin.release}`);

const pyodide = await loadPyodidePin();
await verifyBundledPyodide(repository, pyodide);
console.log(`Verified bundled Pyodide: ${pyodide.version}`);

await verifyBundledTesseractSources(repository);
console.log("Verified bundled xberg-tesseract source inputs: 1.1.5");
