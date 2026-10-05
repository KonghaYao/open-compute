/** Durable-value codec for persisted Workflow payloads and results. */
export type {
  DurableTypedArray,
  DurableValue,
  DurableValueLimits,
} from "./protocol.js";
export {
  DURABLE_VALUE_LIMITS,
  DURABLE_VALUE_MAGIC,
  DURABLE_VALUE_KIND,
  DURABLE_VALUE_SCHEMA,
  durableValueErrorCode,
} from "./format.js";
export { encodeDurableValue } from "./encode.js";
export { decodeDurableValue } from "./decode.js";
