import { readFileSync } from "node:fs";
import { record, string } from "../checks/context.ts";

const lock = JSON.parse(
  readFileSync(
    new URL("../../../packages/runtime/workerd.lock.json", import.meta.url),
    "utf8",
  ),
) as {
  binaryMaximumCompatibilityDate: string;
};

const today = new Date().toISOString().slice(0, 10);
export const COMPATIBILITY_DATE =
  lock.binaryMaximumCompatibilityDate < today
    ? lock.binaryMaximumCompatibilityDate
    : today;
export const COMPATIBILITY_FLAGS: string[] = [];
const packageManifest = record(
  JSON.parse(
    readFileSync(new URL("../../../package.json", import.meta.url), "utf8"),
  ),
  "root package",
);
const catalog = record(packageManifest.catalog, "dependency catalog");
/** Exact dependencies identifying the official cf/Vite fixture builder. */
export const CF_PROJECT_MANIFEST = {
  private: true,
  type: "module",
  devDependencies: Object.fromEntries(
    ["cf", "vite", "@cloudflare/vite-plugin"].map((name) => [
      name,
      string(catalog[name], name),
    ]),
  ),
};
/** Cf version coordinated with the formal workerd/workers-types baseline. */
export const CF_VERSION = string(catalog.cf, "cf");
export const MAX_OUTPUT = 1024 * 1024;
