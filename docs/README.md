# 文档索引

| 内容                     | 权威入口                                                                                    |
| ------------------------ | ------------------------------------------------------------------------------------------- |
| 已实现架构与产品维护     | [完成索引](implemented/README.md)                                                           |
| 正式版本说明             | [Release notes](releases/README.md)                                                         |
| 当前 API 支持与偏差      | [兼容矩阵](references/cloudflare-compatibility.md)、[偏差清单](references/p1-deviations.md) |
| 开发测试、部署与运维     | [参考文档](references/README.md)                                                            |
| 原生运行时实施与后续工作 | [workerd 路线](workerd/README.md)；源码基于 `third_party/workerd/` submodule                |
| 其他待实现设计           | 下表；外部前置阻塞见 [blocked](blocked/README.md)                                           |

已完成文档保留实现职责、关键边界和实际验收结果；重复规则引用权威入口，不再保留实施过程、独立结果副本或废弃方案比较。
历史 PASS 不代表当前工作树已验收；必须原样保留的生成报告会单独标明。

## 待实施

| 文档                                                                               | 当前状态                                                                                                                                                                                                               |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [P20 Cloudflare CLI 单轨迁移与官方应用构建链](p20-cf-cli-migration.md) | planned：以 `ocd cf` 替换 Wrangler launcher；官方 `cf migrate --bundler vite`、Vite 插件和类型生成取代自有应用工具链；OCD 仅保留目标、凭据、有限预检与 API 边界，待真实 E2E 验收 |
| [P21 Cloudflare Python Workers](p21-python-workers.md)                             | planned：复用 workerd/Pyodide 官方 entrypoint，把 binding construction 下沉为语言无关 materializer；新增主部署链 blocking Gate，并补齐 snapshot、package/framework 与全 binding differential qualification；开发/部署入口与 P20 单轨 cf 方案联合验收 |
| [P22 Cloudflare Browser Run](p22-browser-run.md)                                   | Day 1 合同与单文件分发架构完成；`ocd` 内嵌压缩 Browser Runtime、首次使用时离线物化并完整监督；待 BR-G0 在 `chrome-headless-shell` 与 Obscura 中选择一个正式引擎                                                        |
| [P23 Cloudflare Containers](p23-cloudflare-containers.md)                          | Day 1 合同与两阶段 provider 路线完成；短期依赖宿主 Docker + restricted Broker，长期以 BoxLite 或其他待 G0 的可嵌入 runtime + Docker 子集 shim 替换；受 dynamic DoHost/workerd attachment 与真实 engine/package G0 阻断 |
| [P24 macOS Developer ID 签名与 Apple 公证](p24-macos-code-signing-notarization.md) | Day 1 发行合同与 CI 方案完成；待配置受保护的 Apple/GitHub 凭据、签署最终 `ocd`、取得 Notary `Accepted` 并完成真实 tag 验收                                                                                             |
| [P25 平台后续能力](p25-platform-follow-ups.md)                                     | planned：保留尚未拆成独立阶段的 instance 显式 `.env`、operator logger 与 lazy Worker startup 工作；应用项目迁移由 P20 接管                                                                                                               |

P20 是后续应用 CLI/构建入口的权威方案；P21 的旧部署工具描述在联合实施时按 P20 替换，Python runtime 与 package/snapshot 合同仍由 P21 负责。

实现完成后移入 `implemented/`；仍需实施的限制、功能缺陷和 TODO 留在活动方案，真正无法继续的外部阻塞移入
`blocked/`。验证矩阵、Gate 和测试清单不作为独立活动文档保留。
