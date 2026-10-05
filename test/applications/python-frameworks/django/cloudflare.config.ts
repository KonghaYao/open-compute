import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "python-django-fixture",
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
    },
  },
});
