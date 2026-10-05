import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-main-fixture",
    entrypoint: "./src/main.py",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [
      "python_workers",
      "enable_python_external_sdk",
      "python_dedicated_snapshot",
    ],
    env: {
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
