// TODO(P21): Remove this Python-only PyWrangler bridge when cf's official
// Python builder can produce and qualify equivalent Build Output.
import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { access, mkdir, open, realpath, stat, unlink } from "node:fs/promises";
import {
  delimiter,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
} from "node:path";
import { fileURLToPath } from "node:url";

// The upstream internal package's declarations include Worker-only globals.
// At this Node tool boundary, validate its result instead of importing those
// globals into every developer script or weakening strict TypeScript checks.
function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("Invalid Cloudflare configuration result.");
  return value as Record<string, unknown>;
}

async function pythonConfig(project: string, mode?: string) {
  const configModule = import.meta.resolve("@cloudflare/config");
  const api = record((await import(configModule)) as unknown);
  if (typeof api.loadAndParseConfig !== "function")
    throw new Error("Cloudflare configuration loader is unavailable.");
  const loaded: unknown = await api.loadAndParseConfig(
    join(project, "cloudflare.config.ts"),
    {
      mode,
      isPreview: process.env.CLOUDFLARE_PREVIEW_BUILD === "true",
    },
  );
  const result = record(record(loaded).result);
  if (result.success !== true)
    throw new Error("Invalid cloudflare.config.ts for Python build.");
  const worker = record(record(result.data).worker);
  if (
    typeof worker.entrypoint !== "string" ||
    !worker.entrypoint.endsWith(".py")
  )
    throw new Error(
      "The PyWrangler build bridge accepts only a Python entrypoint.",
    );
  const moduleDirectory = relative(
    project,
    dirname(resolve(project, worker.entrypoint)),
  );
  if (
    !moduleDirectory ||
    moduleDirectory === ".." ||
    moduleDirectory.startsWith("../") ||
    isAbsolute(moduleDirectory)
  )
    throw new Error(
      "Keep the Python entrypoint and local package/data in a separate application " +
        "directory inside this project, such as src/main.py. The upstream collector " +
        "otherwise includes project configuration and host virtualenv files.",
    );
  if (
    typeof worker.compatibilityDate !== "string" ||
    !Array.isArray(worker.compatibilityFlags) ||
    !worker.compatibilityFlags.every(
      (flag: unknown) => typeof flag === "string",
    )
  )
    throw new Error(
      "Python builds require a compatibility date and flags in cloudflare.config.ts.",
    );
  return {
    compatibility_date: worker.compatibilityDate,
    compatibility_flags: worker.compatibilityFlags,
  };
}

async function executable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK);
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

/** Resolve an existing project virtualenv or PATH installation without installing it. */
export async function findPyWrangler(
  project: string,
  path = process.env.PATH ?? "",
): Promise<string> {
  const candidates = [
    join(project, ".venv/bin/pywrangler"),
    ...path
      .split(delimiter)
      .filter(Boolean)
      .map((dir) => join(dir, "pywrangler")),
  ];
  for (const candidate of candidates) {
    if (await executable(candidate)) return resolve(candidate);
  }
  throw new Error(
    "PyWrangler is not installed. Install it yourself with `uv tool install workers-py` " +
      "and put pywrangler on PATH, or install workers-py in the project's .venv. " +
      "Python builds use your installation without a version check.",
  );
}

async function run(
  command: string,
  args: string[],
  project: string,
): Promise<void> {
  const cache = fileURLToPath(
    new URL("../.temp/python-build/", import.meta.url),
  );
  await mkdir(cache, { recursive: true });
  await new Promise<void>((accept, reject) => {
    const child = spawn(command, args, {
      cwd: project,
      stdio: "inherit",
      env: {
        ...process.env,
        // PyWrangler proxies build to npx Wrangler. Require an already available
        // Wrangler rather than letting npx download a tool during this bridge.
        npm_config_offline: "true",
        WRANGLER_SEND_METRICS: "false",
        WRANGLER_LOG_PATH: join(cache, "wrangler.log"),
        UV_CACHE_DIR: process.env.UV_CACHE_DIR ?? join(cache, "uv-cache"),
        UV_PYTHON_DOWNLOADS: process.env.UV_PYTHON_DOWNLOADS ?? "never",
      },
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) accept();
      else
        reject(
          new Error(
            `PyWrangler ${args[0]} failed (${signal ?? code}). ` +
              (args[0] === "build"
                ? "Install Wrangler in this Python project's development dependencies " +
                  "with `bun add --dev wrangler`; no tool version is checked."
                : "See the Python dependency/interpreter error above; prepare the inputs required by your PyWrangler installation."),
          ),
        );
    });
  });
}

/** Build Python locally; uploads, authentication and resources remain owned by cf. */
export async function buildPython(
  project: string,
  mode?: string,
  assetsDirectory?: string,
): Promise<void> {
  project = resolve(project);
  const pywrangler = await findPyWrangler(project);
  await access(join(project, "pyproject.toml"));
  // Never read or overwrite a second persistent configuration.
  for (const name of ["wrangler.toml", "wrangler.json", "wrangler.config.ts"]) {
    try {
      await access(join(project, name));
    } catch {
      continue;
    }
    throw new Error(
      `Remove ${name}; cloudflare.config.ts is the authoritative configuration.`,
    );
  }
  const config = await pythonConfig(project, mode);
  let assets: string | undefined;
  if (assetsDirectory !== undefined) {
    assets = await realpath(resolve(project, assetsDirectory));
    const scope = relative(await realpath(project), assets);
    if (
      !scope ||
      scope === ".." ||
      scope.startsWith("../") ||
      isAbsolute(scope) ||
      !(await stat(assets)).isDirectory()
    )
      throw new Error(
        "Static assets must use a separate directory inside the Python project.",
      );
  }

  // PyWrangler sync currently reads only the date/flags from a Wrangler file.
  // Create that disposable input exclusively, then remove it BEFORE building
  // from cloudflare.config.ts. Existing files (including concurrent builds) fail closed.
  const input = join(project, "wrangler.jsonc");
  const handle = await open(input, "wx", 0o600);
  try {
    await handle.writeFile(JSON.stringify(config) + "\n");
    await handle.close();
    await run(pywrangler, ["sync"], project);
  } finally {
    await handle.close();
    await unlink(input);
  }
  // This upstream command writes the complete official Build Output, including
  // SDK JS module MIME and package data. No local bundler/manifest translation.
  const options = join(project, "wrangler.config.ts");
  const buildOptions = await open(options, "wx", 0o600);
  try {
    // Common local Python package data; vendored dependency files are handled
    // entirely by the upstream Python module collector, including SDK JS files.
    // This upstream build input selects files only. The binding and all runtime
    // asset routing settings remain owned by cloudflare.config.ts.
    await buildOptions.writeFile(
      "export default " +
        JSON.stringify({
          rules: [
            {
              type: "Data",
              globs: ["**/*.json", "**/*.html"],
              fallthrough: true,
            },
          ],
          ...(assets === undefined ? {} : { assetsDirectory: assets }),
        }) +
        ";\n",
    );
    await buildOptions.close();
    await run(
      pywrangler,
      [
        "build",
        "--experimental-new-config",
        "--experimental-cf-build-output",
        ...(mode === undefined ? [] : ["--env", mode]),
      ],
      project,
    );
  } finally {
    await buildOptions.close();
    await unlink(options);
  }
  await access(join(project, ".cloudflare/output/v0/config.json"));
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const [project = ".", ...args] = process.argv.slice(2);
  try {
    let mode: string | undefined;
    let assetsDirectory: string | undefined;
    for (let index = 0; index < args.length; index++) {
      const value = args[index]!;
      if (
        value === "--assets-directory" &&
        assetsDirectory === undefined &&
        args[index + 1] !== undefined &&
        !args[index + 1]!.startsWith("--")
      ) {
        assetsDirectory = args[++index]!;
      } else if (mode === undefined && !value.startsWith("--")) {
        mode = value;
      } else {
        throw new Error(
          "Usage: node scripts/build-python.ts [project] [mode] [--assets-directory path]",
        );
      }
    }
    await buildPython(project, mode, assetsDirectory);
    console.log(
      "Python Build Output is ready. Upload/deploy with cf --prebuilt.",
    );
  } catch (error) {
    console.error(
      error instanceof Error ? error.message : "Python build failed.",
    );
    process.exitCode = 1;
  }
}
