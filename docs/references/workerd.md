# open-compute workerd fork

open-compute **不使用 Cloudflare 官方发布的 stock workerd 二进制**。生产运行时来自
[`elliothux/workerd`](https://github.com/elliothux/workerd)，它是
[`cloudflare/workerd`](https://github.com/cloudflare/workerd) 的定制 fork。`ocd` 会把与当前目标匹配的固定
workerd archive 内嵌进自己的单二进制发行物；启动时离线校验并物化，不从 `PATH` 查找，也不会下载或切换到 stock workerd。

当前 formal pin 的唯一 authority 是
[`packages/runtime/workerd.lock.json`](../../packages/runtime/workerd.lock.json)：

| 身份           | 当前值                                                                                             |
| -------------- | -------------------------------------------------------------------------------------------------- |
| fork release   | `v1.20260930.0-open-compute-r3.e3bdb07f5`                                                          |
| fork revision  | `e3bdb07f52affc6a618f02ed2b731a581b0b2f69`                                                         |
| upstream base  | `cb26acc62e64f64487a78e3f73d0a08e9926c690`（正式 pin 的 upstream base）                            |
| `--version`    | `workerd 2026-09-30`                                                                               |
| build workflow | [Open Compute binaries 36668461686](https://github.com/elliothux/workerd/actions/runs/36668461686) |

`workerd --version` 只显示上游日期，不能证明拿到的是本 fork。需要同时核对 revision、目标和 lock 中的 binary
SHA-256。

## Fork 扩展

fork 保留 upstream Worker runtime、module validation、RPC、Durable Objects 与 Loader 基础实现，只在 standalone
宿主缺少的边界增加下列能力：

| 扩展                       | fork 提供的能力                                                                                                                                                                   | open-compute 中的用途                                                                                                      |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Dynamic Worker Loader      | delegated Loader namespace、受约束 capability delegation、原生 `load` / `get`、entrypoint/RPC、Dynamic Durable Object facets、tails、in-flight accounting 与撤销生命周期          | 向普通 Worker 提供正式的 Worker Loader binding，同时保持 account、Script、Version 与 binding namespace 隔离                |
| Workers Standard limits    | standalone CPU、memory、startup CPU、subrequest 与 simultaneous outbound-connection enforcement；Dynamic Worker/entrypoint/delegated Loader ceiling 组合；超限 isolate 摘除与恢复 | 执行 Wrangler/v4 Settings 与 immutable Version 中的 limits，而不是只解析参数或依赖 Cloudflare 私有宿主                     |
| Native host extensions     | 私有 `HostExtensionFactory` / `HostExtensionPort`、generation broker fd 4、session-scoped Cap'n Proto unary/stream transport                                                      | 让 operator-owned native Provider 通过普通 Service Binding facade 服务 Worker；该 ABI 不是 Cloudflare 标准 API             |
| Dynamic binding forwarding | 私有 `openComputePrivateEnv`、host-issued Loader grant、handler-only capability table 与 generation/revocation fence                                                              | 由 `open-compute:worker-loader` 显式转发 KV、D1、R2、Queue 和普通值，同时不把根 binding 变成可 structured-clone 的公共对象 |
| Compatibility catalog      | `compatibility-catalog` 从编入 binary 的 maximum date 与 `CompatibilityFlags` schema annotations 生成确定性 JSON；不加载配置或启动 listener                                       | 让 build、CLI、management API、SDK 与 Dashboard 从 exact binary 发现同一 date/flag 合同                                    |
| Reproducible binaries      | 四目标优化构建 workflow、固定编译器/Bazel/config 与 canonical release archive 生成输入                                                                                            | 为 formal pin、GitHub Release build input 和跨平台产品 Gate 提供可复现来源                                                 |

这些扩展不代表 Cloudflare 托管平台采用相同内部实现。公开 Cloudflare-compatible surface 与 open-compute
私有扩展仍分别记录；实验性 Loader 控制、完整 Workers for Platforms 和 dispatch namespace 不会因为使用 fork 而自动获得支持。
实现与边界详见 [W1 Worker Loader](../implemented/w1-dynamic-workers-worker-loader.md)、
[W2 Standard limits](../implemented/w2-standard-limits.md)、
[W3 native bindings](../implemented/w3-user-extensible-native-bindings.md) 和
[I102 binding forwarding](../implemented/i102-dynamic-worker-binding-forwarding.md)。

## 单独下载 workerd

以下 archive 来自 fork 的不可变 GitHub Release，而不是 Cloudflare release。下载后必须同时核对 archive 与解压 binary
SHA-256；唯一 authority 仍是 formal lock。

二进制沿用 workerd 的 Apache 2.0 license，并包含 upstream source tree 记录的第三方组件；它们由 open-compute
项目发布和支持，不是 Cloudflare 官方发行物。

| Target           | Archive                                                                                                                                           | Archive SHA-256                                                    | Binary SHA-256                                                     |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| macOS ARM64      | [workerd-darwin-arm64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r3.e3bdb07f5/workerd-darwin-arm64.gz) | `632d2ee13c684e70200bacd0bf1583360d430ad1054ef3cb29e0c20ef52481e0` | `d489faf23b0ecf7bfa8edccac5e6d5295f262f7f02f392c7ae92b06d95f55ddf` |
| Linux GNU ARM64  | [workerd-linux-arm64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r3.e3bdb07f5/workerd-linux-arm64.gz)   | `76d00935975e38c1b114771903cac54644dc1cde9a9c1b68a1afbd6829822f6b` | `744a56e9b28728ead237765abc6cbded98560acc4bbfb5339d21e184394839d5` |
| Linux GNU x86-64 | [workerd-linux-64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r3.e3bdb07f5/workerd-linux-64.gz)         | `5145314dbd608857bbd765cc479a609619f0cf380bb91d9704303d218816fa48` | `40424924678782e02d6a68a1af512b1a9dacdf93b16e47c2d7f4825a09f5e336` |
| macOS x86-64     | [workerd-darwin-64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r3.e3bdb07f5/workerd-darwin-64.gz)       | `cd7c543b688120ccb5fdc57706c0e0bddbdde6ee140b55fd7f30cc3e3524c015` | `9149f00c2f2b7bd576f6b164e924ed306562845ca72a8207d99ff6774c58753e` |

例如下载 Linux x86-64 版本：

```sh
curl -fL https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r3.e3bdb07f5/workerd-linux-64.gz -o workerd.gz
echo '5145314dbd608857bbd765cc479a609619f0cf380bb91d9704303d218816fa48  workerd.gz' | sha256sum -c -
gzip -dc workerd.gz > workerd
echo '40424924678782e02d6a68a1af512b1a9dacdf93b16e47c2d7f4825a09f5e336  workerd' | sha256sum -c -
chmod +x workerd
./workerd --version
```

macOS 可把校验命令替换为 `shasum -a 256 workerd`。单独 binary 适合源码/配置验证和 fork 调试；它不包含 `ocd`
拥有的实例注册、SQLite authority、deployment compilation、secret、Gateway、supervision 与恢复逻辑，不能替代完整
open-compute 安装。

## 升级与源码

fork 源码由 [`third_party/workerd/`](../../third_party/workerd/) submodule 固定。每次升级必须一起更新 fork revision、
upstream base、四目标 release asset、archive/binary digest、binary maximum、compatibility catalog schema/digest、system Worker
date/flags 和真实 runtime Gate；不能只替换其中一个文件。四目标 workflow 必须执行 introspection 两次并比较全部
catalog 字节；根 build 再对 host binary 重算 catalog，任何 revision/binary/maximum/catalog drift 都在 Cargo 前失败。上游能力与
fork-ahead 审查见 [workerd upstream](workerd-upstream.md)，构建和内嵌合同见
[单二进制分发](single-binary.md)。
