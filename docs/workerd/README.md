# workerd 原生运行时方案

当前待实施：[W4：workerd fork 收薄](w4-thin-fork.md)。该方案复核 W1/W2/W3、R3 与 P21 候选的全部差异，区分必要原生接口和可替代的平台策略；W4 尚未实施；当前 R4 formal pin 记录见下文。下文已有资格记录保留其原始输入与适用范围。

W1 的逐 surface 复核见[兼容审查记录](../implemented/w1-worker-loader-compatibility-review.md)。

状态：**W1/W2/W3 与正式 pin 均已完成产品 qualification**。W2 的 Wrangler/v4 配置、Dynamic Worker ceiling、原生执行、公开错误、
isolate 摘除与 supervisor 自恢复已在同一 Day1 路径完成资格化。四个平台的源码 revision、二进制与 digest
由 formal lock 固定。2026-09-05 用户确认接受维护自己的 workerd fork 并重新编译。
W1/W2/W3 不再以“等待上游合并后才能开发”为实施前提；public Loader 已接入原生 fork。W2 同时交付原生
ResourceLimits、超限 isolate 摘除，以及 generation-fenced supervisor 功能性探活与自动恢复。

2026-09-06 调整交付顺序：先完成 W1 原生 Loader，再实现 W2 Standard limits。W1 的范围不包含默认
CPU/内存/subrequest enforcement 或 custom limits；显式 limits 必须由原生 API 拒绝，不能静默忽略。
W1 已完成 namespace/权限隔离、结构大小限制、in-flight 计数、缓存与生命周期及正式 pin 验收。
W2 通过请求限额、isolate 摘除和执行器自恢复三层机制消除失控代码影响同进程邻居的已知故障，并完成
Wrangler、v4 Settings、Dynamic Worker ceiling、官方错误分类和完整产品验收。

## 源码与运行时基线

**后续 workerd 修改统一基于仓库中的 [`third_party/workerd/`](../../third_party/workerd/)。**
该目录是用户 fork 的 Git submodule，由根目录 [`.gitmodules`](../../.gitmodules) 登记远端，父仓库 gitlink 固定源码提交。
不要另建一份 workerd 实现、复制到其他目录，
或为了匹配旧的测试二进制而重置这个 checkout。

| 项目                                                              | 2026-10-04 核对结果                                                                                                                                                                                                                                 |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [workerd 上游 issue / PR 核验](../references/workerd-upstream.md) | 已合并能力、standalone 缺口、补丁范围与升级回归重点                                                                                                                                                                                                 |
| fork origin                                                       | <https://github.com/elliothux/workerd>                                                                                                                                                                                                              |
| upstream 项目                                                     | <https://github.com/cloudflare/workerd>                                                                                                                                                                                                             |
| fork checkout HEAD                                                | `fdcb8c97f0b9f75f610b66e418d5175b9188ddad`（已推送 fork 工作分支，追加发布路径修正）                                                                                                                                                                |
| upstream base                                                     | `d99bc6b777e35d72d71c2f1fe2fd1db53284528a`（2026-09-30 upstream `main`）                                                                                                                                                                            |
| HEAD 提交说明                                                     | `0177c203f` P21 原生准备与 binding policy、`a692581eb` Python Queue 单 batch 参数、`c92952f47` SDK fixture export、`e98a3e843` scheduled class handler 参数；`fdcb8c97f` 发布 artifact 路径修正                                                     |
| fork working tree                                                 | 本次记录时 clean；相对上述 upstream base ahead 25（含 merge 与 fork docs/workflow commits）                                                                                                                                                         |
| fork source tree                                                  | `6afa9633528383cdbbd47df4c91d1730d2cbf66a55b99b833499693ad97bc222`（checkout 的 `sha256(git-ls-tree-r-full-tree)`；正式构建来源另由 lock 固定）                                                                                                     |
| W3 runtime                                                        | `workerd 2026-09-30`；native Provider FD/Cap'n Proto unary/stream 回归通过                                                                                                                                                                          |
| open-compute 当前正式 pin                                         | `v1.20260930.0-open-compute-r4.e98a3e843` / `e98a3e8433979356047a202d3e0d1b0e2e2c4b8c`（[run 37175389986](https://github.com/elliothux/workerd/actions/runs/37175389986) 四平台 build 成功，发布 job 路径失败后用原产物恢复；逐目标 digest 已校验） |
| 正式 pin authority                                                | [`packages/runtime/workerd.lock.json`](../../packages/runtime/workerd.lock.json)                                                                                                                                                                    |

源码 checkout 在正式构建 revision 上追加了发布路径修正，formal binary 仍固定为 `e98a3e843`。
fork 的 `--version` 不能替代源码身份与二进制摘要；各项历史资格结果仅适用于其原始输入。正式切换必须完成构建、固定来源和协议、更新所有 pin 消费者及验证。
formal lock 还固定 system Worker date/flags、binary maximum 和 exact binary 生成的 compatibility catalog digest；四目标
catalog 必须逐字节一致。tenant Version 的 date/flags 不使用 system 值，也不由 open-compute allowlist 判断。
初始迁移保留 `main` 分支与 origin。W1 已完成原生本地提交和三个正式平台优化构建，正式 fork pin 已通过本机完整产品验收；macOS Intel 仅保留手动编译输入，
详见 [W1 实施记录](../implemented/w1-dynamic-workers-worker-loader.md)。

## Submodule 工作流

在 open-compute 根目录初始化已有 checkout，或首次克隆时一并获取固定的源码提交：

```sh
git submodule update --init -- third_party/workerd
# 首次克隆可使用：
git clone --recurse-submodules https://github.com/elliothux/open-compute.git
```

初始化可能访问网络；本次直接移动已有 checkout 并登记，没有重新克隆或下载。
`git submodule update` 使用父仓库记录的提交，不使用 `--remote` 自动追踪 fork 最新版本；
更新前先检查并保存子仓库的本地改动。初始化通常得到 detached HEAD，开发前在子仓库创建工作分支。

后续先在 `third_party/workerd/` 内提交源码，再在父仓库通过 `git add third_party/workerd` 记录新的 gitlink，
与相关平台代码和 docs 一起提交。父仓库只保存子仓库提交 ID，不会保存未提交的 fork 文件改动。
共享父仓库更新前必须确保被引用的提交已在 fork 远端可获取；push 仍需相应外部写入授权。
迁移后子仓库 Git 元数据由父仓库 `.git/modules/third_party/workerd/` 管理，旧源码目录不保留副本或别名。

构建平台从 formal lock 指定的 fork GitHub Release 显式下载并验证目标 archive；根 build 不隐式联网。
源码辅助的 conformance 校验在子仓库初始化后，
通过 `git show <正式 pin revision>:<path>` 读取固定版本，不把开发 checkout 当作正式运行时或 npm types 基线；
缺少所需 Git 对象时校验失败，不自动下载或改用 HEAD。后续源码与 pin 升级需保持这些对象可获取。

先读取 fork 的
[开发规则](../../third_party/workerd/AGENTS.md)和修改组件已有的规则，保持其 Bazel/C++/测试布局。

P21 当前运行时为上表的正式 R4 pin，包含部署时原生 preparation、语言无关 binding construction/policy、Python Queue 单 batch 参数与 scheduled class handler 参数。内部 waitUntil observer 通过既有 IoContext/AsyncContextFrame 登记原 promise；原生 DO state 与 handler 继续使用官方 Python helper，不修改 SDK 文件。普通 Main、三框架、Service、Queue、DO、Workflow、Runtime 及现有产品 Gate 已通过完整 workspace coverage 和最终单轮 Gate，输入与已知限制见 [P21 Python Workers 实现](../implemented/p21-python-workers.md)。历史候选 probe 不作为当前 pin 的验收证据；发布复用已有四平台产物，没有重新 native build。

## R4 发布与输入校验（2026-10-04）

用户授权后，候选 `e98a3e843` 已推送并完成四平台优化构建。原 publish job 对 artifact 的 binary 路径假设错误；
源码修正为 `fdcb8c97f`，发布使用原 run 的成功产物恢复，release tag 仍指向实际 binary 的构建 revision。
没有把 workflow 修正提交标为二进制来源。10 项 release asset 的 GitHub digest、`SHA256SUMS`、四目标
archive/binary digest、目标架构、逐目标 build-inputs 与逐字节一致 catalog 已校验；macOS ARM64 输入已
通过 canonical `prepare-workerd.ts --archive` 准备。发布与下载证据保存在 `.temp/p21-workerd-release-r4/`。
workerd fork 的其他 22 个 GitHub workflow 已在 repository settings 禁用，只保留
`Open Compute binaries` 的 `workflow_dispatch` 手动发布入口；当前没有待取消的其他 run。
上游 workflow 源码与历史 run 保留，不执行其自动/其他手动 CI。

正式 lock 唯一 CLI 字段为 `cfVersion`，固定 cf `1.0.0-beta.12`；Vite plugin 为 `2.0.0-beta.sha-52b0dc0e9`，
匹配 workers-sdk revision `52b0dc0e99b5bbdb86c06ed53fb703048efc36cf`。该 SDK revision 是开发工具来源，
不替代 Python `workers-runtime-sdk` 版本。新正式 binary 的 Runtime/PythonPreparation 两 profile 真实编译和 SDK scheduled prepare/restore 组件通过。
此前 R3 和开发候选的报告作为历史证据保留；P21 各 ordinary case 已有正式 pin 的 development pass，完整 workspace coverage 与最终单轮 Gate 仍需通过。

## 文档

本目录按交付顺序编号：W1 为 Dynamic Worker Loader，W2 为 Standard limits。历史平台 P9/P10 记录中的编号
保留为当时的阶段名称，当前方案与链接统一使用本目录名称。

| 文档                                                                                     | 职责                                                                                              |
| ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| [W1 原生 Loader 方案](../implemented/w1-native-limits-loader.md)                         | 接口复用、capability 边界、fork 维护与完成结果                                                    |
| [W1 Dynamic Workers / Worker Loader](../implemented/w1-dynamic-workers-worker-loader.md) | public binding、原生 JS API、namespace、动态 Worker 与产品验收合同                                |
| [W2 Workers Standard limits](../implemented/w2-standard-limits.md)                       | Standard limits、公开配置/API、可观察行为和自恢复的完成合同                                       |
| [W3 用户可扩展原生 Binding](../implemented/w3-user-extensible-native-bindings.md)        | `ocd` 名字/path 注册、Service binding `services + props`、Provider 直连与生命周期；不管理扩展版本 |
| [此前 stock workerd 可行性复核](../implemented/p10-worker-loader-feasibility.md)         | 保留旧 pin 的 No-Go 实测；不作为当前 fork 路线的禁令或完成证据                                    |

本目录保存尚未完成的 workerd 设计与 fork 维护入口。源码基线、fork 交付方式和内部实现分工以本目录为准；
W1/W2/W3 已完成合同不会因允许 fork 而降低；W3 不把用户 Provider 解释为第二个 workerd 或第二套 authority。

返回[文档索引](../README.md)。
