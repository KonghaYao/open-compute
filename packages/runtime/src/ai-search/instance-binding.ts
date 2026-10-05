import { AiSearchInstanceBinding } from "./facade.js";

/** Native construction entrypoint for an instance-scoped AI Search Fetcher. */
export default function instanceBinding(env: {
  fetcher: unknown;
}): AiSearchInstanceBinding {
  return new AiSearchInstanceBinding(env.fetcher);
}
