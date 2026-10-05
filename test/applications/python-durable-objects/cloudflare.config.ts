import { bindings, defineConfig, exports } from "cf/config";

export default defineConfig(({ mode }) => {
  const retired = mode === "retire";
  return {
    worker: {
      name: "python-durable-objects-fixture",
      entrypoint: retired ? "./retired/main.py" : "./src/main.py",
      compatibilityDate: "2026-09-08",
      compatibilityFlags: [
        "python_workers",
        "enable_python_external_sdk",
        "python_dedicated_snapshot",
      ],
      exports: {
        default: exports.worker(),
        Counter: retired
          ? exports.durableObject({ state: "deleted" })
          : exports.durableObject({ storage: "sqlite" }),
      },
      env: retired
        ? { REVISION: bindings.text("retired"), TOKEN: bindings.secret() }
        : {
            REVISION: bindings.text("first"),
            TOKEN: bindings.secret(),
            OBJECTS: bindings.durableObject({
              worker: "python-durable-objects-fixture",
              exportName: "Counter",
            }),
          },
    },
  };
});
