// Original tenant code is initialized by the fixed INTERNAL host policy.
import { moduleValue } from "./module-values.js";
import { workerPolicy } from "./policy.js";
import type { RuntimeSnapshot } from "./protocol.js";
import { bindingError } from "./shared.js";

export function modulesFor(
  snapshot: RuntimeSnapshot,
  validation: boolean,
  entrypointName: string | undefined,
  durableObject = false,
  workflow = false,
) {
  if (
    snapshot.contentKind !== "worker" ||
    typeof snapshot.mainModule !== "string" ||
    snapshot.bindings.some((binding) => binding.capabilityVersion !== 1)
  )
    throw bindingError("VERSION_INVARIANT_VIOLATION");
  const modules: Record<string, WorkerLoaderModule> = {};
  for (const module of snapshot.modules) {
    if (
      module.name.startsWith("cloudflare-internal:") ||
      module.name.startsWith("open-compute:")
    )
      throw bindingError("VERSION_INVARIANT_VIOLATION");
    Object.defineProperty(modules, module.name, {
      value: moduleValue(module),
      enumerable: true,
    });
  }
  return {
    modules,
    mainModule: snapshot.mainModule,
    policy: workerPolicy(
      snapshot,
      validation,
      entrypointName,
      durableObject,
      workflow,
    ),
  };
}
