const NativeError = Error;
const nativeDescriptor = Object.getOwnPropertyDescriptor;
const nativeDefine = Object.defineProperty;

function data(error: unknown, name: string): unknown {
  if (
    error === null ||
    (typeof error !== "object" && typeof error !== "function")
  )
    return undefined;
  try {
    return nativeDescriptor(error, name)?.value;
  } catch {
    return undefined;
  }
}

type DoError = Error & {
  stableCode: string;
  durableObjectReset?: boolean;
};

function safeError(code: string, reset: boolean): DoError {
  const safe = new NativeError(code) as DoError;
  nativeDefine(safe, "stableCode", { value: code, enumerable: true });
  safe.stack = `Error: ${code}`;
  if (reset)
    nativeDefine(safe, "durableObjectReset", { value: true, enumerable: true });
  return safe;
}

/** Preserve native reset classification without exposing upstream exception details. */
export function sanitizeDoError(error: unknown, code?: string): DoError {
  const message = data(error, "message");
  const selected =
    code ??
    (typeof message === "string"
      ? /\b(DO_[A-Z_]+)\b/.exec(message)?.[1]
      : undefined) ??
    "DO_RUNTIME_EXCEPTION";
  return safeError(selected, data(error, "durableObjectReset") === true);
}
