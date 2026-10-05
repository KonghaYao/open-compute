import { bindings, defineConfig, exports } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-services-fixture",
    entrypoint: "./src/main.py",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [
      "python_workers",
      "enable_python_external_sdk",
      "python_dedicated_snapshot",
    ],
    exports: { default: exports.worker(), NamedApi: exports.worker() },
    env: {
      REVISION: bindings.text("first"),
      TOKEN: bindings.secret(),
      TARGET: bindings.worker({ worker: "python-services-js-target" }),
      NAMED: bindings.worker({
        worker: "python-services-js-target",
        exportName: "NamedApi",
      }),
    },
  },
});
