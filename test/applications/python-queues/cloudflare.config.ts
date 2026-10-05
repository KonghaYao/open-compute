import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-queues-fixture",
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
      EVENTS: bindings.queue({ name: "python-queues-events" }),
      KV: bindings.kv({ id: "019c0000-0000-7000-8000-000000000002" }),
    },
  },
});
