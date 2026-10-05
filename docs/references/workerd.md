# open-compute workerd fork

open-compute **不使用 Cloudflare 官方发布的 stock workerd 二进制**。生产运行时来自
[`elliothux/workerd`](https://github.com/elliothux/workerd)，它是
[`cloudflare/workerd`](https://github.com/cloudflare/workerd) 的定制 fork。`ocd` 会把与当前目标匹配的固定
workerd archive 内嵌进自己的单二进制发行物；启动时离线校验并物化，不从 `PATH` 查找，也不会下载或切换到 stock workerd。

当前 formal pin 的唯一 authority 是
[`packages/runtime/workerd.lock.json`](../../packages/runtime/workerd.lock.json)：

| 身份           | 当前值                                                                                             |
| -------------- | -------------------------------------------------------------------------------------------------- |
| fork release   | `v1.20260930.0-open-compute-r4.e98a3e843`                                                          |
| fork revision  | `e98a3e8433979356047a202d3e0d1b0e2e2c4b8c`                                                         |
| upstream base  | `d99bc6b777e35d72d71c2f1fe2fd1db53284528a`（正式 pin 的 upstream base）                            |
| `--version`    | `workerd 2026-09-30`                                                                               |
| build workflow | [Open Compute binaries 37175389986](https://github.com/elliothux/workerd/actions/runs/37175389986) |

四个平台的 build job 均成功。该 run 的 publish job 因 artifact 内二进制保留 `bazel-bin/src/workerd/server/` 路径而失败；发布使用原始成功产物恢复，没有重新编译或改变 binary。archive 使用 macOS `Apple gzip 487.0.1` 的 `gzip -9n`，Release 同时包含逐目标 build-inputs、catalog 与 `SHA256SUMS`。

源码 checkout 的 `fdcb8c97f0b9f75f610b66e418d5175b9188ddad` 仅追加发布路径修正；正式 binary 仍来自上表的 `e98a3e8433979356047a202d3e0d1b0e2e2c4b8c`。R4 固定 P21 原生准备与 binding policy primitives，不代表完整 Python 产品资格已通过。

`workerd --version` 只显示上游日期，不能证明拿到的是本 fork。需要同时核对 revision、目标和 lock 中的 binary
SHA-256。

## Fork 扩展

fork 保留 upstream Worker runtime、module validation、RPC、Durable Objects 与 Loader 基础实现，只在 standalone
宿主缺少的边界增加下列能力：

| 扩展                       | fork 提供的能力                                                                                                                                                                   | open-compute 中的用途                                                                                                      |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Dynamic Worker Loader      | delegated Loader namespace、受约束 capability delegation、原生 `load` / `get`、entrypoint/RPC、Dynamic Durable Object facets、tails、in-flight accounting 与撤销生命周期          | 向普通 Worker 提供正式的 Worker Loader binding，同时保持 account、Script、Version 与 binding namespace 隔离                |
| Workers Standard limits    | standalone CPU、memory、startup CPU、subrequest 与 simultaneous outbound-connection enforcement；Dynamic Worker/entrypoint/delegated Loader ceiling 组合；超限 isolate 摘除与恢复 | 执行 cf/v4 Settings 与 immutable Version 中的 limits，而不是只解析参数或依赖 Cloudflare 私有宿主                           |
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
| macOS ARM64      | [workerd-darwin-arm64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r4.e98a3e843/workerd-darwin-arm64.gz) | `35866787c114955cf321761495eeefa21ec6dfcf6969f6960ab8b5abb853d78a` | `5fcc34038f37c5a42668193f31bc1a9eeb4355f42f8188b312d03c77f3efb788` |
| Linux GNU ARM64  | [workerd-linux-arm64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r4.e98a3e843/workerd-linux-arm64.gz)   | `e8e1eda68ca7058cce04f6ac07fad123a1de967565fa284bb1d8c88507b62cb2` | `7f23d3fceb6ea9406e33d1acc1105d67ddbc25941a3498dd6f33c574b716ca0f` |
| Linux GNU x86-64 | [workerd-linux-64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r4.e98a3e843/workerd-linux-64.gz)         | `22a6b617c8ed4fd2e577cf19111d842b4e596ca2137cfafb082b25bcdcf9d87b` | `5f9f5152df987be874e6092dbc56b28023d00e6c54b361617618d32071606ce5` |
| macOS x86-64     | [workerd-darwin-64.gz](https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r4.e98a3e843/workerd-darwin-64.gz)       | `c7342e5579d798f184e7d0320087686060fc8819f00c096aa4d45b155941c675` | `ced654c8982520783a29f23cdb0d159cd3290ce07b2a30d5fa739741a8d08050` |

例如下载 Linux x86-64 版本：

```sh
curl -fL https://github.com/elliothux/workerd/releases/download/v1.20260930.0-open-compute-r4.e98a3e843/workerd-linux-64.gz -o workerd.gz
echo '22a6b617c8ed4fd2e577cf19111d842b4e596ca2137cfafb082b25bcdcf9d87b  workerd.gz' | sha256sum -c -
gzip -dc workerd.gz > workerd
echo '5f9f5152df987be874e6092dbc56b28023d00e6c54b361617618d32071606ce5  workerd' | sha256sum -c -
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
