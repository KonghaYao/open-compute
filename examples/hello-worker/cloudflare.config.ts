import { bindings, defineConfig } from "cf/config";

export default defineConfig(({ mode }) => ({
  worker: {
    name: `hello-typescript-${mode}`,
    entrypoint: "./src/index.ts",
    compatibilityDate: "2026-09-08",
    workersDev: false,
    env: { GREETING: bindings.text(`Hello from ${mode}`) },
  },
}));
