---
title: "Static Assets"
---

把静态文件冻进同一份不可变 deployment。配置在 `assets`。

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "site",
    entrypoint: "src/index.ts",
    assets: {
      runWorkerFirst: false,
      htmlHandling: "auto-trailing-slash",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
    },
  },
});
```

`worker.assets.htmlHandling`：`auto-trailing-slash`（默认）、`force-trailing-slash`、`drop-trailing-slash`、`none`。`worker.assets.notFoundHandling`：`none`（默认）、`404-page`、`single-page-application`。`worker.assets.runWorkerFirst` 可以是 boolean，或一组以 `/` / `!/` 开头的路径规则。Assets-only 项目省略 `worker.entrypoint`，也不能声明执行环境或 Worker-first。

binding 存在时，`env.<binding>.fetch()` 只取资源，不进入租户 Worker。

## 兼容性

| 主题                                                                       | Cloudflare                                                                                  | open-compute                                           |
| -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| HTML trailing slash、SPA、Worker-first、`_headers` / `_redirects` 路由概念 | 是，见 [Cloudflare Static Assets](https://developers.cloudflare.com/workers/static-assets/) | 对齐                                                   |
| 对象存储                                                                   | 全球 CDN                                                                                    | 选定 Local/S3 authority 上的不可变对象，由本机提供服务 |
| 全球 CDN placement / 复制 / purge 传播 / 产品配额                          | 是                                                                                          | 不提供                                                 |
| Pages 产品迁移向导                                                         | 是                                                                                          | 不提供                                                 |

通过 Vite `publicDir` 设置资源目录，通过 `bindings.assets()` 声明 fetch binding。通过 Vite `build.sourcemap` 配置 source map 输出。
