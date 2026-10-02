import { delimiter, resolve } from "node:path";

export function processEnv(
  extra: Readonly<Record<string, string>>,
): Record<string, string> {
  const env: Record<string, string> = {
    ...extra,
    CI: "true",
    CF_SEND_TELEMETRY: "false",
    DO_NOT_TRACK: "1",
  };
  for (const name of ["PATH", "HOME", "TMPDIR", "TMP", "TEMP"]) {
    const value = process.env[name];
    if (value !== undefined) env[name] = value;
  }
  env.PATH = `${resolve(import.meta.dirname, "../../../node_modules/.bin")}${delimiter}${env.PATH ?? ""}`;
  return env;
}
