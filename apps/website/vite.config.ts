import { cloudflare } from "@cloudflare/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  publicDir: "dist",
  build: { outDir: ".cloudflare/vite" },
  plugins: [cloudflare({ types: { generate: false } })],
});
