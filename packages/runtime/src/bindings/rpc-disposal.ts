const nativeGet = Reflect.get;
const nativeApply = Reflect.apply;
const nativeDefine = Object.defineProperty;
const disposeSymbol = Symbol.dispose;

/** Retain a native RPC result's group disposer when decoding its contents. */
export function preserveRpcDisposal<T extends object>(
  source: object,
  output: T,
): T {
  const dispose: unknown = nativeGet(source, disposeSymbol, source);
  if (typeof dispose === "function") {
    nativeDefine(output, disposeSymbol, {
      value: () => nativeApply(dispose, source, []),
      configurable: true,
      writable: true,
    });
  }
  return output;
}
