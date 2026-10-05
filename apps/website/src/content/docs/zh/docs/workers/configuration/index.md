---
title: "配置"
---

官方 `cloudflare.config.ts` / `cf/config` 是配置权威。`ocd cf` 只做文件预检和目标选择，不解析 TS 或绑定。

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

应用使用 `worker.env` / `bindings` 声明变量和绑定，使用 `worker.exports` 声明 DO / Workflow 生命周期；Vite 配置负责 bundling、assets 目录与 source maps。官方 Build Output 位于 `.cloudflare/`，不再使用旧部署配置重定向。

官方工具暴露的类型或配置不代表 OCD 支持对应产品；不支持的 binding 在上传 authority 处拒绝。
