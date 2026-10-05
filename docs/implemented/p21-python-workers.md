# P21：Cloudflare Python Workers

状态：已完成（2026-10-05）。普通 Python Worker 使用 workerd 官方 Python entrypoint、内置 Pyodide 和未修改的 `workers-runtime-sdk`。Python 与 JavaScript 共用 binding materializer、资源权限和持久化 authority。

## 用户入口与构建边界

维护配置为 `cloudflare.config.ts`，认证、上传、部署和资源管理使用 cf。当前 cf 缺少可用的 Python package builder，Python 开发构建临时由 `scripts/build-python.ts` 调用用户安装的 PyWrangler。优先选择项目 `.venv/bin/pywrangler`，再查找 PATH；未安装则明确报错，不安装或校验版本。开发者显式准备 PyWrangler 所需的依赖与构建解释器，cf 消费生成的 Build Output。

临时例外只限 Python 构建，等待 cf 原生 Python builder 后删除桥接代码的 TODO 留在活动后续方案和用户文档。正式 `ocd` 启动与请求不运行构建工具、不联网下载 runtime 或安装 package。

## 唯一运行链路

上传 admission 验证不可变模块、compatibility metadata 和 binding descriptor。部署前 coordinator 启动独立受监督的 preparation child；它复用现有进程组、就绪、停止、reap 和 orphan recovery 的 ownership，使用独立 lease，不开 DO storage。生成 snapshot 后先确认 child 退出，再加密并提交 prepared artifact。

`control.sqlite` 保存 Version、artifact identity 与资源 authority；对象存储保存加密不可变产物。加载时验证 Version/module/runtime identity、内容 digest 与绑定部署身份的 AEAD。promotion、rollback 和 restart 选择既有不可变内容；损坏、身份不匹配或过期 generation 失败关闭，不在读取时修复或重新构建。secret rotation 与 cancellation/deadline fencing 沿用平台 authority。

KV、R2、Queue、DO 和 Service 使用原生 workerd public objects；D1 使用上游 binding，其他声明产品使用现有 wrapped binding。窄协议 adapter 接入既有 SQLite、对象存储与资源后端，PRIVATE_POLICY、内部 transport/token 不进入租户 `env`。旧 public facade 和 Queue 旧序列化路径已删除。当前官方 compatibility date/flag 的行为直接由正式 runtime 合同决定。

## 当前输入

- 正式 workerd release：`v1.20260930.0-open-compute-r4.e98a3e843`；source `e98a3e8433979356047a202d3e0d1b0e2e2c4b8c`，upstream base `d99bc6b777e35d72d71c2f1fe2fd1db53284528a`。
- Pyodide：`314.0.6_2026-08-17_6`；SDK：未修改的 `workers-runtime-sdk` 1.9.2。
- 开发配置与上传 CLI：cf `1.0.0-beta.12`；Vite plugin `2.0.0-beta.sha-52b0dc0e9`。固定租户类型仍为 `@cloudflare/workers-types@5.20260830.1`。
- 唯一 formal pin 为 [`workerd.lock.json`](../../packages/runtime/workerd.lock.json)。逐目标 archive/binary digest、source/build inputs、Python bundle/license 输入由 lock、release catalog 与构建校验拥有。

四平台 workerd release 复用 [run 37175389986](https://github.com/elliothux/workerd/actions/runs/37175389986) 的已有产物，publish 修正没有重跑 native build。workflow-only commit `fdcb8c97f0b9f75f610b66e418d5175b9188ddad` 不冒充 binary source。10 项 release asset 已校验，workerd 的其他 CI 已停用，保留手动 publish。当前主机 single-binary Gate 证明内嵌输入和离线首次启动；此次没有额外发布 open-compute 平台安装包，也不据此宣称其他主机的 daemon 验收。

## 已接受的限制

公开上传仍拒绝 cross-Script DO 和 Workflow binding。Python/JavaScript 的资格测试分别调用各自 namespace/definition，不把它们写成相互绑定同一个对象。Workflow definition 的 current Version 独立于 HTTP deployment；rollback 后通过公开 `PUT /workflows/{name}` 重绑 definition，旧实例继续使用其固定 Version。Queue 的 force 只授权 backlog purge，不绕过活跃引用保护。

Cache API 保持 Worker 级共享可变状态；promote 和 rollback 不回退缓存内容。默认与命名空间、不同 Worker 继续隔离，自动 Workers Cache 的默认版本隔离独立验证。

Assets 使用现有 private wrapped binding，公开名称为 `Fetcher`，让官方 SDK 选择其既有 fetch 包装器并转换 Python 请求头。Workflow 内部 suspension/rollback 信号使用已有脱敏 Error 类型，跨 Python FFI 兑现 rejected promise，仍用私有 identity 和 controller verdict 决定中断。两处修复均复用现有运行链路。

自动缓存与原生 Cache API 共用 workerd 的 Response 序列化和同一个持久化后端。Cache HTTP adapter 在元数据入口删除 hop-by-hop 和 Connection 指定的字段，保留原样 encoded body、作用域和 purge/refresh fence；storage 继续严格拒绝传输头，不接受特殊修复或测试分支。

原生 R2 延续 upstream 的已知长度流上传合同：定长分块流可写入，未知长度流抛出 `TypeError` 且不产生对象。D1 binding 延续上游 WrappedBinding 的序列化能力，父 Worker 的持久数据可由显式接收该 binding 的子 Worker 读取，子 `env` 仍仅包含声明的键；原生 KV/R2/Queue 的直接 transfer 拒绝保持。测试使用当前原生 public objects，未恢复已删除的 facade。

Flask 带 request/app context 的流式响应受 [cloudflare/workers-py#287](https://github.com/cloudflare/workers-py/issues/287) 影响。用户于 2026-10-04 明确要求记录并暂缓。SDK 保持原版，普通 Flask HTTP、template、HEAD/status/query、secret、restart 和 rollback 仍需通过。该流式路由不计为通过；Django 和 FastAPI 的流式验收保留。原始失败与诊断证据保留，待上游修复后重验 body、并发上下文和 cancellation cleanup。

Dynamic Python 的 fresh isolate 仍受 `OC-WKR-LIMIT-001` 的一秒 startup 限制，未声明支持；普通部署 prepared artifact 的通过不能推出 Dynamic Python 通过。完整支持面由 [兼容矩阵](../references/cloudflare-compatibility.md)、[偏差](../references/p1-deviations.md)、机器可读合同与用户文档拥有。

## 验收证据

2026-10-05 的完整 workspace coverage（`20261005T063253-76fd44a7`）执行 61 个进程、1771 个 case，零 ignored，全部通过；行覆盖率为 149835/165886（90.3241%）。精确 66 个对象、139 份 profiles 与报告保留在 `.temp/coverage/run-iDjTkgN5/`，未合并历史输入或改变阈值。174 条 LLVM mapping 警告经逐条诊断均为 hash=0 的依赖/标准库记录，没有项目符号。

修正测试 HTTP 服务的 TCP half-close 配置并完成五种 Clippy 配置、格式、source policy、baseline identity 和 CF 复核后，冻结源码运行最终普通 workspace Gate（`20261005T092248-6ab16d3d`）：61/61 进程、1771/1771 case、零 ignored、单轮全部通过。环境为 Rust 1.98，`RUSTFLAGS=-D warnings -C debuginfo=0`、`CARGO_INCREMENTAL=0`；保留默认调试断言、优化和全部案例，没有 profiling instrumentation。

覆盖率源码身份为 `f2be50333229f6bc84baef88d608907102b423737fccc0a64ddf3a14a269a97d`，最终 conformance 源码身份为 `2d208bffed6f587c1dfd8bfa0f3bbd2b97fe9b66e795e774910c4e46a0f0db78`；最终 Gate 的输入摘要为 `de24f357e34186a1183ec8fc9bfad135137779f5e61badf9a42ecbe4aa35a1d5`。覆盖率后仅更新文档、baseline 和两个 Rust 测试 fixture；生产代码未变化。新 TCP fixture 经 focused 和最终完整 Gate 验证，未把原覆盖率写成新 fixture 的覆盖率。

机器可读平台合同仍为 `incomplete`：12 项通过、9 项包含未运行的 hosted differential、1 项明确不支持、0 failed、0 blocked。此次完成当前声明范围的普通 Python 验收，不把本地通过写成完整 hosted Cloudflare 资格。

P21 注册七个 target、九个 case，覆盖普通 Main、三种 framework、Services、Queues、DO、Workflow 和 runtime/FFI/stdlib/outbound/observability，并保留已有产品权限、失败与恢复 Gate。编译、source review 或一次性 native probe 都不冒充普通 daemon 执行。

新增的 Python preparation、加密与 artifact authority 六个 Rust 模块在本轮完整覆盖率中分别达到 91.92%–97.53% 行覆盖率；coordinator 为 331/344（96.22%），native preparation owner 为 95/98（96.94%）。这些数字来自同一轮精确对象与 profiles，不合并历史输入。

CF 复核对照固定的官方 types、cf producer、SDK 与 formal workerd source，逐项记录 source、owning checks 和普通 Gate 的证据边界。此轮不声称完成 hosted Cloudflare differential、全球调度或全部上游 overload 资格。原始失败、修复验证与新 run 输入保留于 `.temp/p21-final-preflight/`、`.temp/gate-run/`、`.temp/coverage/` 和 `.temp/p21-workerd-release-r4/`。

## #129 Dynamic 基线与 #128 HTTP 子项

2026-10-05 的 scoped Gate `20261005T123536-25820dff` 通过：`./test/gate.py p21-python-runtime`，native inventory 完全匹配，1 个真实进程用例、1 pass、零 ignored、单轮，567.04 秒。复用上文 R4 的正式 darwin-arm64 binary，SHA-256 `5fcc34038f37c5a42668193f31bc1a9eeb4355f42f8188b312d03c77f3efb788`；未重建 workerd、变更正式 pin 或修改 SDK。运行环境仍为 Rust 1.98、`RUSTFLAGS=-D warnings -C debuginfo=0`、`CARGO_INCREMENTAL=0`。

[`dynamic.rs`](../../crates/service/tests/p21_python_runtime/dynamic.rs) 在普通部署的真实 daemon 中调用公开 `loadWorker`/`getWorker`，使用固定 `child.py`、`2026-09-08`、`globalOutbound = null`，不注入 prepared artifact。该原生 raw Dynamic 路径按 matching workerd 自动选择内置 SDK；它与普通部署的外部 SDK 1.9.2 是不同输入。每阶段分别新建两次、同键调用两次：

| 阶段                                    | 单次 wall 时间 | 实际结果                          |
| --------------------------------------- | -------------- | --------------------------------- |
| fresh-process-before-python-preparation | 1017–1053 ms   | 4/4 startup CPU limit；factory +2 |
| after-ordinary-python-preparation       | 1013–1016 ms   | 4/4 startup CPU limit；factory +2 |
| fresh-process-after-restart             | 1016–1023 ms   | 4/4 startup CPU limit；factory +2 |

12 次均返回 `Worker exceeded CPU time limit.`，一秒 startup CPU 限额保持不变。失败的初始化没有形成可成功调用的 warmed child；同键两次均重新运行 factory。普通应用的预初始化与运行也没有令 raw Dynamic 初始化通过。三阶段的父 Worker 健康调用均成功，restart 后其不可变 Version 保持不变。wall 时间不是独立 CPU 测量；这份负基线不声明 Dynamic Python 可用于生产，后续实现留在 [#126](https://github.com/elliothux/open-compute/issues/126)。中英文指南已纠正 Cloudflare GA 日期、区分本地资格与上游状态、解释 interpreter/snapshot/TCP 边界，W1/W2 标识其历史 pin，不再作为当前 Dynamic 支持声明。

[`http_clients.rs`](../../crates/service/tests/p21_python_runtime/http_clients.rs) 与应用 `http_client_cases.py` 使用未修改的 requests 2.33.1、httpx 0.28.1（sync/async）和其原版依赖，实际 Python 为 3.14.2。三组完整断言分别运行于首次部署、fresh restart、rollback 后：Unicode 正文/响应头、422 错误、完整 stream 读取、真实超时、closed-port refusal、失败后恢复和 AsyncClient task cancellation。服务器 admission 与迟到响应计数证明 deadline 在响应完成前触发；取消后有界 drain 到零 pending，并在最终停止时证明监听端口可重绑。流迭代验证完整正文，不据此宣称网络分块一定即时交付或 task cancellation 一定立即 abort Fetch。

requests/urllib3 的真实超时链是 `ConnectionError → MaxRetryError → TimeoutError → _TimeoutError → JsException`，不能只捕获 `requests.Timeout`。httpx sync/async 的该 fixture 超时均为 `ConnectTimeout`。三种客户端各自在限额为 2 的独立 Version 内完成两次真实 HTTP 请求，第三次被原生 `Too many subrequests` 拒绝；服务端恰好收到两次，下一次独立 invocation 仍能成功。上传会话分别创建，不复用已消费的 token。现有 Vectorize 等待断言的有界窗口覆盖 30 秒 durable claim 及 SQLite busy windows，不放宽产品限制。

输入由用户已安装的 PyWrangler 构建，再由 cf `workers versions create --prebuilt` 捕获：

```sh
node scripts/build-python.ts /absolute/python-project --assets-directory public
node test/conformance/applications/prepare-python-main.ts /absolute/python-project /absolute/capture-dir runtime
OPEN_COMPUTE_TEST_WORKERD=/absolute/verified/workerd ./test/gate.py p21-python-runtime
```

本次复用既有构建解释器，设置 `UV_PYTHON_DOWNLOADS=never`、`npm_config_offline=true`；Cargo 的 workerd archive 与 Caddy 输入来自既有 `runtime-env.sh` 的正式锁校验结果。维护 fixture 的 194 个模块及 upload SHA-256 `515bb1afe95d4672ddc69e8050813511f55db8356af516a8a68e3c4634d77a4d` 由 manifest 验证，19 个 SDK source hash 仍匹配原版 inventory。PyWrangler 选用 Pyodide 314.0.7 索引的 requests/httpx wheels；[pylock.toml](../../test/fixtures/python-runtime/pylock.toml) 保留精确 URL/digest，运行资格来自正式 314.0.6 bundle，未把 package 索引当作 runtime pin 升级。

五种 canonical Clippy 配置、后续 fixture 的 scoped Clippy、格式、source policy、capture 工具 strict typecheck、13 个 capture/parser 检查、baseline identity 和双语网站构建通过。此次 follow-up 只改测试、捕获输入和文档，生产代码未变化；上文完整 workspace/coverage 是此前生产实现的验收，这次新增断言的证据为本 scoped run。完整输出、负基线、超时异常链、限额结果与此前失败保存在 `.temp/python-issues-128-129/` 和 `.temp/gate-run/`；没有重试未修复的失败或删除证据。

[#129](https://github.com/elliothux/open-compute/issues/129) 的基线与文档范围已完成。[#128](https://github.com/elliothux/open-compute/issues/128) 仅补齐 requests/httpx 子项；AI/API client、HTTP MCP、PostgreSQL/MySQL drivers、TLS 及其剩余资格后续再补。Flask context streaming #287 保持已接受的上游限制，不修改 SDK 或把该路线计为通过。
