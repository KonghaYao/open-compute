import { execFileSync } from "node:child_process";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { loadPortableFixtures } from "../adapters/fixtures.ts";
import { cloudflareProject, openComputeProject } from "../adapters/projects.ts";
import {
  cloudflareDeploymentUrl,
  cloudflareTransientFailure,
  cloudflareWorkerMissing,
  observationUrl,
} from "../adapters/transport.ts";
import { array, contracts, record, ROOT, string, strings } from "./context.ts";

export function portableFixtureInventory(): void {
  const root = join(ROOT, "test/conformance/fixtures");
  const paths = execFileSync(
    "find",
    [root, "-name", "contract.json", "-type", "f"],
    { encoding: "utf8" },
  )
    .trim()
    .split("\n")
    .filter(Boolean)
    .sort();
  if (!paths.length) throw new Error("portable fixture inventory is empty");
  const ids = new Set<string>();
  const catalogIds = new Set(
    contracts().map((contract) => string(contract.id, "contract.id")),
  );
  for (const path of paths) {
    const contract = record(
      JSON.parse(readFileSync(path, "utf8")),
      `fixture ${relative(ROOT, path)}`,
    );
    const id = string(contract.id, "fixture.id");
    if (ids.has(id)) throw new Error(`duplicate fixture id: ${id}`);
    ids.add(id);
    const source = resolve(
      dirname(path),
      string(contract.source, `${id}.source`),
    );
    if (!source.startsWith(`${dirname(path)}/`) || !lstatSync(source).isFile())
      throw new Error(`${id}: invalid source`);
    if (readFileSync(source, "utf8").includes("OPEN_COMPUTE"))
      throw new Error(`${id}: target-specific Worker branch`);
    for (const mapped of strings(contract.contracts, `${id}.contracts`)) {
      if (!catalogIds.has(mapped))
        throw new Error(`${id}: unknown contract mapping`);
    }
    if (!array(contract.observations, `${id}.observations`).length)
      throw new Error(`${id}: no observations`);
    const cleanup = record(contract.cleanup, `${id}.cleanup`);
    if (
      !array(cleanup.cloudflare, `${id}.cleanup.cloudflare`).length ||
      !array(cleanup.openCompute, `${id}.cleanup.openCompute`).length
    )
      throw new Error(`${id}: incomplete cleanup ownership`);
  }
}

export async function cloudflareRunnerSafety(): Promise<void> {
  const name = "oc-p34-test-1234";
  const url = cloudflareDeploymentUrl(
    `Uploaded fixture\nhttps://${name}.account.workers.dev`,
    name,
  );
  if (url !== `https://${name}.account.workers.dev/`)
    throw new Error("cf deployment URL parsing differs");
  if (
    !cloudflareWorkerMissing("Worker not found [code: 10007]") ||
    !cloudflareWorkerMissing("environment missing [code: 10090]") ||
    cloudflareWorkerMissing("authentication failed [code: 10000]")
  ) {
    throw new Error("cf missing-Worker classification differs");
  }
  if (
    !cloudflareTransientFailure(
      "A fetch request failed, likely due to a connectivity issue",
    ) ||
    cloudflareTransientFailure("authentication failed [code: 10000]")
  ) {
    throw new Error("cf transient failure classification differs");
  }
  if (
    observationUrl("http://worker.account.localhost:8787/", "/reset") !==
    "http://worker.account.localhost:8787/reset"
  ) {
    throw new Error("open-compute differential URL escaped its Worker origin");
  }
  const differentialDirectory = join(ROOT, "test/conformance/differential");
  const source = [
    "test/conformance/differential.ts",
    ...readdirSync(differentialDirectory, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith(".ts"))
      .map((entry) => `test/conformance/differential/${entry.name}`)
      .sort(),
  ]
    .map((path) => readFileSync(join(ROOT, path), "utf8"))
    .join("\n");
  if (source.includes("--delete-with-references")) {
    throw new Error(
      "differential cleanup may delete foreign Worker references",
    );
  }
  for (const obsoleteTransport of [
    "/operator/api",
    '"open-compute.json"',
    '"--ocd"',
  ]) {
    if (source.includes(obsoleteTransport)) {
      throw new Error(
        `differential runner retains obsolete transport: ${obsoleteTransport}`,
      );
    }
  }
  if (
    !source.includes("CLOUDFLARE_API_BASE_URL") ||
    !source.includes('new URL("/client/v4", endpoint)') ||
    !source.includes("cloudflare.config.ts")
  ) {
    throw new Error(
      "open-compute differential runner does not use the official cf v4 boundary",
    );
  }
  if (!source.includes('CF_SEND_TELEMETRY: "false"')) {
    throw new Error(
      "cf telemetry was not disabled for the differential runner",
    );
  }
  for (const requiredOperation of [
    "ensureCloudflareAbsent",
    "deployments",
    "delete",
    "verifyCfAccount",
    "verifyOpenComputeAccount",
    "recordOwnership",
    "readOnlyCf",
    "ensureCloudflareKvAbsent",
    "createCloudflareKv",
    "cleanupCloudflareKv",
    "ensureCloudflareD1Absent",
    "createCloudflareD1",
    "cleanupCloudflareD1",
    "ensureCloudflareR2Absent",
    "createCloudflareR2",
    "cleanupCloudflareR2",
    "ensureQueueAbsent",
    "createQueue",
    "cleanupQueue",
    "ensureWorkflowAbsent",
    "verifyWorkflowCreated",
    "cleanupWorkflow",
    "--force",
  ]) {
    if (!source.includes(requiredOperation))
      throw new Error(
        `Cloudflare runner safety operation is missing: ${requiredOperation}`,
      );
  }
  if (!source.includes('["workers", "delete", name, "--force"]')) {
    throw new Error(
      "differential Worker cleanup is not scoped to the exact run-owned name",
    );
  }
  if (
    !readFileSync(
      join(ROOT, "test/conformance/adapters/observations.ts"),
      "utf8",
    ).includes("activationDeadline")
  ) {
    throw new Error("Cloudflare activation wait is missing");
  }
  const fixture = (
    await loadPortableFixtures(join(ROOT, "test/conformance/fixtures"))
  )[0];
  if (fixture === undefined)
    throw new Error("portable differential fixture is missing");
  const project = openComputeProject(
    fixture,
    name,
    "0123456789abcdef0123456789abcdef",
  );
  if (
    record(project.worker, "worker").entrypoint !== "src/index.ts" ||
    project.accountId !== "0123456789abcdef0123456789abcdef" ||
    record(project.worker, "worker").workersDev !== false
  ) {
    throw new Error(
      "open-compute differential project is not a standard local cf config",
    );
  }
  const kvFixture = (
    await loadPortableFixtures(join(ROOT, "test/conformance/fixtures"))
  ).find((item) => item.id === "kv/portable/namespace");
  if (kvFixture === undefined)
    throw new Error("portable KV differential fixture is missing");
  const kvProject = openComputeProject(
    kvFixture,
    name,
    "0123456789abcdef0123456789abcdef",
    { KV: "019c0000-0000-7000-8000-000000000002" },
  );
  if (
    JSON.stringify(record(record(kvProject.worker, "worker").env, "env").KV) !==
    JSON.stringify({ type: "kv", id: "019c0000-0000-7000-8000-000000000002" })
  )
    throw new Error("portable KV binding identity drift");
  const d1Fixture = (
    await loadPortableFixtures(join(ROOT, "test/conformance/fixtures"))
  ).find((item) => item.id === "d1/portable/database");
  if (d1Fixture === undefined)
    throw new Error("portable D1 differential fixture is missing");
  const d1Ids = {
    DB: "019c0000-0000-7000-8000-000000000003",
    OTHER: "019c0000-0000-7000-8000-000000000004",
  };
  const d1Project = openComputeProject(
    d1Fixture,
    name,
    "0123456789abcdef0123456789abcdef",
    d1Ids,
    { DB: `${name}-d1-0`, OTHER: `${name}-d1-1` },
  );
  if (
    Object.keys(record(record(d1Project.worker, "worker").env, "env"))
      .length !== 2
  )
    throw new Error("portable D1 binding identity drift");
  const cfD1 = cloudflareProject(
    d1Fixture,
    name,
    "0123456789abcdef0123456789abcdef",
    {
      DB: "11111111-1111-4111-8111-111111111111",
      OTHER: "22222222-2222-4222-8222-222222222222",
    },
    { DB: `${name}-d1-0`, OTHER: `${name}-d1-1` },
  );
  const cfEnv = record(record(cfD1.worker, "worker").env, "env");
  if (
    Object.keys(cfEnv).length !== 2 ||
    record(cfEnv.DB, "DB").type !== "d1" ||
    record(cfEnv.OTHER, "OTHER").type !== "d1"
  )
    throw new Error(
      "portable Cloudflare D1 project does not bind only exact owned databases",
    );
}
