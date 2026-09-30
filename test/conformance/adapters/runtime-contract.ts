import { readFileSync } from "node:fs";

const lock = JSON.parse(
  readFileSync(
    new URL("../../../packages/runtime/workerd.lock.json", import.meta.url),
    "utf8",
  ),
) as {
  binaryMaximumCompatibilityDate: string;
  workersSdk: { wranglerVersion: string };
};

const today = new Date().toISOString().slice(0, 10);
export const COMPATIBILITY_DATE =
  lock.binaryMaximumCompatibilityDate < today
    ? lock.binaryMaximumCompatibilityDate
    : today;
export const COMPATIBILITY_FLAGS: string[] = [];
/** Wrangler version coordinated with the formal workerd/workers-types baseline. */
export const WRANGLER_VERSION = lock.workersSdk.wranglerVersion;
export const MAX_OUTPUT = 1024 * 1024;
