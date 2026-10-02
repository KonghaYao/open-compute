---
title: "Compatibility flags"
---

项目使用 cf 标准 `compatibility_flags` 数组。open-compute 原样保留该数组，由 pinned workerd binary 接受或拒绝完整的日期/flag 组合。

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "hello-typescript",
    entrypoint: "src/index.ts",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [],
  },
});
```

使用 cf 的 snake_case 字段。`GET /client/v4/open-compute/capabilities` 返回 exact binary reflection 得到的全部 enable/disable input、默认日期、implication 与 experimental 标记。catalog 只用于发现，部署仍执行 workerd validation。内部 system flags 仍属于 executable identity，不复制到项目配置。

正式 self-host runtime 以 workerd experimental process mode 运行，因此 catalog 标记为 experimental 的输入在 binary 接受时可部署。Cloudflare hosted Dynamic Workers 文档明确说明 experimental flags 不能在 production 启用；不要把本地 admission 解读为 hosted-production availability。

| 主题                       | Cloudflare                                                                                                     | open-compute                              |
| -------------------------- | -------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| flag 名称与语义            | [Cloudflare compatibility flags](https://developers.cloudflare.com/workers/configuration/compatibility-flags/) | 来自 workerd 的相同名称                   |
| 项目 `compatibility_flags` | 是                                                                                                             | 按不可变 Version 持久化和验证             |
| 未知、重复或冲突 flag      | upload 失败                                                                                                    | pinned workerd 拒绝 candidate             |
| 实际支持集合               | Dashboard / cf                                                                                                 | exact binary 内嵌的 compatibility catalog |
