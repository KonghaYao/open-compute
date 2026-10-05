---
title: "日志与实时 Tail"
description: "在选中的 open-compute instance 上持久化、查询并实时查看 Worker 日志。"
---

日志通过 Dashboard Live Tail 或现有 SDK/API 查询。cf 目前没有等价的终端流式 tail；服务端日志和 live tail 能力仍保留。

## 配置日志

使用 cf 的 `observability` 配置。支持日志启用、sampling、invocation log 与持久化；不支持外部 log destination 与 trace。

```ts
// worker.observability in cloudflare.config.ts
({
  observability: {
    enabled: true,
    headSamplingRate: 1,
    logs: { enabled: true, invocationLogs: true, persist: true },
    traces: { enabled: false },
  },
});
```

持久化 telemetry 支持 `events` 与 `invocations` query view、key/value discovery、filter，以及限定到选定 script 的 live-tail session。retention、database size、invocation log size、query timeframe 与 event count、ingest queue capacity、tail session 数与 tail client buffer 均受 instance 配置限制。

## 本机差异

Telemetry 存在选中的 instance，而不是 Cloudflare 全球 analytics service。hosted-only metadata 会省略，regex filter 使用有界 RE2-compatible subset，且不提供 account-wide live tail。Calculations、traces、agents、requests、saved queries、Tail Workers、Logpush 与 external destination 均不支持。

参见[行为差异](/zh/docs/platform/deviations/)、[限制](/zh/docs/platform/limits/)与[管理 SDK](/zh/docs/platform/reference/sdk/)。
