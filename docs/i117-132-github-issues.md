# I117、I130、I132：剩余 GitHub issues 与 AI Search Gate

状态：**planned**。本批只包含仍需实施的
[GitHub issue #117](https://github.com/elliothux/open-compute/issues/117)、
[GitHub issue #130](https://github.com/elliothux/open-compute/issues/130) 与
[GitHub issue #132](https://github.com/elliothux/open-compute/issues/132)。Python issues 已归入 P20，rerank issue 已归入 P19。

## 共用 Gate：完整 AI Search 与集中 provider 协议验收

P19 实施时扩展现有 `p5-search`，不要为 embedding、rewrite、chat 和两个 rerank 协议分别建立互不相干的测试入口。验收分成两个目标，
由同一个 `p5` Gate group 统一登记、发现和报告：

1. `p5-search` 是普通 workspace Gate，使用进程内有界 HTTP fixture 和正式 pinned workerd，完整执行 upload／index → embedding →
   vector／keyword／hybrid retrieval → boosting → reranking → Search／Chat。它不读取真实 provider credential，也不访问公网；CI 和
   `./test/gate.py --workspace` 必须始终可重复执行。
2. `p5-ai-provider-qualification` 是显式 live Gate，使用已经配置的 CI environment secrets 调用真实 provider，并通过生产
   `AiBackendProtocol` client 和 AI Search service 执行完整链路。它不是独立 curl probe，不允许为通过 qualification 写第二套 JSON codec、
   response parser 或排序逻辑。

### 协议与端到端矩阵

`p5-search` 的本地 fixture 冻结并覆盖以下全部协议；每个 fixture 都校验 method、path、Content-Type、Authorization、固定 request body、
response size limit 和稳定错误分类：

| 能力 | protocol | 普通 Gate fixture | live Gate provider |
| --- | --- | --- | --- |
| 文档与 query embedding | `openai_embeddings_v1` | OpenAI-compatible `/v1/embeddings` | 阿里云百炼 |
| query rewrite、Chat、SSE | `openai_chat_completions_v1` | OpenAI-compatible `/v1/chat/completions` | DeepSeek |
| 专用 rerank | `cohere_rerank_v2` | Cohere `POST /v2/rerank` exact fixture | Cohere |
| 通用专用 rerank | `rerank_v1` | 通用 `POST /v1/rerank` exact fixture | 阿里云百炼 |

本地产品矩阵至少包含两个完整 rerank 流：同一批固定文档分别经 `cohere_rerank_v2` 和 `rerank_v1` 运行，验证 provider index
归一化、确定性 tie-break、`reranking.match_threshold`、`scoring_details.reranking_score`、最终 top-level `score`、namespace merge 及
Chat context 顺序。另以 reranking disabled 路径证明不会调用任一 rerank provider。vector、keyword 和 hybrid 三种 retrieval、query
rewrite、non-stream Chat、SSE、restart 后同一 alias 解析，以及 P19 规定的 malformed／partial／越界 score、401/403、429、4xx、5xx、
timeout 和超限 body 都留在本地 fixture Gate，不能依赖真实供应商偶然返回错误来覆盖。

live Gate 使用最小固定 corpus，把协议探测合并为三个端到端 case：

- 百炼 embedding + DeepSeek rewrite／Chat，不启用 rerank；
- 百炼 embedding + Cohere `cohere_rerank_v2` + DeepSeek Chat；
- 百炼 embedding + 百炼 `rerank_v1` + DeepSeek Chat。

每个 case 必须证明索引、文本 Search、reranking score 和最终 Chat 都来自同一生产 AI Search workflow；仅验证 HTTP 200、直接调用
provider client 或绕过 SQLite／Vectorize／workerd 均不算通过。输入保持为少量非敏感固定文本，请求次数和 token 数有显式上限；不做
自动重试，429、timeout 或 provider contract drift 直接失败并保留 sanitized evidence。

### CI environment 与调度

CI 只向 `p5-ai-provider-qualification` allowlist 下列现有变量；不得把它们设置为 workflow 全局环境，也不得传给普通 workspace target：

```text
BAILIAN_API_HOST
BAILIAN_API_KEY
DEEPSEEK_API_KEY
COHERE_API_KEY
```

model、protocol、endpoint suffix 和预期 response contract 固定在受 review 的 qualification manifest 中，不再增加 model env 或从 provider
动态 discovery。`BAILIAN_API_HOST` 只作为百炼 base authority，production config 按 manifest 组装 embeddings 与 rerank endpoint；日志、
错误、Gate report 和 retained failure evidence 都不能包含 credential、Authorization、query、document、response body 或 host 中可能携带的
workspace identity。

GitHub Actions 使用受保护的 `ai-search-qualification` environment，在 trusted `main` push、release qualification 或手动
`workflow_dispatch` 上执行；fork PR 和不受信任的 PR 不接收 secrets，只执行本地 `p5-search`。job 先确认四个变量均为非空，再由 Gate
runner 的 per-target environment allowlist 传入 test process；缺失变量是明确的 configuration failure，不能 skip、降级为 fixture 或只跑
部分协议。`setup-open-compute` 继续提供 verified `OPEN_COMPUTE_TEST_WORKERD`，不复制 workerd pin 或下载 runtime。

CI 的 AI Search job 顺序固定为：`bun run build`、`cargo fetch --locked`、本地 `./test/gate.py p5-search --jobs 1`，随后在允许 secrets 的
trusted job 执行 `./test/gate.py p5-ai-provider-qualification --jobs 1`。最终 CI aggregator 同时要求适用事件上的两个 job 成功；live
qualification 不暗中加入 `--workspace`，从而不破坏无网络、本地和 fork PR 验收，但它仍由同一 Gate case registry、exact discovery、
单轮执行、超时、清理和报告机制管理。

### 完成条件

- `test/gate_cases.py` 对本地完整 AI Search case 和三条 live flow 各登记一次，并确保四种外部协议都有真实覆盖；新增、遗漏、重复或
  ignored case 在调用 provider 前失败；
- `p5-search` 在无任何 AI provider env 时通过完整本地协议、产品、restart 和失败矩阵；
- `p5-ai-provider-qualification` 使用上述四个变量，通过三条真实端到端 flow，并在 sanitized report 中记录 provider、protocol、固定
  model/revision、HTTP status 类别、耗时和 response schema digest，不记录 payload；
- CI workflow 不在 fork PR 暴露 secrets，不使用 `pull_request_target` 执行 PR checkout，不把 credential 写入 artifact、cache、命令行或
  `$GITHUB_OUTPUT`；
- P19 文档、`docs/references/testing.md`、CI workflow 与 Gate group 同步为这一边界：普通 workspace Gate 使用本地 exact fixture，受保护
  live Gate 集中验证真实 provider protocol 与完整 AI Search flow。

## I117：开放普通主机 IP 网络并加固内部端点

### 结论

#117 提出的同机 PostgreSQL、私网 Redis 和内部 HTTP 可达性属于单机自托管 common path，但不实施其
`[outbound].allow/deny` 配置。Cloudflare 托管平台需要在多租户网络中封禁 private、loopback、link-local、metadata 和平台地址；
open-compute 的 Worker 与普通本机应用共享一台由 operator 管理的主机，再维护一套实例级 CIDR 防火墙只会重复操作系统、容器或 VM
的网络职责，而且仍不能提供端口级或进程级隔离。

当前模型直接改为一个 workerd 原生 `Network(allow = ["network"])` 通用 IP capability。它统一承载应用 Worker 的 `fetch()`、
HTTP(S)／WebSocket、`cloudflare:sockets.connect()`、`node:net`、`node:tls` 和 Python 对应路径；不开放 `unix` 或
`unix-abstract`。operator 负责通过宿主 firewall、network namespace、容器或 VM 限制公网、LAN、loopback 和云 metadata。
open-compute 不把该能力描述为 per-Worker／per-instance 网络 sandbox。

保留 capability delegation，而不是地址策略：validation execution、无 outbound 的 extension facade 及其它
`globalOutbound = null` 路径继续没有通用网络；声明的 Service／DO capability tunnel 也不变成第二条通用出网路径。Python P20
复用同一 capability，不增加 Python 专用代理或规则实现。

### 修复方案

1. 把 runtime config 中的 `internet` service 改为表达真实语义的 `outbound-network`，使用
   `allow = ["network"]`、空 `deny` 和既有 browser CA trust；同步把私有 loader binding `PUBLIC_NETWORK` 改为
   `OUTBOUND_NETWORK`。不新增 `OutboundConfig`、CIDR parser、policy persistence、兼容分支或热加载。
2. 所有应用执行入口继续只经 `tenantGlobalOutbound()` 获得该 capability；validation、probe 和明确无 outbound 的路径返回
   `null`。系统 Worker 不把 capability、socket 或内部 Fetcher 暴露给 tenant。
3. 网络可达性不再承担平台鉴权。binding-backend 与 observability 保留现有 generation token 前置验证；runtime-source 把
   `startupGeneration` 从 JSON body 移到现有 `x-open-compute-startup-generation` header，在读取和解析 body 前完成 constant-time
   token／generation 验证。所有无效或旧 generation 返回同一 404，不泄露 listener、token 或内部状态。
4. 更新当前 architecture contract、Cloudflare compatibility/deviation、英文和中文 TCP 文档及 Python 方案。历史
   `docs/implemented/` 与 release notes 保留当时 public-only 证据，不回写为当前事实。

### 安全边界

- 应用漏洞或 SSRF 可以到达 operator 未在宿主网络层阻断的 localhost、LAN 和 metadata；这是明确的 self-host 信任模型，不伪装成
  Cloudflare 托管隔离。
- Unix domain socket 继续不可达，Worker 不能直接连接 Docker socket、数据库 Unix socket 或平台文件系统端点。
- 平台管理面和内部 listener 必须依赖 role credential、generation token 或 capability authorization，不能信任来源地址或内部 header
  名称。
- 不增加 hostname pre-resolution 检查、JavaScript proxy、第二套 TCP stack、端口 ACL、per-Worker profile 或自动 firewall 规则。

### 验收

- 正常部署的 JavaScript Worker 通过 `fetch()`、WebSocket、`cloudflare:sockets.connect()`、`node:net` 和 `node:tls`
  访问受控 public、private、IPv4/IPv6 loopback 目标；DNS 解析和 redirect 仍由同一原生 Network capability 处理；
- 标准 PostgreSQL driver 连接同机数据库并完成 parameterized query、commit、rollback 和 connection close；Python 路径在 P20
  主链资格化时复用相同 fixture；
- Unix socket 连接失败，`globalOutbound = null` 的 validation／facade 路径仍失败，声明的 Service／DO capability 行为不变；
- tenant 对 runtime-source、binding-backend、observability 和管理面发送缺失、伪造或旧 generation 凭据均不能读取 secret 或调用内部功能；
- 不同实例重启及 workerd 自恢复后继续获得同一 host-network 模型，deployment metadata 不能增加未委托 capability；
- 更新 focused runtime/config tests、现有 egress Gate 和 PostgreSQL integration 后，再按仓库规则完成静态检查、coverage 与源码冻结后的
  单轮 workspace Gate。

## I130：修复 R2 S3 启动 preflight multipart 下限

### 结论

该问题属实且需要修复。`crates/artifacts/src/r2_preflight.rs` 的 multipart canary 当前上传两个 6-byte part；S3 multipart
允许先接收小 part，但在 complete 时要求每个非最终 part 至少 5 MiB。严格执行该规则的 RustFS 因而拒绝 part 1，导致使用该 S3
backend 的 instance 在 readiness 之前以 `R2_PROVIDER_UNAVAILABLE` 停止。

仓库已经用 `R2_MIN_MULTIPART_PART_BYTES = 5 * 1024 * 1024` 表达同一产品下限，但 preflight 没有复用它；现有 `MockS3`
又未在 complete 时检查非最终 part，因而测试没有暴露该错误。

R2 S3 endpoint 是 open-compute 已声明的外部数据面，不属于 Cloudflare tenant Worker Runtime API 兼容审查面；这不降低该缺陷的优先级：
它会阻断受支持的 S3-backed instance 启动，属于 common path 可用性问题。

### 修复方案

1. `verify_multipart()` 的第一个 part 直接复用 `R2_MIN_MULTIPART_PART_BYTES`，使用确定性 5 MiB 内容；最终 part 保持小型 sentinel。
   不新增第二个 multipart size 常量或 provider 特判。
2. 同步更新 complete size、assembled body 与摘要断言。继续验证每个 part ETag、multipart ETag、SSE-C 读回和精确字节内容，不能把
   canary 降级为“complete 返回成功”。
3. 保持当前失败清理：complete 或后续校验失败时 abort multipart，顶层 preflight 继续删除 canary objects；不修改 bucket、权限、
   encryption 或 readiness 的 fail-closed 语义。
4. 扩展现有 HTTP `MockS3` 的 test-only strict multipart 模式，在 complete 时拒绝小于 5 MiB 的非最终 part；preflight 回归必须通过该
   模式。该 fixture 同时保留一个 undersized non-final part 的拒绝断言，避免以后再次由 permissive mock 掩盖 provider 规则。

不为本修复引入 provider allowlist、RustFS 分支、兼容 fallback 或新的 preflight framework。

### 验收

- strict fixture 先证明两个 6-byte part 会以 `EntityTooSmall` 等价错误失败，再证明修复后的 canary 成功并清理所有临时 upload/object；
- 第一个 part 的记录长度等于 `R2_MIN_MULTIPART_PART_BYTES`，最终 part 可小于该值，完成对象长度、ETag 和读回内容完全一致；
- 既有 conditional put、metadata、range、pagination、multi-delete、SSE-C 和失败清理回归保持通过；
- 实现完成后运行 artifacts focused tests、Rust 静态检查、coverage，以及源码冻结后的单轮 workspace Gate。

## I132：恢复 Cloudflare 上游刷新并完成当前候选升级

本项沿用[Cloudflare 上游刷新](references/cloudflare-upstream-refresh.md)的既有 three-way contract，不建立第二套 pin 或更新流程。

### 结论

该 issue 属实且需要处理，包含两个相连但不同的缺口：

1. `.github/workflows/cloudflare-upstream-review.yml` 只把 `summary` 写入 `$GITHUB_OUTPUT`，`classification` 被外层
   `>> /dev/null` 丢弃。因此 tracking issue 显示 `Classification: ****`，job output 也为空；即使 scanner 将来返回 `ready`，
   `draft-pr` job 也不会启动。
2. scanner 对任何 Wrangler version 变化都无条件返回 `blocked`，并等待“this repository's wrangler evidence implementation”。这符合
   fail-closed 原则，但说明 P16 约定的 Wrangler candidate inspection 尚未完成，自动刷新永远无法越过 Wrangler release。

2026-09-29 从当前正式 baseline 重新运行 scanner 的结果为：OpenAPI `780de88d0324…`、official SDK `7.2.0`、Wrangler
`4.143.0`，146 个 selected operations 中 38 个变化、0 个删除、0 个 SDK unmapped；当前唯一报告的 blocker 是仓库缺少 Wrangler
candidate evidence。以上只是一份发现快照，实施时必须重新解析 stable/HEAD 并冻结当次 exact identities，不能把这些版本写成新的长期
authority。

该工作属于 Cloudflare management API、official SDK 与 Wrangler toolchain 的协调更新，不属于 tenant Worker Runtime
`cf-compatibility-check` 范围；资格依据是本仓库的 upstream-refresh contract、固定 OpenAPI/SDK/Wrangler 输入和真实 CLI/product Gate。

### 修复方案

#### 1. 修正 workflow output

用一个 shell redirection block 将 `classification=<value>` 和 multiline `summary` 一起追加到 `$GITHUB_OUTPUT`，保留现有
`test -n "$classification"`。不增加 wrapper action 或新的 workflow helper。

手动 dispatch 的验收必须确认：tracking issue 中的 classification 非空；`unchanged` 不写 issue；`blocked`/`breaking` 不创建 PR；只有
`ready` 才进入 `draft-pr`。

#### 2. 补齐 Wrangler candidate evidence

扩展现有 `test/upstream-review/scanner.ts`，复用同一 verified npm tarball 下载/解包路径，对候选 Wrangler 记录 package、
`config-schema.json` 和 CLI 的 immutable digests，并生成 selected config/binding/command surface 的静态差异。classification 规则收敛为：

- selected OpenAPI operation 删除、official SDK closure 丢失或 Wrangler 已用命令/字段出现不兼容删除时为 `breaking`；
- 上游身份可验证，但存在 scanner 无法解释的 selected contract 差异时为 `blocked`，并列出精确字段、命令或 evidence owner；
- OpenAPI/SDK closure 完整，Wrangler selected surface 可机械投影且没有未解释差异时为 `ready`，由 Draft PR 继续完整资格；
- 不再仅因 `candidateWrangler.version != baseline.wrangler.version` 永久阻塞。

在现有 `scanner.test.mjs` 增加最小 fixture：Wrangler identity 改变但 selected surface 等价时为 `ready`；selected command/config 删除时为
`breaking`；未知 drift 时为 `blocked`。scanner 仍不得执行下载包的 lifecycle script 或 candidate CLI。

#### 3. 复用 frozen candidate 更新路径

继续由 `test/upstream-review/apply-candidate.ts` 消费 scanner report 中的 exact revision、integrity 和 digest；不新增人工更新器。Draft PR
一次性更新当前 authority 与消费者：

- root catalog、`bun.lock`、OpenAPI upstream lock 和 `packages/runtime/workerd.lock.json` 的 Workers SDK identity；
- subset、capability/deviation projection、generated SDK/surface、Wrangler config/CLI evidence 与 conformance catalog；
- production 中认证的 Wrangler version、launcher/capability response、real-process fixtures，以及受影响的英文/中文当前文档；
- candidate 的 38 个或重新扫描后实际 changed operations 对应的 wire model、handler 与 tests。

只更新当前 authority；release notes 与 `docs/implemented/` 中的历史 4.138.0 证据保持原样。任何新增 Wrangler prerequisite 都在现有
Cloudflare v4 handler/product owner 中直接实现，不保留双 pin、版本分支或旧 request fallback。

#### 4. 资格与完成条件

Draft PR 先通过 scanner fixtures、candidate digest、P6 contract regeneration、OpenAPI/SDK 双向 closure 和 Wrangler request trace；再运行
受影响的 focused product tests 与真实 Wrangler/`ocd` Gate。若 CLI trace 变化，先更新实现与 sanitized fixture，再继续资格，不能把未知
请求加入 allowlist。

随后按仓库约定完成 build、format、Clippy、no-default-features、MSRV、metadata、dependency boundaries、coverage，并在源码冻结后只运行
一次 workspace Gate。正式 pin 只在全部通过后移动；失败时保留现有 baseline 和 artifact，并把精确 blocker 写回同一个 tracking issue。

### 验收

- workflow 的 step output、tracking issue classification 与 `draft-pr` 条件一致，不再出现空 classification；
- scanner 能在不执行 candidate package code 的前提下区分 Wrangler 的 compatible、blocked 和 breaking drift；
- Draft PR 完全由 frozen report 生成，CI 不重新解析 npm `latest` 或 schema HEAD；
- 当前 OpenAPI、official SDK、Wrangler、generated SDK、runtime lock、capability response 和 CLI traces 指向同一已资格组合；
- 合并后 tracking issue 关闭，本文精简为实际结果并以原编号移入 `docs/implemented/`。

只有实际成功退出的检查才写入完成记录。
