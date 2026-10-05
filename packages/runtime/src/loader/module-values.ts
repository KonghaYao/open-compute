import type { RuntimeModule } from "./protocol.js";

export function bytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  const value = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) value[i] = binary.charCodeAt(i);
  return value;
}

export function moduleValue(module: RuntimeModule): WorkerLoaderModule {
  const raw = bytes(module.bytesBase64);
  switch (module.type) {
    case "esModule":
      return {
        js: new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(
          raw,
        ),
      };
    case "commonJsModule":
      return {
        cjs: new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(
          raw,
        ),
      };
    case "python":
      return {
        py: new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(
          raw,
        ),
      };
    case "text":
      return {
        text: new TextDecoder("utf-8", {
          fatal: true,
          ignoreBOM: false,
        }).decode(raw),
      };
    case "json":
      return {
        json: JSON.parse(
          new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(
            raw,
          ),
        ),
      };
    case "data":
      return { data: raw.buffer };
    case "wasm":
      return { wasm: raw };
    default:
      throw new Error("unsupported module representation");
  }
}
