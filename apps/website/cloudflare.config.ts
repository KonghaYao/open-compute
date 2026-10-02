import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "open-compute-website",
    entrypoint: "./src/worker.ts",
    compatibilityDate: "2026-09-08",
    placement: { mode: "smart" },
    assets: {
      notFoundHandling: "404-page",
      runWorkerFirst: ["/api/*"],
    },
    env: { ASSETS: bindings.assets() },
    cache: { enabled: true },
    observability: { enabled: true },
  },
});
