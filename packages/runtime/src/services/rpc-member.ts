const nativeGet = Reflect.get;
const nativeOwn = Object.hasOwn;
const objectPrototype = Object.prototype;
const NativeTypeError = TypeError;

function unavailable(): never {
  const error = Object.assign(
    new NativeTypeError("SERVICE_ENTRYPOINT_NOT_FOUND"),
    {
      stableCode: "SERVICE_ENTRYPOINT_NOT_FOUND",
    },
  );
  error.stack = "TypeError: SERVICE_ENTRYPOINT_NOT_FOUND";
  throw error;
}

/** Resolve a member or direct function call using native RPC ownership rules. */
export function serviceRpcMember(
  target: object,
  name: string,
  operation: "call" | "apply",
): (...args: unknown[]) => unknown;
/** Read an RPC property without exposing instance fields or inherited function fields. */
export function serviceRpcMember(
  target: object,
  name: string,
  operation: "get",
): unknown;
export function serviceRpcMember(
  target: object,
  name: string,
  operation: "call" | "get" | "apply",
): unknown {
  if (operation === "apply") {
    if (typeof target !== "function") unavailable();
    return target;
  }
  // This boundary receives admitted RpcTarget/WorkerEntrypoint instances or functions.
  // A class target remains a class target even if tenant code changes its prototype.
  const ownOnly = typeof target === "function";
  if (nativeOwn(target, name) !== ownOnly) unavailable();
  const value: unknown = nativeGet(target, name, target);
  if (!ownOnly && value === nativeGet(objectPrototype, name)) unavailable();
  if (operation === "call" && typeof value !== "function") unavailable();
  return value;
}
