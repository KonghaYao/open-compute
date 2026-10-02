import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "oc-p4-vinext",
    entrypoint: "./worker/index.ts",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: ["nodejs_compat"],
    env: {
      P4_PUBLIC_MARKER: bindings.text("p4-public-marker"),
      IMAGES: bindings.images(),
      VERSION: bindings.versionMetadata(),
      ASSETS: bindings.assets(),
    },
    assets: { notFoundHandling: "none" },
    cache: { enabled: true },
  },
});
