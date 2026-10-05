import { bindings, defineConfig, exports } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-workflows-fixture",
    entrypoint: "./src/main.py",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [
      "python_workers",
      "enable_python_external_sdk",
      "python_dedicated_snapshot",
    ],
    exports: {
      default: exports.worker(),
      Flow: exports.workflow({ name: "python-workflows-flow" }),
    },
    env: {
      REVISION: bindings.text("first"),
      TOKEN: bindings.secret(),
      KV: bindings.kv({ id: "11111111111111111111111111111111" }),
      FLOW: bindings.workflow({
        name: "python-workflows-flow",
        worker: "python-workflows-fixture",
        exportName: "Flow",
      }),
    },
  },
});
