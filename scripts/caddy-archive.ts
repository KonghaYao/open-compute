import { spawnSync } from "node:child_process";
import { lstat, open, readFile } from "node:fs/promises";
import { isAbsolute, join } from "node:path";
import { gunzipSync } from "node:zlib";
import {
  downloadBounded,
  hostTarget,
  repository,
  sha256,
} from "./workerd-archive.ts";

const maxArchive = 32 * 1024 * 1024;
const maxBinary = 128 * 1024 * 1024;
const targets = [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
] as const;

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("invalid Caddy pin");
  return value as Record<string, unknown>;
}

function string(value: unknown): string {
  if (typeof value !== "string" || !value)
    throw new Error("invalid Caddy pin field");
  return value;
}

/** Load one target from the sole formal Caddy release pin. */
export async function loadCaddyPin(target = hostTarget()) {
  if (!targets.includes(target as (typeof targets)[number]))
    throw new Error("unsupported Caddy target");
  const lock = record(
    JSON.parse(
      await readFile(
        join(repository, "packages/runtime/caddy.lock.json"),
        "utf8",
      ),
    ) as unknown,
  );
  const release = string(lock.release);
  const expectedVersion = string(lock.expectedVersionOutput);
  const entry = record(record(lock.targets)[target]);
  const source = record(lock.source);
  const sourceRepository = string(source.repository);
  const revision = string(source.revision);
  const archiveName = string(entry.archiveName);
  const archiveUrl = string(entry.archiveUrl);
  const archiveSha256 = string(entry.archiveSha256);
  const binarySha256 = string(entry.binarySha256);
  for (const field of [
    "caddyVersion",
    "goVersion",
    "builderImage",
    "build",
    "workflowRun",
  ])
    string(source[field]);
  if (
    lock.schemaVersion !== 2 ||
    sourceRepository !== "https://github.com/elliothux/open-compute-caddy" ||
    !/^[a-f0-9]{40}$/.test(revision) ||
    !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(release) ||
    archiveName !== `caddy-${target}.gz` ||
    archiveUrl !==
      `${sourceRepository}/releases/download/${release}/${archiveName}` ||
    !/^[a-f0-9]{64}$/.test(archiveSha256) ||
    !/^[a-f0-9]{64}$/.test(binarySha256)
  ) {
    throw new Error("formal Caddy pin does not match the release contract");
  }
  return {
    target,
    release,
    archiveName,
    archiveUrl,
    archiveSha256,
    binarySha256,
    expectedVersion,
    sourceRepository,
    revision,
  };
}

/** Verify one prepared Caddy binary, including its custom provider module. */
export async function verifyCaddyBinary(
  path: string,
  pin: Awaited<ReturnType<typeof loadCaddyPin>>,
): Promise<void> {
  const metadata = await lstat(path);
  if (
    !metadata.isFile() ||
    metadata.size > maxBinary ||
    (metadata.mode & 0o111) === 0 ||
    sha256(await readFile(path)) !== pin.binarySha256
  ) {
    throw new Error("prepared Caddy does not match the formal pin");
  }
  const version = spawnSync(path, ["version"], {
    encoding: "utf8",
    timeout: 20_000,
    killSignal: "SIGKILL",
    maxBuffer: 4096,
  });
  if (
    version.error ||
    version.status !== 0 ||
    version.stdout.trim() !== pin.expectedVersion
  ) {
    throw new Error("verified Caddy failed the host version probe");
  }
  const modules = spawnSync(path, ["list-modules"], {
    encoding: "utf8",
    timeout: 20_000,
    killSignal: "SIGKILL",
    maxBuffer: 4 * 1024 * 1024,
  });
  if (
    modules.error ||
    modules.status !== 0 ||
    !modules.stdout.split("\n").includes("dns.providers.opencompute")
  ) {
    throw new Error("verified Caddy is missing the open-compute DNS module");
  }
}

/** Materialize one exact Caddy release asset for an explicit build step. */
export async function prepareCaddy(
  directory: string,
  archivePath: string | undefined,
  download: boolean,
) {
  if (download && archivePath !== undefined)
    throw new Error("choose at most one of --archive ABS or --download");
  const pin = await loadCaddyPin();
  let archive: Buffer;
  if (archivePath !== undefined) {
    if (!isAbsolute(archivePath) || !(await lstat(archivePath)).isFile())
      throw new Error("archive must be an absolute regular file");
    if ((await lstat(archivePath)).size > maxArchive)
      throw new Error("archive exceeds the size bound");
    archive = await readFile(archivePath);
  } else if (download) {
    archive = await downloadBounded(
      pin.archiveUrl,
      maxArchive,
      "Caddy archive",
    );
  } else
    throw new Error(
      "provide a verified --archive or explicitly use --download",
    );
  if (archive.length > maxArchive || sha256(archive) !== pin.archiveSha256)
    throw new Error("Caddy archive SHA-256 does not match the formal pin");
  const binary = gunzipSync(archive, { maxOutputLength: maxBinary });
  if (sha256(binary) !== pin.binarySha256)
    throw new Error("Caddy binary SHA-256 does not match the formal pin");
  const output = join(directory, "caddy");
  const file = await open(output, "wx", 0o600);
  try {
    await file.writeFile(binary);
    await file.chmod(0o500);
    await file.sync();
  } finally {
    await file.close();
  }
  await verifyCaddyBinary(output, pin);
  return { ...pin, binary: output };
}
