import { AiSearchNamespaceBinding } from "./facade.js";

/** Native construction entrypoint for a namespace-scoped AI Search Fetcher. */
export default function namespaceBinding(env: {
  fetcher: unknown;
}): AiSearchNamespaceBinding {
  return new AiSearchNamespaceBinding(env.fetcher);
}
