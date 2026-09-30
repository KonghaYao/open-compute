import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, readFile } from "node:fs/promises";
import type { loadPin } from "./workerd-archive.ts";

type Pin = Awaited<ReturnType<typeof loadPin>>;
const maxBinary = 256 * 1024 * 1024;
const maxCatalog = 1024 * 1024;
const digest = (bytes: Uint8Array) =>
  createHash("sha256").update(bytes).digest("hex");

/** Run compatibility discovery on an exact, formally pinned workerd binary. */
export async function compatibilityCatalog(
  binaryPath: string,
  pin: Pin,
): Promise<Buffer> {
  const metadata = await lstat(binaryPath);
  if (
    !metadata.isFile() ||
    metadata.size > maxBinary ||
    (metadata.mode & 0o111) === 0 ||
    digest(await readFile(binaryPath)) !== pin.binarySha256
  ) {
    throw new Error("prepared workerd does not match the formal pin");
  }
  const result = spawnSync(binaryPath, ["compatibility-catalog"], {
    encoding: "buffer",
    maxBuffer: maxCatalog,
    timeout: 30_000,
  });
  if (
    result.error ||
    result.status !== 0 ||
    result.stderr.length !== 0 ||
    result.stdout.length === 0 ||
    result.stdout.length > maxCatalog
  ) {
    throw new Error("workerd compatibility introspection failed");
  }
  const bytes = result.stdout;
  if (digest(bytes) !== pin.catalogSha256)
    throw new Error("workerd compatibility catalog SHA-256 mismatch");
  const parsed = JSON.parse(
    new TextDecoder("utf-8", { fatal: true }).decode(bytes),
  ) as Record<string, unknown>;
  if (
    parsed.schemaVersion !== pin.catalogSchemaVersion ||
    parsed.validation !== "code_version" ||
    parsed.binaryMaximumDate !== pin.binaryMaximumCompatibilityDate ||
    parsed.futureDatesAllowed !== false ||
    !Array.isArray(parsed.features)
  ) {
    throw new Error("workerd compatibility catalog does not match the lock");
  }
  return bytes;
}
