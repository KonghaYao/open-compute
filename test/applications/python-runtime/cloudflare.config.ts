import { bindings, defineConfig, exports } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-runtime-fixture",
    entrypoint: "./src/main.py",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [
      "python_workers",
      "enable_python_external_sdk",
      "python_dedicated_snapshot",
    ],
    exports: { default: exports.worker() },
    assets: {
      runWorkerFirst: true,
      htmlHandling: "none",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
      IMAGES: bindings.images(),
      AI: bindings.ai(),
      VECTORS: bindings.vectorize({ name: "python-runtime-vectors" }),
      ARTIFACTS: bindings.artifacts({ namespace: "python-runtime-apps" }),
      SEARCH: bindings.aiSearchNamespace({ namespace: "default" }),
      DIRECT_SEARCH: bindings.aiSearch({ name: "python-runtime-search" }),
      ISOLATED_SEARCH: bindings.aiSearchNamespace({
        namespace: "python-runtime-isolated",
      }),
      REVISION: bindings.text("first"),
      TOKEN: bindings.secret(),
      OUTBOUND_URL: bindings.text("http://127.0.0.1:1/"),
      KV: bindings.kv({ id: "11111111111111111111111111111111" }),
      DB: bindings.d1({
        id: "019c0000-0000-7000-8000-000000000006",
        name: "python-main-db",
      }),
      BUCKET: bindings.r2({ name: "python-main-bucket" }),
    },
  },
});
