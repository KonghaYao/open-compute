// Explicit build input acquisition; never shipped or invoked by ocd.
import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { prepareCaddy } from "./caddy-archive.ts";
import { absoluteDestination, sourceArguments } from "./workerd-archive.ts";

const input = sourceArguments(process.argv.slice(2));
const destination = await absoluteDestination(input.destination);
await mkdir(destination, { mode: 0o700 });
let complete = false;
try {
  const result = await prepareCaddy(destination, input.archive, input.download);
  complete = true;
  console.log(`OPEN_COMPUTE_BUILD_CADDY=${join(destination, "caddy")}`);
  console.log(`OPEN_COMPUTE_CADDY_RELEASE=${result.release}`);
} finally {
  if (!complete) await rm(destination, { recursive: true, force: true });
}
