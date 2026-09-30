import { mkdir, open, readFile } from "node:fs/promises";
import { join } from "node:path";
import { verifyBundledCaddy } from "./bundled-caddy.ts";
import { verifyBundledPyodide } from "./bundled-pyodide.ts";
import { verifyBundledTesseractSources } from "./bundled-tesseract.ts";
import {
  bundledCompatibilityCatalog,
  bundledWorkerdArchive,
} from "./bundled-workerd.ts";
import {
  hostTarget,
  loadPin,
  loadPyodidePin,
  repository,
} from "./workerd-archive.ts";

// Prepare the official release targets from their checked-in binaries; never download.
// The darwin-x64 pin remains available for explicit manual Intel builds.
for (const target of [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
]) {
  const pin = await loadPin(target);
  await bundledWorkerdArchive(repository, pin);
  console.log(`Verified bundled workerd: ${target}`);
}

const hostPin = await loadPin(hostTarget());
const catalog = await bundledCompatibilityCatalog(repository, hostPin);
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

console.log(`Verified bundled Caddy: ${await verifyBundledCaddy()}`);

const pyodide = await loadPyodidePin();
await verifyBundledPyodide(repository, pyodide);
console.log(`Verified bundled Pyodide: ${pyodide.version}`);

await verifyBundledTesseractSources(repository);
console.log("Verified bundled xberg-tesseract source inputs: 1.1.5");
