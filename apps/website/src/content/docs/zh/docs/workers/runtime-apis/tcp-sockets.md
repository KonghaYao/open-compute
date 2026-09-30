---
title: "TCP sockets"
---

`connect()` 从 `cloudflare:sockets` 导入，用于建立出站 TCP。API 形状与 [Cloudflare TCP sockets](https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/) 对齐；网络策略边界不同。

```ts
import { connect } from "cloudflare:sockets";

export default {
  async fetch(request: Request): Promise<Response> {
    const socket = connect({ hostname: "example.com", port: 80 });
    const writer = socket.writable.getWriter();
    await writer.write(new TextEncoder().encode("GET / HTTP/1.0\r\n\r\n"));
    await writer.close();
    return new Response(socket.readable, {
      headers: { "Content-Type": "text/plain" },
    });
  },
} satisfies ExportedHandler;
```

完整 `Socket` / `SocketAddress` / `SocketOptions` / `startTls()` 签名见 Cloudflare 原文。不可在 global scope 创建并跨请求共享 socket。

## 兼容性

| 主题                                                                                                                     | Cloudflare                                                                                                    | open-compute                                                                       |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `connect(address, options?)` 返回带 `readable` / `writable` / `opened` / `closed` / `close()` / `startTls()` 的 `Socket` | 是                                                                                                            | 是                                                                                 |
| `secureTransport`：`off` \| `on` \| `starttls`                                                                           | 是                                                                                                            | 是                                                                                 |
| 租户通用出站 `fetch()`、`cloudflare:sockets.connect()`、`node:net`、`node:tls`                                           | Cloudflare 托管网络策略                                                                                       | 共享所属实例的 `Network(allow=["network","local"], deny=["unix","unix-abstract"])` |
| 命名 Service/DO 的 `Fetcher.connect()`                                                                                   | 托管策略                                                                                                      | 使用绑定声明的连接，而非第二条通用出站通道                                         |
| Cloudflare 自有 IP 段封禁 / Worker self-connect（TCP Loop）/ 默认 SMTP 25 封禁                                           | 是，见 [troubleshooting](https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/#troubleshooting) | 不提供                                                                             |
| public / private / loopback / link-local / metadata IP 目标                                                              | Cloudflare 应用托管限制                                                                                       | 宿主网络可路由时即可访问                                                           |
| Unix 与 abstract-Unix socket                                                                                             | 不暴露                                                                                                        | 不暴露                                                                             |

open-compute 不提供 per-Worker 或 per-instance 网络隔离。operator 应通过宿主 firewall、network namespace、容器或 VM 过滤目标；每个平台注册 listener 都必须独立认证，不能依赖来源地址。
