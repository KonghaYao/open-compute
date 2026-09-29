import { readFile } from "node:fs/promises";
import { execFile, spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

interface QualificationCase {
  readonly id: string;
  readonly providers: readonly string[];
  readonly protocols: readonly string[];
  readonly models: readonly string[];
  readonly endpointSuffixes: readonly string[];
  readonly responseSchemaSha256: string;
}

interface Manifest {
  readonly schemaVersion: number;
  readonly retrievedAt: string;
  readonly cases: readonly QualificationCase[];
}

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const execFileAsync = promisify(execFile);
const manifest = JSON.parse(
  await readFile(
    resolve(ROOT, "test/ai-provider-qualification.manifest.json"),
    "utf8",
  ),
) as Manifest;

if (
  manifest.schemaVersion !== 1 ||
  !/^\d{4}-\d{2}-\d{2}$/.test(manifest.retrievedAt) ||
  manifest.cases.length !== 3 ||
  new Set(manifest.cases.map(({ id }) => id)).size !== manifest.cases.length ||
  manifest.cases.some(
    (item) =>
      !item.id ||
      item.providers.length === 0 ||
      item.protocols.length === 0 ||
      item.models.length !== item.protocols.length ||
      item.endpointSuffixes.length !== item.protocols.length ||
      !/^[a-f0-9]{64}$/.test(item.responseSchemaSha256),
  )
) {
  throw new Error("invalid AI provider qualification manifest");
}

const args = process.argv.slice(2);
if (args.length === 1 && args[0] === "--list") {
  process.stdout.write(
    `${JSON.stringify({ schemaVersion: 1, cases: manifest.cases.map(({ id }) => id) })}\n`,
  );
} else {
  const selected: string[] = [];
  for (let index = 0; index < args.length; index += 2) {
    if (args[index] !== "--case" || args[index + 1] === undefined)
      throw new Error("use --case <id>");
    selected.push(args[index + 1]!);
  }
  const requested = selected.length
    ? selected
    : manifest.cases.map(({ id }) => id);
  if (
    new Set(requested).size !== requested.length ||
    requested.some((id) => !manifest.cases.some((item) => item.id === id))
  ) {
    throw new Error("unknown or duplicate AI provider qualification case");
  }

  const providerVariables = [
    "BAILIAN_API_HOST",
    "BAILIAN_API_KEY",
    "DEEPSEEK_API_KEY",
    "COHERE_API_KEY",
  ] as const;
  if (providerVariables.some((name) => !process.env[name]))
    throw new Error("AI provider qualification environment is incomplete");

  const buildEnvironment = Object.fromEntries(
    [
      "PATH",
      "HOME",
      "TMPDIR",
      "TMP",
      "TEMP",
      "OPEN_COMPUTE_BUILD_WORKERD_ARCHIVE",
    ].flatMap((name) =>
      process.env[name] === undefined ? [] : [[name, process.env[name]!]],
    ),
  );
  let buildOutput: string;
  try {
    ({ stdout: buildOutput } = await execFileAsync(
      "cargo",
      [
      "test",
      "--locked",
      "--offline",
      "--all-features",
      "-p",
      "open-compute-service",
      "--test",
      "p5_search_gate",
      "--no-run",
      "--message-format=json",
      ],
      {
        cwd: ROOT,
        env: buildEnvironment,
        maxBuffer: 64 * 1024 * 1024,
        encoding: "utf8",
      },
    ));
  } catch {
    throw new Error("AI provider qualification build failed");
  }
  const executable = buildOutput
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line) as Record<string, unknown>)
    .find(
      (item) =>
        item.reason === "compiler-artifact" &&
        (item.target as { name?: string } | undefined)?.name ===
          "p5_search_gate" &&
        typeof item.executable === "string",
    )?.executable;
  if (typeof executable !== "string")
    throw new Error("AI provider qualification executable was not produced");

  const results = [];
  for (const id of requested) {
    const definition = manifest.cases.find((item) => item.id === id)!;
    const started = performance.now();
    const environment = Object.fromEntries(
      [
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "TEMP",
        "OPEN_COMPUTE_TEST_WORKERD",
        ...providerVariables,
      ].map((name) => [name, process.env[name]!] as const),
    );
    environment.OPEN_COMPUTE_AI_QUALIFICATION_CASE = id;
    const exitCode = await new Promise<number>((accept) => {
      const child = spawn(
        executable,
        [
          "--exact",
          "p5_real_vectorize_ai_search_and_markdown_matrix",
          "--test-threads=1",
        ],
        { cwd: ROOT, env: environment, stdio: "ignore" },
      );
      child.once("error", () => accept(-1));
      child.once("exit", (code) => accept(code ?? -1));
    });
    const status = exitCode === 0 ? "passed" : "failed";
    results.push({
      id,
      status,
      providers: definition.providers,
      protocols: definition.protocols,
      models: definition.models,
      providerRevision: manifest.retrievedAt,
      httpStatusClass: status === "passed" ? "2xx" : "non-2xx-or-contract-failure",
      durationMs: Math.round(performance.now() - started),
      responseSchemaSha256: definition.responseSchemaSha256,
    });
    if (status === "failed") break;
  }
  const status =
    results.length === requested.length &&
    results.every((item) => item.status === "passed")
      ? "passed"
      : "failed";
  process.stdout.write(
    `${JSON.stringify({ schemaVersion: 1, status, cases: results })}\n`,
  );
  if (status === "failed") process.exitCode = 1;
}
