# P19：AI Search Query API 完整兼容与专用 Rerank provider

状态：**planned**。本方案解决
[#131](https://github.com/elliothux/open-compute/issues/131)，把 AI Search reranking 从 Chat Completions 提示词调用改为专用
reranker provider，并首批实现两个闭集协议：`cohere_rerank_v2` 和 `rerank_v1`。同一阶段关闭当前
AI Search text query 路径上已确认的 Cloudflare API shape／score／threshold／boosting 缺口；不能只替换 provider 后继续宣称完整兼容。

## 1. 用户结果与结论

- operator 可以把 AI Search 的 reranking alias 映射到 Cohere v2 或通用 `/v1/rerank`
  endpoint；tenant 继续只选择公开 model alias，不能提供 endpoint、credential、header 或 wire protocol。
- AI Search 先完成 vector／keyword／hybrid retrieval，再把有界候选集一次发送给专用 cross-encoder；provider 返回原始候选
  `index` 与 `relevance_score`，open-compute 据此排序、过滤并返回 Cloudflare-compatible scoring fields。
- `scoring_details.reranking_score` 返回专用 reranker 的 `[0,1]` score；顶层 `score`、namespace merge 与 boosting 后 score
  的映射必须先由固定 Cloudflare differential 冻结，不能预设顶层值必然等于 `reranking_score`。
- `reranking.match_threshold` 只过滤 reranking score；普通 retrieval `match_threshold` 按 Cloudflare 文档过滤 vector
  similarity，不能再过滤 fusion score。keyword-only 与 hybrid keyword-only candidate 的精确边界由 hosted differential 冻结。
- 实现当前官方 relevance boosting 顺序：bounded retrieval candidate pool → vector threshold → fusion → `boost_by` → reranking →
  reranking threshold → final result limit；候选数量与顶层 score 的可观察细节由固定 fixture 约束。
- Search response 必须返回官方必需的 `query_kind`。当前只支持 text query，因此成功结果固定返回 `"text"`；image／file typed
  message parts 与 multimodal query 继续 fail closed，并在 repository docs、英文网站和中文网站显著说明“不支持多模态 AI Search”。
- Chat Completions 只负责 chat 与 query rewrite。删除 chat model 的 `rerank` capability、rerank prompt、JSON permutation parser
  和相关 fixture，不保留 fallback 或旧配置双读。
- provider 不可用、返回不完整 index、非有限／越界 score、超限 body 或 malformed JSON 时整次 search fail closed；不能静默返回
  rerank 前顺序，也不能把上游响应正文暴露给 tenant 或日志。

这里的“通用”只指 open-compute 内部统一的 rerank contract。外部没有 OpenAI Rerank 标准，两个协议 token 各自固定准确的 wire
shape；不提供任意 JSON template、脚本 adapter 或按 provider 名称猜测协议。

## 2. 当前问题

当前 [`AiBackendProtocol`](../crates/core/src/config/ai/backend.rs) 只有 `openai_embeddings_v1` 与
`openai_chat_completions_v1`。[`AiGenerationCapability::Rerank`](../crates/core/src/config/ai.rs) 强制 reranking model 引用 Chat
Completions backend；[`OpenAiChatClient::rerank`](../crates/service/src/ai_provider.rs) 把 query 和 candidates 写进 prompt，并要求模型
返回完整 index permutation。

该路径存在以下实际缺口：

1. 专用 cross-encoder endpoint 无法配置；更换 endpoint 或 model 名不能改变 chat request/response shape。
2. LLM 只返回顺序，没有 provider relevance score；当前 `reranking.match_threshold` 错误地使用 rerank 前的 fusion score。
3. 普通 `retrieval.match_threshold` 也由通用 fusion helper 错误地过滤 fusion score；Cloudflare 当前合同明确要求 vector similarity。
4. 返回 chunk 缺少 `scoring_details.reranking_score`，Search response 还缺少必需的 `query_kind`，与当前 Cloudflare schema 不一致。
5. 当前 runtime 明确拒绝官方 `retrieval.boost_by`，因此缺少 rerank 前的 relevance boosting 阶段。
6. prompt 输出具有非确定格式、token 开销和较高延迟；完整 permutation 校验只能发现错误，不能获得可比较的最终分数。
7. `generation_models` 同时表达文本生成与 cross-encoder，导致 backend protocol、模型能力和配置 ownership 错层。

只在现有 chat prompt 上增加 JSON mode 或重试不会解决协议与 score 缺口，因此不作为方案。

## 3. Compatibility authority

实施时固定并转成请求／响应 fixture 的上游 authority：

- [Cohere Rerank API v2](https://docs.cohere.com/reference/rerank)：`POST /v2/rerank`、`query`、`documents`、
  `top_n` 与 `results[].relevance_score`；
- 通用 `/v1/rerank` 核心子集，以
  [Voyage Reranker API](https://docs.voyageai.com/reference/reranker-api)、
  [Jina Reranker API](https://jina.ai/en-US/reranker/)及实际接入 gateway 的固定 trace 交叉验证；品牌不是配置或实现
  authority，只有本节冻结的公共子集属于 `rerank_v1`；
- [Cloudflare AI Search reranking](https://developers.cloudflare.com/ai-search/configuration/retrieval/reranking/)、
  [result controls](https://developers.cloudflare.com/ai-search/configuration/retrieval/result-controls/)、
  [relevance boosting](https://developers.cloudflare.com/ai-search/configuration/retrieval/boosting/)和
  [How AI Search works](https://developers.cloudflare.com/ai-search/concepts/how-ai-search-works/)：候选阶段、vector threshold、
  boosting／reranking 顺序和生成前 content retrieval；
- [Cloudflare AI Search Search API](https://developers.cloudflare.com/api/resources/ai_search/subresources/namespaces/subresources/instances/methods/search/)
  与固定 OpenAPI：request optionality、`query_kind`、顶层 `score`、`scoring_details.reranking_score`、范围和 error shape；
- [Cloudflare chunk citations](https://developers.cloudflare.com/ai-search/how-to/chunk-citations/)：顶层 `score` 是 overall relevance，
  `reranking_score` 是独立的 reranker score；网页未声明两者恒等，因此 equality 必须来自 differential，而不是实现假设；
- 当前 repository 固定的 OpenAPI、Workers types、Wrangler/SDK request shape 与真实 Cloudflare differential trace。

网页用于发现合同。进入 Gate 的 exact JSON、Content-Type、status、score range、unknown-field 行为和错误分类必须保存为本地 fixture；
不得在测试中实时请求供应商。实现时记录每份 authority 的 URL、版本／revision、获取日期和 fixture digest。

### 3.1 完整兼容边界

P19 的“完整兼容”限定为当前 text-only AI Search instance／namespace Search 与 Chat Completions query path，以及与该路径直接交互的
instance reranking／retrieval config；它不是对 Cloudflare 托管 AI Gateway、global placement、billing 或未声明 source connector 的
实现要求。该边界内不能删除、改名、缩窄、静默忽略或错误解释官方字段：

| Surface | 必须一致的公开合同 |
| --- | --- |
| Instance config | `reranking`、`reranking_model`、`retrieval_options.boost_by` 的 optionality、update semantics 与 response projection |
| Request reranking | `ai_search_options.reranking.{enabled,model,match_threshold}` |
| Request retrieval | `retrieval_type`、`fusion_method`、`keyword_match_mode`、`filters`、`context_expansion`、`metadata_only`、`return_on_failure`、`match_threshold`、`max_num_results`、`boost_by` |
| Search response | `chunks`、`search_query`、必需的 `query_kind`，以及 namespace response 的 `instance_id`／`errors` |
| Chunk scoring | 顶层 `score`、vector／keyword score 与 rank、`fusion_method`、`reranking_score` 的存在条件、范围与排序语义 |
| Chat | non-stream／SSE 均消费同一最终 chunk 顺序和 scoring fields，不另建一套 rerank 逻辑 |

当前模型合同只接受文本 query 与 string message content：

- `query_kind` 在成功的 instance／namespace Search response 中固定返回 `"text"`；Chat chunk event 和 non-stream response 使用同一
  text retrieval 结果。
- image URL、file part、纯 image query 与 text+image multimodal query 不进入 embedding、retrieval、reranking 或 generation；在 public
  validation boundary 返回固定、sanitized 的 unsupported error，不能只取其中的 text、自动 OCR 或降级为 keyword search。
- document ingestion 中的图片、扫描 PDF、OCR 和可选 VLM description 是“把文档转换成可索引文本”，不代表 query modality 支持。
- repository compatibility/deviation 文档、英文网站和中文网站必须明确写出 text-only 限制；能力矩阵不得因为公开 schema 含
  `image`／`multimodal` token 就宣称已经支持。

官方当前只文档化 `@cf/baai/bge-reranker-base` 作为 AI Search reranking model。operator catalog 必须允许用这个公开 alias 映射到一个
已配置 provider；额外 alias 是 open-compute extension，不能改变官方字段 shape，也不能把 remote model、protocol 或 endpoint 暴露给
tenant。未配置对应 provider 时明确返回 unsupported/config error，不能回退到 Chat Completions。

Cloudflare 文档之间若对 candidate count、limit 时点或顶层 score 映射存在冲突，以同一固定 OpenAPI revision 下的真实 Cloudflare
differential 为资格依据；在证据产生前保持 `unverified`，不得选择一个方便实现的解释后宣称兼容。

## 4. Ownership

| 层级 | 负责 | 不负责 |
| --- | --- | --- |
| `AiBackendConfig` | 一个 operation-specific endpoint、闭集 protocol、auth、静态 headers | model alias、候选数量、tenant 输入 |
| `AiRerankingModelConfig` | 公开 alias 到 backend、remote model、可选 provider revision 的映射 | transport、prompt、embedding profile |
| Rerank client | protocol 编码、bounded HTTP、响应归一化与严格校验 | retrieval、threshold、public response composition |
| AI Search service | 候选集、model 选择、最终排序、threshold、Cloudflare response fields | provider-specific JSON |
| Storage | 已有 AI Search public config 与 alias | provider secret、query-time score 持久化 |

Reranking 是 query-time 可编辑行为，不影响已持久化 chunk 或 embedding bytes，因此不引入 embedding profile、不触发 reindex，也不把
provider score 写入 SQLite。backend、remote model 或 provider revision 改变后，后续 query 直接使用新配置；正在执行的请求继续使用其
构造时解析出的不可变 client 值。

## 5. Operator 配置合同

### 5.1 Protocol 与 backend

`AiBackendProtocol` 增加两个闭集值：

```text
cohere_rerank_v2
rerank_v1
```

backend 继续使用 P5.2 的 operation-specific 完整 endpoint，不追加 path：

```toml
[ai]
default_reranking_model = "company/reranker"

[ai.backends.company-rerank]
protocol = "cohere_rerank_v2"
endpoint = "https://api.example.com/v2/rerank"
auth = { kind = "bearer", secret = { env = "RERANK_API_KEY" } }
headers = { "X-Title" = "open-compute" }

[ai.reranking_models."company/reranker"]
backend = "company-rerank"
remote_model = "rerank-v4.0-fast"
provider_revision = "2026-09-01"
```

`default_reranking_model` 只为创建／更新时省略 `reranking_model` 的 AI Search instance 提供 operator default；它不回退到
`default_generation_model`。未配置任何 reranking backend 仍是合法部署，但启用 reranking 或在请求中指定 reranking model 时必须
解析到合法 mapping。

认证、canonical URL、HTTPS／loopback、redirect、reserved headers 和 secret handling 完全复用现有 backend 合同。新增协议不
增加 auth kind，也不允许 tenant-controlled provider headers。

### 5.2 Reranking model mapping

新增独立的：

```rust
AiConfig::reranking_models: BTreeMap<String, AiRerankingModelConfig>
AiConfig::default_reranking_model: Option<String>
```

`AiRerankingModelConfig` 只包含：

```text
backend
remote_model       # 必填
provider_revision  # 可选，仅在上游提供真实不可变 revision 时填写
```

`AiGenerationModelConfig.capabilities` 收敛为 `chat` 与 `rewrite`；删除 `AiGenerationCapability::Rerank`。旧配置中的
`capabilities = ["rerank"]` 直接拒绝，operator 必须迁移到 `reranking_models`；不保留 alias、隐式转换或 chat fallback。

### 5.3 通用请求上限

当前 `max_embedding_request_bytes`／`max_embedding_response_bytes` 实际也被 Chat client 使用，名称与 ownership 已不准确。P19 将其
直接改为：

```toml
[ai]
max_provider_request_bytes = 2097152
max_provider_response_bytes = 16777216
```

它们约束 embeddings、chat、rewrite 与 rerank 的完整 JSON/SSE body；VLM 继续使用已有独立上限。删除旧字段，不双读。AI Search
公开 candidate pool 与 final `max_num_results` 都不超过 50，不新增 operator `max_rerank_documents` 配置；实际送入 reranker 的数量按
固定 Cloudflare behavior matrix 决定。serialized body 仍必须在发请求前检查；response 必须流式有界收集，不能先无限读取再检查。

### 5.4 Provider 可用性与 Instance 能力开关

operator catalog 只声明一个能力可以被 instance 选择，不负责启用该能力。配置
`embedding_models`／`default_embedding_model` 或 `reranking_models`／`default_reranking_model` 都不能改变既有 instance 的行为，
也不增加全局 `embedding_enabled` 或 `reranking_enabled`：

| 能力 | Instance authority | Request authority | 未启用时的行为 |
| --- | --- | --- | --- |
| Vector embedding | `index_method.vector` | `retrieval_type = vector/keyword/hybrid` 只能在已建索引范围内选择 | 不生成文档或 query embedding |
| Keyword retrieval | `index_method.keyword` | `retrieval_type = keyword/hybrid` 只能在已建索引范围内选择 | 不建立或查询 keyword index |
| Reranking | `reranking` | `ai_search_options.reranking.enabled` | 直接返回 retrieval／fusion 结果 |

Embedding 与 reranking 不使用 model 是否存在来隐式启用：

- `index_method.vector = true` 时，create/update 必须解析显式 `embedding_model` 或 `default_embedding_model`；文档索引和
  vector/hybrid query 使用同一 frozen embedding contract。`index_method.vector = false` 时，即使 operator catalog 有 embedding
  model，也不调用 embedding provider。Embedding 影响持久索引；改变 model/profile/contract 必须完整 reindex。
- `reranking = true` 时，create/update 必须解析显式 `reranking_model` 或 `default_reranking_model`；`false` 时可以保留一个已验证的
  model alias 供以后启用，但当前 query 不调用 provider。Reranking 只影响 query，可以由请求级 `enabled` 覆盖 instance default。
- 请求不能启用 instance 没有构建的 vector／keyword index；但可以在已有候选检索完成后临时启用一个 operator catalog 中存在的
  reranking model。
- 没有配置任何 reranking provider 是正常的 embedding-only 部署，不产生启动 warning。只有持久 instance 明确启用了 reranking、
  但其 alias 已无法解析时，启动检查／doctor 才报告一次 sanitized 配置问题；受影响请求继续 fail closed，不能静默关闭 reranking。

因此 provider 配置表达 availability，instance 字段表达默认 policy，request 字段表达单次 override。operator 新增 backend 不得
让现有 instance 突然增加费用、延迟或改变排序。

## 6. 两个 wire adapter

所有 adapter 接收同一个内部输入：

```rust
struct RerankRequest<'a> {
    query: &'a str,
    documents: &'a [String],
}

struct RerankResult {
    index: usize,
    relevance_score: f32,
}
```

不为单一 client 增加 trait/factory。一个 `RerankClient` 持有已解析 protocol，并在 request encode／response decode 处对闭集 enum
分派；共用 HTTP/status/auth/limit 逻辑。

### 6.1 `cohere_rerank_v2`

请求：

```json
{
  "model": "rerank-v4.0-fast",
  "query": "search query",
  "documents": ["first", "second"],
  "top_n": 2
}
```

`top_n` 固定为候选数量，要求返回完整集合。P19 不发送 `max_tokens_per_doc`、`priority` 或结构化 document 等 provider extension。

响应只消费：

```json
{
  "results": [
    { "index": 1, "relevance_score": 0.91 },
    { "index": 0, "relevance_score": 0.22 }
  ]
}
```

允许 bounded `id`、`meta`、`document` 和未知字段存在但不依赖它们。

### 6.2 `rerank_v1`

该协议明确表示 `/v1/rerank` 生态的最小公共子集，不代表 OpenAI 标准，也不绑定供应商品牌：

```json
{
  "model": "BAAI/bge-reranker-v2-m3",
  "query": "search query",
  "documents": ["first", "second"]
}
```

首版不发送 `top_n` 或 `top_k`：主流实现对字段名并不一致，省略后均应返回全部 documents；也不发送
`return_documents`、`truncation` 或 provider-specific 参数。响应采用与 Cohere 相同的
`results[{index,relevance_score}]` 核心结构。需要不同必填字段或不同 response shape 的 endpoint 不属于此协议，不能通过附加任意
JSON 配置接入。

## 7. 响应归一化与失败语义

两个 adapter decode 后统一执行：

1. result 数量必须与输入 documents 数量完全相同；
2. 每个 index 必须 `< documents.len()`，且所有 index 唯一、完整；
3. score 必须是有限浮点数且位于 `[0, 1]`；`NaN`、Infinity、负数和大于 1 均拒绝；
4. unknown fields 可忽略，但 JSON 深度、完整 response bytes、集合长度和字符串长度继续有界；
5. 本地按 `relevance_score` 降序排序；相同 score 用原候选 index 升序作为确定性 tie-break，不依赖 provider 的数组顺序；
6. 401/403、429 与合法 `Retry-After`、5xx、其他非成功 status、redirect、错误 Content-Type、timeout、超限 body 和 malformed JSON
   使用现有稳定 `AiProviderError` 分类；任何返回给 tenant 的错误保持 sanitized。

不接受 partial top-N response，因为 AI Search 需要对送入 reranker 的完整候选集应用 reranking threshold、构造 scoring details，并让
namespace merge 保持确定性。公开 query path 的候选池上限只有 50，首版没有引入分页或多批 merge。`max_num_results` 在
boosting／reranking 前后作用于哪个集合不得由 provider 的 `top_n` 偶然决定，而由下一节固定的 Cloudflare behavior matrix 决定。

## 8. AI Search pipeline 与公开结果

公开文档能够直接确定的顺序为：

```text
query rewrite（可选 Chat Completions）
  -> vector / keyword retrieval
  -> retrieval.match_threshold（只看 vector similarity）
  -> hybrid fusion
  -> relevance boosting（如果配置 boost_by）
  -> 按 Cloudflare behavior matrix 选择有界 rerank input
  -> 专用 rerank provider
  -> reranking.match_threshold
  -> final max_num_results
  -> context expansion
  -> search response / chat context
```

Cloudflare reranking 文档把送入模型的结果描述为已经受 `max_num_results` 约束；boosting 文档则描述先取最多 50 个候选，boost／rerank
后再返回 top `max_num_results`。实现前必须在同一固定官方 revision 下取得下列 differential，不把网页冲突编造成折中语义：

1. 无 `boost_by`、有 reranking 时，reranker 实际接收的 candidate count；
2. 有 `boost_by`、有 reranking 时，boost、rerank 与 `max_num_results` 的精确时点；
3. vector、hybrid 和 keyword-only 下普通 `match_threshold` 对无 vector score candidate 的行为；
4. boost 与 rerank 前后顶层 `score` 的值，以及 namespace merge 使用的 score；
5. equal score 的顺序、空结果和所有候选被两个 threshold 过滤后的 response。

代码内部必须分别保留 retrieval／fusion、boosting 与 reranking score，直到 public response projection；不能通过反复覆盖一个 `score`
字段丢失阶段事实。最终 `public_score` 只在拿到 differential 后由一个共享 projection 计算，instance Search、namespace Search、Chat
context 和 SSE chunks event 全部复用它。

候选发给 provider 前保持与 `RankedCandidate` 一一对应。归一化结果通过原始 index 找回 candidate，不用 provider 回传 document 文本。
`RankedCandidate` 增加可选 `reranking_score`；reranking 成功后：

- candidate 顺序使用 rerank score；
- `scoring_details.reranking_score` 存在并返回 provider 的归一化 score；
- 顶层 `score` 与 namespace merge 使用共享 `public_score` projection；只有 hosted differential 证明相等时，才令它等于
  `reranking_score`；
- `vector_score`、`keyword_score`、对应 rank 与 fusion method 不被覆盖；
- `reranking.match_threshold` 针对 reranking score，先过滤再做 context expansion 与 chat context construction。

没有启用 reranking 时，response shape 和 score 行为遵守同一固定 Cloudflare contract，且不出现 `reranking_score`。请求级
`ai_search_options.reranking.model` 只能选择 `reranking_models` 中的 alias；未声明 alias、协议不匹配或没有 default 时返回稳定的
unsupported/config error，不回退到 generation model。

所有成功的 Search response 还必须包含：

```json
{
  "query_kind": "text",
  "search_query": "effective query",
  "chunks": []
}
```

`query_kind` 不是可选的本地诊断字段。namespace Search 同样返回 `"text"`；若请求包含任何 image／file content，则在搜索前拒绝，
不能错误返回 `"text"` 掩盖输入降级。

## 9. 配置、持久化与升级

- AI Search 公开 create/update/request schema 中已有 `reranking`、`reranking_model` 与 `retrieval_options.boost_by`；直接实现当前官方
  shape，不新增替代字段。
- create/update 将省略的 model 解析为 `default_reranking_model`，并把最终公开 alias 写入现有 public config JSON。
- per-request model override 在调用前解析 operator catalog；tenant 永远不能覆盖 remote model、protocol、endpoint 或 auth。
- instance 与 per-request `boost_by` 使用同一 metadata schema validator；request 数组完整替换 instance default，空数组显式关闭本次
  boosting。boost score 是 query-time value，不持久化到 chunk。
- `query_kind` 是 response projection，不进入 SQLite；当前所有成功 text search 都返回 `"text"`。
- 本阶段预计不需要 SQLite schema 变化；若实现发现必须持久化新字段，只能追加下一条 migration，不能修改已发布 migration bytes。
- 已持久化 instance 若引用旧 generation-only rerank alias，而 operator 未增加同名 `reranking_models` mapping，请求明确失败。Day 1
  直接迁移当前模型，不读取旧 capability、不自动复制配置、不保留 chat rerank。
- doctor 与 config check 验证所有 backend/model/default 引用及 protocol-specific `remote_model` 规则，但不联网探测 endpoint。
- support bundle 只记录 backend／model 数量、alias、protocol 和 secret-free digest；不记录 endpoint credential、request document 或
  provider response。

## 10. 实施 ownership

- `crates/core/src/config/ai/backend.rs`：增加两个 protocol token 与 operation validation。
- 新建 `crates/core/src/config/ai/rerank.rs`：拥有 reranking model config、validation 与解析；避免继续扩大接近 800 行的
  `config/ai.rs`。
- 新建 `crates/service/src/ai_provider/rerank.rs`：拥有 `RerankClient`、两个 codec 与统一 response validation；删除
  `OpenAiChatClient::rerank`，不把 `ai_provider.rs` 推过 production file budget。
- `crates/service/src/ai_search_config.rs`：解析 default／instance alias 并验证独立 model catalog。
- `crates/search/src/ai_search/`：从 fusion helper 删除错误的 fusion-score threshold；保留独立阶段 score，不让一个通用字段承担
  vector threshold、fusion、boost 和 rerank 四种语义。
- `crates/service/src/ai_search_backend/search.rs`：接入 normalized score、vector threshold、boosting、Cloudflare behavior matrix、
  `query_kind` 与统一 public-score projection；保持 transport codec 不进入 handler。
- `packages/runtime/src/ai-search/validation.ts` 与 `responses.ts`：接受当前官方 `boost_by`，严格验证 text-only input，并要求成功 Search
  response 含 `query_kind: "text"`；不能继续把官方字段当 unknown 或 unsupported。
- 同步 doctor、support bundle、operator config、Dashboard 提示、fixtures 与 tests；更新
  [`P5.2 provider profiles`](implemented/p5-2-ai-provider-profiles.md)、
  [`OC-AI-SEARCH-001`](references/p1-deviations.md)和
  [Cloudflare compatibility matrix](references/cloudflare-compatibility.md)中 generation/rerank ownership、text-only query limitation 与
  provider topology 的当前描述。
- 英文网站 `apps/website/src/content/docs/docs/ai-search/index.md` 与中文网站对应页必须同时说明：AI Search query 目前只支持文本，
  response 仍返回官方 `query_kind: "text"`，文档 OCR／VLM ingestion 不等于 image／multimodal query 支持。
- 删除所有 chat rerank prompt、parser、capability、fixture、测试和死 import；不保留 deprecated alias。

## 11. 验收

### 11.1 Config 与 codec tests

- 两种 backend protocol 与 model mapping 的成功配置；错误 protocol、缺失 backend、错误 default、缺少 remote model 全部拒绝。
- 每个 adapter 固定 exact request JSON、最终 endpoint、Content-Type、auth/header 和不追加 path。
- Cohere 与 generic v1 的成功 response 都归一化为相同结果；乱序输入得到确定性降序输出。
- duplicate/missing/out-of-range index、partial response、NaN/Infinity/out-of-range score、错误 JSON/root shape/content type、超限
  response 全部失败。
- 401/403、429/Retry-After、redirect、4xx、5xx、transport failure 和 timeout 保持现有稳定分类，且测试错误与日志不含 response
  body、credential、query 或 document。

### 11.2 Product regression

- AI Search vector、keyword 与 hybrid 三条路径分别证明 rerank 在 retrieval/fusion 后执行。
- `reranking.match_threshold` 使用 reranking score；普通 retrieval threshold 只使用 vector similarity，不再使用 fusion／boost／rerank
  score。vector、hybrid、keyword-only edge 均由固定 differential fixture 覆盖。
- instance／request `boost_by` 的 override、空数组关闭、metadata type／direction rejection、rerank 前顺序和 final limit 均有覆盖。
- 返回 chunk 同时保留原 scoring details，并增加 `reranking_score`；顶层 score、排序、namespace merge 和 chat context 使用固定
  Cloudflare differential 证明的统一 public-score projection，而不是实现者假定的 rerank score。
- instance 与 namespace Search 成功响应都必须包含 `query_kind: "text"`；string query／string message 成功，image、file 与
  multimodal content 在 provider 调用前稳定拒绝，且错误不回显 payload。
- instance default、instance explicit model、request override、request disable、未知 alias、未配置 default 和 provider failure 均有覆盖。
- query rewrite/chat 继续只使用 Chat Completions；删除 rerank capability 后现有 chat 与 SSE 行为不变。
- restart 后依赖 operator catalog 的同一 alias 可继续查询；缺失或协议改变时 fail closed，不需要 reindex，不写入 query-time score。

### 11.3 最终检查

实施完成并完成 review/fix 后，按根 `AGENTS.md` 执行 format、Clippy、no-default-features、Rust 1.98 MSRV、metadata、dependency
boundary、相关 TypeScript checks 与 coverage，最后只运行一次 `./test/gate.py --workspace`。协议 Gate 使用本地有界 fixture；真实外部
provider 调用只能作为另行授权的 qualification，不能成为普通开发 Gate 或隐式下载依赖。

## 12. 非目标

- OpenAI Rerank API、Chat Completions rerank、LLM listwise prompt 或 provider fallback；
- 任意 request/response template、JSONPath、脚本、plugin、动态 adapter 或未知字段映射配置；
- NVIDIA NIM `/v1/ranking`、Cloudflare Workers AI `contexts`、Bedrock、Vertex AI；出现明确需求后以新 protocol token 增加，不预留
  当前分支；
- image query、file query、multimodal embedding／retrieval／reranking。当前只支持 text query，并必须在 repository docs、能力矩阵、
  Dashboard 提示和英中文网站公开这一限制；
- provider discovery、启动探测、health polling、负载均衡、成本路由或自动重试；
- 改变 embedding、indexing、chunking 或独立 Vectorize 的模型和持久化合同。

## 13. 完成条件

P19 只有同时满足以下条件才能移入 `docs/implemented/`：

1. 两个协议均通过 exact wire fixture、错误矩阵与 AI Search product regression；
2. chat rerank 实现与配置能力已完全删除；
3. current text-query public API 的 `boost_by`、`query_kind`、scoring details、最终顶层 score、namespace merge 和两个 threshold 语义均有
   固定 OpenAPI／hosted differential／fixture 证据；
4. image、file 和 multimodal query 在 public boundary fail closed，所有 text Search response 返回 `query_kind: "text"`；
5. operator 英中文文档能分别给出 Cohere v2 与通用 `/v1/rerank` 配置；repository docs、Dashboard 和英中文网站均显著声明
   AI Search 当前不支持多模态 query；
6. `OC-AI-SEARCH-001` 与 capability matrix 不再声称 rerank 使用 OpenAI-compatible Chat API，也不把 text-only query 宣称为
   multimodal support；
7. 不存在旧字段双读、provider-specific production 分支、secret/query/document 泄漏或隐式外部下载；
8. 根仓要求的静态检查、coverage 和单轮 workspace Gate 均成功退出并记录当时 revision。
