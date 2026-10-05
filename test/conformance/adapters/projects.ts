import { relative } from "node:path";
import { COMPATIBILITY_DATE, COMPATIBILITY_FLAGS } from "./runtime-contract.ts";
import type { JsonRecord, PortableFixture } from "./types.ts";

function exactBindingIds(
  fixture: PortableFixture,
  ids: Readonly<Record<string, string>>,
): Readonly<Record<string, string>> {
  const expected = Object.keys(fixture.bindings)
    .filter(
      (binding) =>
        fixture.bindings[binding]?.type === "kv_namespace" ||
        fixture.bindings[binding]?.type === "d1_database",
    )
    .sort();
  const actual = Object.keys(ids).sort();
  if (
    JSON.stringify(actual) !== JSON.stringify(expected) ||
    Object.values(ids).some((id) => id.length === 0)
  )
    throw new Error("portable fixture binding identities are incomplete");
  return ids;
}

export function openComputeProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
  bindingIds: Readonly<Record<string, string>> = {},
  bindingNames: Readonly<Record<string, string>> = {},
): JsonRecord {
  return workerProject(
    fixture,
    name,
    accountId,
    bindingIds,
    bindingNames,
    false,
  );
}

/** Minimal open-compute Cf project used before owned bindings are provisioned. */
export function openComputeBaseProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
): JsonRecord {
  return baseProject(fixture, name, accountId, false);
}

/** Minimal Worker project used before any owned binding has been provisioned. */
export function cloudflareBaseProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
): JsonRecord {
  return baseProject(fixture, name, accountId, true);
}

function baseProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
  workersDev: boolean,
): JsonRecord {
  return {
    accountId,
    worker: {
      name,
      entrypoint: relative(fixture.root, fixture.source),
      compatibilityDate: COMPATIBILITY_DATE,
      compatibilityFlags: [...COMPATIBILITY_FLAGS],
      workersDev,
    },
  };
}

export function cloudflareProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
  bindingIds: Readonly<Record<string, string>> = {},
  bindingNames: Readonly<Record<string, string>> = {},
): JsonRecord {
  return workerProject(
    fixture,
    name,
    accountId,
    bindingIds,
    bindingNames,
    true,
  );
}

function workerProject(
  fixture: PortableFixture,
  name: string,
  accountId: string,
  bindingIds: Readonly<Record<string, string>>,
  bindingNames: Readonly<Record<string, string>>,
  workersDev: boolean,
): JsonRecord {
  const ids = exactBindingIds(fixture, bindingIds);
  const expectedNames = Object.keys(fixture.bindings)
    .filter(
      (binding) =>
        fixture.bindings[binding]?.type === "d1_database" ||
        fixture.bindings[binding]?.type === "r2_bucket" ||
        fixture.bindings[binding]?.type === "queue_producer" ||
        fixture.bindings[binding]?.type === "workflow",
    )
    .sort();
  const actualNames = Object.keys(bindingNames).sort();
  if (
    JSON.stringify(expectedNames) !== JSON.stringify(actualNames) ||
    Object.values(bindingNames).some((bindingName) => bindingName.length === 0)
  ) {
    throw new Error(
      "portable fixture named Cloudflare bindings are incomplete",
    );
  }
  const env: JsonRecord = {};
  const exports: JsonRecord = {};
  for (const [binding, value] of Object.entries(fixture.bindings)) {
    switch (value.type) {
      case "kv_namespace":
        env[binding] = { type: "kv", id: ids[binding] };
        break;
      case "d1_database":
        env[binding] = {
          type: "d1",
          id: ids[binding],
          name: bindingNames[binding],
        };
        break;
      case "r2_bucket":
        env[binding] = { type: "r2", name: bindingNames[binding] };
        break;
      case "queue_producer":
        env[binding] = { type: "queue", name: bindingNames[binding] };
        break;
      case "worker_loader":
        env[binding] = { type: "worker-loader" };
        break;
      case "do_namespace":
        env[binding] = {
          type: "durable-object",
          worker: name,
          exportName: value.className,
        };
        exports[value.className] = {
          type: "durable-object",
          storage: "sqlite",
        };
        break;
      case "workflow":
        env[binding] = {
          type: "workflow",
          worker: name,
          exportName: value.className,
          name: bindingNames[binding],
        };
        exports[value.className] = {
          type: "workflow",
          name: bindingNames[binding],
          ...(value.schedules === undefined
            ? {}
            : { schedules: value.schedules }),
        };
        break;
    }
  }
  const config = baseProject(fixture, name, accountId, workersDev);
  const worker = config.worker as JsonRecord;
  return { ...config, worker: { ...worker, env, exports } };
}
