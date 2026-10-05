---
title: "Configuration"
---

Official `cloudflare.config.ts` / `cf/config` owns configuration. `ocd cf` checks files and selects a target; it does not parse TypeScript or bindings.

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "app",
    entrypoint: "src/index.ts",
    compatibilityDate: "2026-09-08",
    workersDev: false,
    limits: {
      cpuMs: 60000,
      subrequests: 20000,
    },
    env: {
      LOG_LEVEL: bindings.text("info"),
    },
  },
});
```

Declare variables and bindings with `worker.env` / `bindings`, and DO / Workflow lifecycle with `worker.exports`. Vite owns bundling, assets directories, and source maps. Official Build Output lives under `.cloudflare/`; there is no old deployment-config redirect.

A type or configuration accepted by the official tools does not imply OCD supports the product. Unsupported bindings are rejected at the upload authority.
