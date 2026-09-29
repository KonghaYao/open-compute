# P20：Cloudflare Python Workers 完整兼容方案

状态：**planned**。官方合同、目标架构、实施顺序与验收边界已确定；尚未实现，不得宣称 Python Worker 已受支持。

本文定义 open-compute 对 Cloudflare Python Workers 的完整支持方案。这里的“完整”指：在
[Cloudflare 兼容矩阵](references/cloudflare-compatibility.md)已经声明的 Worker Runtime、binding、trigger 和产品范围内，Python
与 JavaScript 获得相同的公开能力、权限边界、生命周期和失败语义。它不把 Cloudflare 全球控制面、边缘调度或 open-compute
尚未声明支持的产品纳入范围。

P20 不新增第二套 Python runtime，也不把现有 JavaScript facade 翻译成一套 Python SDK。正式实现必须复用 workerd 内置的
Pyodide、Cloudflare 官方 Python entrypoint 和 `workers-runtime-sdk`；open-compute 只负责把已经存在的资源 authority 和 backend
以 workerd 的标准 binding 形态注入同一个 `env`。

## 1. 结论

目标数据流固定为：

```text
pywrangler / Wrangler multipart upload
  -> ocd admission + immutable Version
  -> workerd official Python module loader
  -> official Python entrypoint + Pyodide + workers-runtime-sdk
  -> one language-neutral env
       -> native workerd bindings (KV/R2/Queue/DO/Service/...)
       -> upstream wrapped bindings (D1 and products without a native class)
  -> existing ocd storage/resource authorities through narrow backend adapters
```

关键决策：

1. Python 主模块保持 `pythonModule`，由 workerd 官方 Python bridge 成为实际 entrypoint；不在 Python 外再套当前
   `__open_compute__/entry.js`。
2. 把当前 wrapper 中的 binding materialization 下沉为 **language-neutral env construction**。JavaScript 和 Python 使用同一份
   binding object，不再各自维护 facade。
3. KV、R2、Queue、Durable Objects 和 Service Binding 优先使用 workerd 原生 capability；D1 和没有原生类型的产品使用
   workerd `wrappedBinding` 机制承载官方或现有经过 qualification 的 TypeScript facade。
4. 复用现有 SQLite、对象存储、资源 descriptor、权限、generation fencing 和内部 transport；只在 workerd binding 与现有
   backend authority 之间增加协议 adapter，不重写 storage engine。
5. 包解析、wheel 选择和 bundling 由开发机上的 `pywrangler`/`uv` 完成。`ocd` 启动和请求路径不运行 `pip`、`uv`、Node、
   Bun，不联网下载 package。
6. 普通已部署 Python Worker 使用 deploy-time prepared artifact。request path 只验证并加载不可变产物，不执行安装、编译或
   snapshot 生成。
7. 只保留当前正式 workerd/Pyodide/SDK 组合。P20 qualification 完成时协调更新正式 pin；不保留旧 Python runtime、旧 SDK
   或双 binding 路径。
8. Dynamic Worker Python 是独立合同。未满足现有 fresh-isolate startup limit 前继续记录为限制，不为通过 Python 功能而放宽
   limit 或隐式复用不受 identity 约束的 warm isolate。

## 2. 官方合同与固定基线

方案冻结于 2026-09-29，实施前必须重新固定以下官方材料的 revision、package digest 和可观察 fixture：

- [Python Workers](https://developers.cloudflare.com/workers/languages/python/)；
- [Python basics](https://developers.cloudflare.com/workers/languages/python/basics/)；
- [How Python Workers Work](https://developers.cloudflare.com/workers/languages/python/how-python-workers-work/)；
- [Python packages](https://developers.cloudflare.com/workers/languages/python/packages/)；
- [Python FFI](https://developers.cloudflare.com/workers/languages/python/ffi/)；
- [Python standard library](https://developers.cloudflare.com/workers/languages/python/stdlib/)；
- [Python examples](https://developers.cloudflare.com/workers/languages/python/examples/)；
- [Dynamic Workers API](https://developers.cloudflare.com/dynamic-workers/api-reference/)；
- [`cloudflare/workerd`](https://github.com/cloudflare/workerd) 的 Python entrypoint、Pyodide、snapshot 和 binding 实现；
- [`cloudflare/workers-py`](https://github.com/cloudflare/workers-py) 的 `pywrangler` 与 `workers-runtime-sdk`；
- 固定 Wrangler upload builder、config schema、Workers types 和 multipart fixtures。

官方当前合同已经不是“Python beta 的最小 fetch demo”：Python Worker 使用 Pyodide，入口为继承 `WorkerEntrypoint` 的
`Default` class；`pywrangler` 负责 package bundling；FFI 可访问 JavaScript Runtime API 和 bindings；普通部署会在 deploy-time
执行入口模块及其 top-level import，并把专用 WebAssembly linear-memory snapshot 与源码一起部署。Dynamic Workers 也接受
`.py` / `{ py: string }`，但官方明确提示其启动明显慢于 JavaScript。

当前 open-compute 正式 runtime baseline 由
[`workerd.lock.json`](../packages/runtime/workerd.lock.json)唯一指定：

| 项目                         | 当前正式值                                 |
| ---------------------------- | ------------------------------------------ |
| workerd fork release         | `v1.20260918.1-open-compute-i102.1c7b89be` |
| source revision              | `1c7b89bea323a39a8511271913820f9fcf39306d` |
| version output               | `workerd 2026-09-18`                       |
| effective compatibility date | `2026-09-08`                               |
| Pyodide bundle               | `314.0.6_2026-08-17_2`                     |
| Workers types                | `5.20260830.1`                             |
| Wrangler snapshot            | `4.138.0`                                  |

这些值只说明二进制含有对应 Pyodide bundle，不证明 open-compute upload、binding 或 deploy lifecycle 已支持 Python。P20
开始实施时先用同一套正式 pin 完成 G0；若官方 upload/SDK 合同需要更新，再按 runtime pin policy 一次性协调升级并重新跑全部
受影响 Gate。

## 3. “完整兼容”的明确范围

### 3.1 必须支持

- `main = "src/entry.py"`、`text/x-python`、多 Python module 与 package data；
- `python_workers`、`python_workflows` 及固定 runtime 所需的官方 compatibility flags；
- `workers-runtime-sdk` 的 `WorkerEntrypoint`、named entrypoint、RPC、`scheduled`、Queues、Durable Objects 和 Workflows；
- Pyodide FFI、`js` module、Web Platform globals、公开 outbound `fetch()` 与已声明的 socket API；
- pure-Python、PyEmscripten wheel、固定 Pyodide package 和 importable file；
- open-compute 兼容矩阵内的 vars、secrets、KV、D1、R2、Queues、Durable Objects、Service Binding、Workflows、assets、Cache、
  Workers AI、Vectorize、Images、AI Search 与 Artifacts；
- 与 JavaScript 相同的 route、deployment、rollback、logs、limits、permission、secret redaction、restart 和 crash recovery；
- `pywrangler deploy` 通过现有 Cloudflare v4 upload endpoint，无 vendor-only deployment API。

### 3.2 不自动纳入

- open-compute 兼容矩阵明确列为 unsupported/non-target 的 Cloudflare 产品；
- Cloudflare 全球边缘 placement、multi-region replication、managed fleet rollout 和 account billing；
- 任意 CPython native extension。支持范围是 pure-Python、PyEmscripten 与固定 Pyodide 生态，不是宿主机 CPython ABI；
- request-time package install、任意外部 package index、宿主文件系统或宿主 Python；
- 未经固定 SDK 和 differential fixture 验证的新 Cloudflare Python convenience wrapper。

文档、capability manifest 和网站只能写“open-compute declared surface 的 Python parity”。不得简写为“所有 Cloudflare Python
产品 100% 等价”。

## 4. 当前差距

| 层               | 当前行为                                                     | 与官方合同的差距                                        |
| ---------------- | ------------------------------------------------------------ | ------------------------------------------------------- |
| upload           | `main_module` 固定按 `EsModule` 解析；`text/x-python` 被拒绝 | 无法上传 `.py` 主模块或 Python module                   |
| immutable bundle | `ModuleType` 没有 Python                                     | manifest、digest、download round-trip 都不能表达 Python |
| RuntimeSource    | runtime protocol 没有 `py`                                   | 不能把 Python module 交给 Worker Loader                 |
| loader           | `moduleValue()` 只映射 JS/CJS/text/json/data/wasm            | 没有 `{ py: string }` / `pythonModule`                  |
| entrypoint       | 每个 tenant 都生成平台 JS main wrapper                       | 会遮蔽 workerd 官方 Python entrypoint                   |
| binding          | facade 在 JS wrapper 内创建                                  | Python official entrypoint 收不到最终 binding object    |
| compatibility    | 只接受当前 JavaScript flag 子集                              | `python_workers` 等无法进入 immutable Version           |
| package          | 没有 pywrangler upload qualification                         | package modules、wheel、SDK pin 未验证                  |
| prepare          | 没有 dedicated snapshot lifecycle                            | cold start 与官方普通部署行为不一致                     |
| capability/docs  | `python_modules` 为 unsupported，网站仍写 beta/loader demo   | 对外状态与官方当前产品不一致                            |

以上不是几个独立的小缺陷。根因是当前 runtime assembly 把“语言 entrypoint”和“open-compute binding facade”耦合在同一层 JS
wrapper 中。P20 必须先拆开这两个职责，逐项放行 MIME 或 flag 不能形成可维护的 Python 支持。

## 5. 目标 runtime 架构

### 5.1 官方 Python wrapper 是唯一语言入口

workerd 收到 Python main module 后会注册 Python runtime modules，并以官方 `PYTHON_ENTRYPOINT` 完成 Pyodide 初始化、导入用户
module、发现 handler/class 和 Python/JavaScript 值转换。这是 Cloudflare runtime 的实现机制，不是用户可见的额外 Worker。

因此：

- `RuntimeSnapshot.mainModule` 继续指向用户 `.py`；
- loader 传递真实 Python module type；
- 不把 main 改写为 `__open_compute__/entry.js`；
- 不 fork `workers-runtime-sdk`，也不复制 `python-entrypoint.js`；
- open-compute 的 policy、binding 和 observability hook 必须位于 entrypoint 之下或 workerd host 边界，而不是再包一层 tenant
  handler。

JavaScript Worker 可以继续使用 wrapper 来适配 entrypoint 行为，但 binding construction 不能继续只存在于该 wrapper。两种语言
最终必须从同一 host-owned materializer 获得 `env`。

### 5.2 binding materializer

新增一个 loader-owned、tenant 不可见的 binding materialization 阶段。输入只允许经过 Rust authority 验证的
`RuntimeSnapshot` descriptor 和 capability-scoped host transport；输出是交给 workerd isolate 的最终 `env`。它不接受 tenant
提供 endpoint、token、resource ID 或 module specifier。

优先级固定为：

1. workerd 原生 binding；
2. workerd/upstream official wrapped binding；
3. 现有 open-compute TypeScript facade 作为一个 `wrappedBinding` module；
4. 没有可验证实现则该 binding 对 Python 保持 unsupported，不能退回 raw internal Fetcher。

| binding             | 目标对象                                              | backend 复用方式                                     | P20 动作                                        |
| ------------------- | ----------------------------------------------------- | ---------------------------------------------------- | ----------------------------------------------- |
| KV                  | workerd native `KvNamespace`                          | adapter 把标准 KV subrequest 映射到现有 KV authority | 删除 public `KVNamespace` facade 路径           |
| R2                  | workerd native `R2Bucket`                             | adapter 映射到现有 R2 object authority/S3 backend    | 删除 public `R2Bucket` facade 路径              |
| D1                  | upstream `cloudflare-internal:d1-api` wrapped binding | 保留现有 D1 engine，提供其标准 Fetcher wire          | 删除 open-compute 重复 public D1 facade         |
| Queue               | workerd native `WorkerQueue` 或固定 upstream wrapper  | 现有 queue descriptor/transport                      | 移除 wrapper-only construction                  |
| Durable Object      | native `DurableObjectNamespace`                       | 现有 router、identity 和 output gate                 | 保持 native object，不转成 JSON/RPC DTO         |
| Service             | native `Fetcher` / RPC stub                           | 现有 service routing和 generation fence              | 直接进入 env                                    |
| Workflow            | 固定 upstream wrapper；不足部分用一个 wrapped binding | 现有 workflow authority                              | 同一个对象供 JS/Python 使用                     |
| Assets/Cache        | native Fetcher/Cache 或现有 wrapped binding           | 现有 asset/cache backend                             | 从 entrypoint wrapper 下沉                      |
| AI/Vectorize/Images | 固定公开 class；必要时使用现有 facade wrapped binding | 现有 product transport                               | 复用 facade source，不复制 Python 实现          |
| AI Search/Artifacts | open-compute extension wrapped binding                | 现有 extension transport                             | 明确为 vendor extension，不冒充 Cloudflare 产品 |

`workers-runtime-sdk` 的 env wrapper 只负责把已经存在的 JavaScript binding 变成更 Pythonic 的对象；它不是 KV、D1 或 R2
backend。因此仅“复用 Python SDK”不能完成注入，必须先让 `env.KV`、`env.DB`、`env.BUCKET` 本身成为正确的 workerd binding。

当前 fork 已有 Frankenvalue/capability transport 和 `kvNamespace`、`r2Bucket`、`wrappedBinding` 等 host primitive。P20 先做 G0
证明 private Worker Loader 能从 trusted descriptor materialize 这些对象；tenant JavaScript 不能序列化或伪造 native binding。
若现有 API 不足，只在 `third_party/workerd/` 增加一个最窄的 host-only construction 接口，不增加第二套 loader、公共 token 或
tenant-visible escape hatch。

### 5.3 backend adapter，而不是 storage 重写

当前 TypeScript facade 说的是 open-compute 私有 RPC/binary protocol；workerd native binding 说的是 Cloudflare/Miniflare 已有的
internal subrequest protocol。两者不能直接互换。每个产品新增一个位于 system Worker/host service 边界的 adapter：

```text
native/wrapped binding request
  -> validate operation, headers, size and resource capability
  -> translate to existing typed backend call
  -> translate canonical result/error/stream back to upstream wire
```

adapter 必须复用当前 resource identity、permission、generation、quota、transaction 和 redaction；不得绕过 `workers`/`storage`
authority 直接访问 SQLite 或对象目录。流式 body 保持 bounded backpressure，不把 R2 object 或 D1 dump 全量缓冲到 JS heap。

每个 adapter 以固定 upstream implementation 的真实请求 fixture 为合同。不要为未观察到的 internal header 建通用代理框架。

### 5.4 wrapper 职责迁移

当前 `__open_compute__/entry.js` 还承担 cache、scheduled workflow、loopback、observability 和错误清理。P20 逐项归位：

| 当前职责                       | 目标位置                                                       |
| ------------------------------ | -------------------------------------------------------------- |
| binding facade creation        | language-neutral materializer                                  |
| resource permission/generation | Rust authority + backend adapter                               |
| request identity/observability | Gateway/host dispatch context                                  |
| stable error sanitization      | product adapter和 service response boundary                    |
| scheduled/queue dispatch       | workerd native Python handler dispatch                         |
| Workflow entrypoint            | official Python `WorkflowEntrypoint` + shared workflow binding |
| Durable Object wrapping        | native Python DO entrypoint + existing object host policy      |
| automatic cache policy         | host/cache binding policy，不依赖 tenant main language         |
| dynamic forwarding             | shared materializer；只转发明确允许的 typed capabilities       |

只有 JavaScript export-shape adaptation 仍留在 JS wrapper。任何安全策略如果 Python 绕过 wrapper 就失效，说明它仍在错误层，P20
不得以“Python 特例”复制一份。

## 6. Upload、bundle 与 toolchain

### 6.1 Cloudflare v4 upload

直接扩展当前模型，不增加 vendor upload endpoint：

- `ModuleType::Python`，稳定序列化名 `python`；
- multipart 接受官方 Python content type（至少固定 Wrangler/pywrangler 实际发送的 `text/x-python`）；
- `.py` main 必须对应 Python module type，JS/CJS/Python main 互斥；
- download round-trip 保留相同 module bytes、name、content type 和 main metadata；
- canonical bundle digest 覆盖 Python source、package module、package data 和 SDK module；
- path、UTF-8、duplicate name、reserved prefix、module count、单 module/总 bundle size 继续 fail closed；
- obsolete `python-requirement` module 不作为兼容 fallback；官方当前 package 流程上传已解析的 package 内容。

当前总 module bytes 上限为 16 MiB。P20.0 必须用固定 pywrangler fixture确认 Cloudflare 当前 Python upload limit；若声明与官方
一致，需要在同一变更中协调 limit、streaming admission、disk budget 和 Gate。不能只为一个 fixture 放大内存缓冲。

### 6.2 pywrangler

正式开发流程：

```text
uv run pywrangler dev
uv run pywrangler deploy
```

`pywrangler` 继续调用 Wrangler 的标准命令和 Cloudflare v4 endpoint。open-compute 不 fork CLI；只在需要选择 open-compute
account/base URL 时提供与现有 Wrangler integration 相同的配置/launcher。所有 network package resolution 都发生在开发机，
上传物是完整、自包含、可做 digest 的 module set。

qualification fixture 必须包含：

- 无依赖 hello world；
- 多文件 local import 与 package data；
- `workers-runtime-sdk`；
- pure-Python dependency；
- PyEmscripten wheel；
- 固定 Pyodide package；
- FastAPI、Flask、Django 各一个最小真实应用。

`ocd` 对上传内容做结构、digest、limit、compatibility 和 runtime validation，但不读取 `pyproject.toml` 后自行解析 dependency。

## 7. Prepared Python artifact 与 snapshot

### 7.1 普通部署

官方普通部署会执行 top-level imports 并生成 dedicated memory snapshot。open-compute 必须复用 workerd snapshot machinery，而不是
把首次请求当成 prepare。目标生命周期：

```text
uploaded Version (source authority)
  -> isolated validation/prepare process
  -> official Python import + package initialization
  -> dedicated snapshot + runtime metadata
  -> encrypted immutable prepared artifact
  -> atomic deployment promotion
  -> request isolates restore exact prepared artifact
```

若 stock/fork workerd 没有适合 self-hosted service 调用的 deploy-time ArtifactBundler 接口，在
`third_party/workerd/` 增加 narrow host-only prepare/restore API，直接调用现有 Python snapshot implementation。不得在 Rust 或
TypeScript 重写 Pyodide snapshot format。

### 7.2 authority 与 cache key

Version 的 source bundle 仍是权威事实；prepared artifact 是该 Version 的不可变派生产物。identity 至少覆盖：

- canonical module manifest/digest；
- compatibility date 和 ordered flags；
- workerd source revision、binary digest 与 process flags；
- Pyodide bundle digest；
- `workers-runtime-sdk` 和 bundled package digests；
- runtime/system Worker asset manifest；
- binding descriptor digest、secret generation 与其他 top-level 可观察 env generation；
- prepare format/schema version。

任一输入变化都生成新 prepared identity，绝不原地覆盖。缺失、截断、digest 不匹配、runtime pin 不匹配或 metadata 不完整时部署
fail closed；request path 不联网、不重新 prepare、不悄悄退回 baseline snapshot。

### 7.3 secret 和顶层副作用

Python module top level 可能访问 env、secret 或 package state，因此 dedicated snapshot 按含 secret 的 deployment artifact 处理：

- 使用 instance master key 和独立 AEAD context 加密；
- 路径、日志、API、metrics 和错误不暴露 snapshot bytes、secret 或 import traceback 中的值；
- secret rotation 创建新 Version/prepared artifact，旧 deployment 回滚仍只读取其原有 immutable identity；
- prepare process 使用与正式 isolate 相同的 capability set，不能访问 control API、S3 credential、SQLite handle 或内部 token；
- prepare 失败返回稳定、清理后的 upload/deploy error，详细诊断只进入受控的 sanitized evidence。

不要先做“可选 snapshot cache”。普通 Python deployment 的 prepared artifact 是部署正确性输入；Dynamic Worker 才允许使用
baseline/no-dedicated-snapshot 路径，并保持其独立 limit。

## 8. compatibility date、flags 与 SDK

- `python_workers` 进入允许列表，并由 upload/version authority 固定；
- `python_workflows`、dedicated snapshot 等 flags 只在正式 workerd revision 中存在且 fixture 通过后启用；
- 未知、互斥、runtime 不支持或 Python-only flag 用在错误 module kind 时 upload fail closed；
- 不为早期 open-compute Python prototype 保留 flag alias 或默认补旗；
- `workers-runtime-sdk` 由 pywrangler bundle 固定，server 不从 PyPI 解析“latest”；
- release qualification 记录 SDK name、version、wheel/sdist digest、source revision 和 license；
- types/autocomplete 是开发工具输出，不是 server runtime authority。

P20 完成后，网站中“Python Workers 仍为 beta”和只展示 Dynamic Worker loader demo 的页面必须改成正式普通部署流程；如果 Dynamic
Worker startup 仍受限，单独写明该限制，不把它泛化为全部 Python Worker 状态。

## 9. Dynamic Worker Python

Worker Loader 的 Python code shape 与官方一致：

```ts
{
  compatibilityDate: "YYYY-MM-DD",
  compatibilityFlags: ["python_workers"],
  mainModule: "worker.py",
  modules: { "worker.py": { py: "..." } }
}
```

P20 必须让 shared module parser、binding forwarding allowlist 和 language-neutral materializer接受 Python，不再生成只适用于 JS 的
loaded-isolate wrapper。仍需保持：

- `load()` 每次 fresh isolate；`get()` 只按完整 immutable identity 缓存；
- caller 只能转发显式允许的 typed binding，不得传 raw platform capability；
- `globalOutbound`、limits、tails 和 DO facets 维持当前 security contract；
- Dynamic Python 没有 dedicated snapshot 时明确使用官方 baseline 行为；
- fresh-isolate 1 秒启动限制继续作为 `OC-WKR-LIMIT-001` qualification。若真实固定 runtime 不能通过，就保留公开限制，不建立
  hidden warm-up service、预加载租户源码或放宽 Gate。

## 10. 实施顺序

### P20.0：固定合同与 G0

- 固定 workerd、Pyodide、workers-py、workers-runtime-sdk、Wrangler、Workers types 和官方 docs revision；
- 捕获 pywrangler multipart、module types、package layout、flags、普通部署 snapshot 和 Dynamic Worker fixtures；
- 用正式 pin 验证 Python main、dedicated snapshot、Worker Loader `{py}` 和每类 binding materialization primitive；
- 输出支持／不支持／未验证矩阵；G0 代码放 `.temp/`，结论写回本文件或兼容矩阵，不建立长期 POC tree。

退出条件：知道 stock/fork workerd 中哪些能力可直接复用、唯一需要的 host-only fork surface，以及正式 pin 是否必须更新。

### P20.1：module admission

- 增加 Python module type、multipart/download round-trip、canonical bundle 和 RuntimeSource wire；
- 加入 Python flags validation；
- 用真实 pywrangler bundle 验证 upload/version/deployment/rollback；
- Python capability 仍保持 disabled，直到 P20.2 和 P20.3 通过。

### P20.2：language-neutral bindings

- 实现一个 host-owned materializer；
- 先迁移 KV、R2、D1、Queue、DO、Service；
- 再迁移 Workflow、Assets、Cache、AI、Vectorize、Images、AI Search、Artifacts；
- JavaScript 和 Python differential 同时通过后，删除对应 wrapper-only construction 与重复 public facade；
- 任一产品不得长期保留 JS-old/Python-new 双路径。

### P20.3：official Python entrypoint

- loader 传递 Python modules，让 workerd official entrypoint 成为 main；
- 支持 fetch、named/RPC、scheduled、queue、DO 和 Workflow；
- 把 observability、limits、errors、cache policy 等剩余 wrapper 职责迁到共同边界；
- 删除 Python 外层 JS main 实验代码和任何 SDK patch。

### P20.4：prepared artifact

- 接通 workerd official validation/snapshot/restore；
- 持久化加密 immutable prepared artifact 与完整 identity；
- 完成 promotion、rollback、restart、secret rotation、corruption 和 crash recovery；
- 确认 request path 无安装、编译、prepare 和网络访问。

### P20.5：tooling 与 ecosystem

- qualification `pywrangler deploy`、types 和 dev handoff；
- qualification package matrix、FastAPI、Flask、Django、requests/httpx；
- 修正 capability manifest、兼容矩阵、偏差清单、英文/中文网站和 examples；
- 只在所有必选 Gate 通过后把 Python 状态改为 supported。

### P20.6：Dynamic Python

- 在普通部署完成后再接通 `{py}` Worker Loader 和 binding forwarding；
- 跑 fresh-isolate limit、network policy、facets、tails、revocation 和 error fixtures；
- 不因普通 Python Worker 已支持而自动把 Dynamic Python 标为 supported。

## 11. 验收矩阵

所有 product Gate 使用正式 verified workerd、真实进程、真实 SQLite、当前对象 backend 和 fresh process。Miniflare、mock binding 或
stock upstream binary 只能用于差异定位，不能代替最终证据。

### 11.1 主 Python 链 blocking Gate

P20 实施时必须新增产品目标 `p20-python-main`，并纳入 `all`、`p3` 和最终 `--workspace` 的单轮计划。它不是可选
qualification；该目标不存在、未注册、被忽略或未通过时，普通 Python Worker 必须继续标为 unsupported。

`test/gate_cases.py` 只登记一个拥有整条真实进程链路的 `TIMING` case：

```text
python_main::p20_python_main_upload_prepare_dispatch_restart_rollback
```

该 case 在同一个隔离 test scope 中依次证明：

1. 使用固定 pywrangler 生成、已记录版本和 digest 的 multipart fixture，经真实 v4 endpoint 上传 `.py` main、多 module、
   package data、vars、secret、KV、D1 与 R2 binding；
2. admission 固定 module type、compatibility date/flags、bundle digest 和 binding descriptor，拒绝错误 MIME、缺失 main、未知 flag、
   duplicate/reserved module 和损坏 package；
3. 真实 verified formal-pin workerd 执行官方 Python entrypoint，prepare 只发生一次并发布加密 immutable prepared artifact；
4. promotion 后通过公开 Worker endpoint 调用 Python `fetch`，同时读写 KV、D1、R2，并验证 response、stream、exception、secret
   redaction 和 host-network outbound policy；
5. 终止并重新启动 workerd generation，再调用同一 deployment，证明使用同一 prepared identity restore，而不是 request-time
   install、compile、prepare 或 warm-memory fallback；
6. 上传第二个 Python Version、promote 后再 rollback，证明 route/deployment pin、binding generation 和旧 snapshot identity 精确恢复；
7. 在 test-support fault point 注入 partial/corrupt prepared artifact，证明 failed prepare/restore 不改变 active deployment，重启后
   仍 fail closed，且不遗留进程、listener、临时文件或 plaintext secret；
8. 最终核对 active Version、deployment、prepared artifact、KV/D1/R2 数据和进程清理状态，并输出去 secret 的 Gate evidence。

case 内部的两次 Version、重启、rollback 与确定性 fault point 是一个不可拆的 lifecycle matrix，不计为重复 Gate round，也不允许
失败自动重试。fixture 必须是开发侧预先生成并提交的静态输入；Gate 不运行 `uv`/`pywrangler`、不下载 package、不访问 Cloudflare
账号。实现第一个可执行 case 时，同一变更必须完成 `test/gate.py` target、`test/gate_cases.py` 注册、`--list` inventory、
`docs/references/testing.md` 映射和 `p3-contract` capability/case 双射；不得先注册空 target 或只在文档中声称 Gate 存在。

`p20-python-main` 只拥有普通 Python Worker 主链。框架/package 扩展矩阵和 Dynamic Python 使用独立 case/target；它们不能替代此
blocking Gate，也不能让同一个主链 case 在 workspace 中被第二次调度。

### 11.2 entrypoint 与 runtime

- fetch：request/body/headers/streaming response、exception、`waitUntil`；
- named entrypoint 与 RPC method，包括 Python/JS structured value round-trip；
- scheduled、queue batch/retry、Durable Object fetch/RPC/alarm/storage、Workflow run/step；
- `js` FFI、Request/Response、Web Streams、crypto、console/logging、public `fetch()`、declared sockets；
- supported stdlib 与官方 excluded/non-functional module inventory；
- filesystem 仅 isolate ephemeral，销毁后不可依赖。

### 11.3 bindings

每个声明 supported 的 binding 至少覆盖：

- Python 与 JavaScript 对同一资源的成功结果 differential；
- 参数边界、stream/body、pagination/batch、metadata 和公开 exception shape；
- read-only/write permission、stale generation、deleted resource、quota/size limit；
- restart、rollback 与并发 mutation；
- `repr`、error、logs 和 metrics 不泄露 descriptor、internal token、path 或 secret；
- SDK convenience wrapper 与 raw FFI 两条官方使用方式。

KV、D1、R2 必须作为首批 blocking Gate；它们未通过时不能宣称 Python binding parity。

### 11.4 packages

- multi-module import、relative/package import、package data；
- pure-Python、PyEmscripten、Pyodide package；
- unsupported native wheel 的稳定拒绝；
- FastAPI、Flask、Django；
- top-level import error、missing module、corrupt wheel/data、bundle limit；
- requests/httpx 只经统一 host-network outbound capability；与 JavaScript 一样由宿主 firewall、network namespace、容器或 VM
  负责地址过滤，不新增 Python 专用代理或规则层。

### 11.5 prepared artifact 与生命周期

- clean deploy 生成一次、后续请求只 restore；
- daemon/workerd restart 后加载同一 identity；
- source、flag、runtime、Pyodide、SDK、binding 或 secret generation 变化必然换 identity；
- prepare crash、disk full、partial write、checksum mismatch、AEAD failure、unsupported schema 都 fail closed；
- failed prepare 不改变 active deployment；promotion/rollback 原子；
- snapshot、temporary file、process 和 listener 全部清理，无 orphan；
- cold isolate latency 与 memory budget 满足固定产品 limit。

### 11.6 differential evidence

对 Cloudflare remote、固定 upstream workerd 和 open-compute 分别记录：

- upload acceptance/response；
- handler结果与 error shape；
- binding方法、stream和exception；
- package/import行为；
- compatibility flag/date 行为；
- cold/warm/restart lifecycle。

不可访问或无法稳定观察的 Cloudflare internal snapshot bytes 不做 byte-for-byte 比较；比较公开行为，并验证本地 artifact 的
identity、加密和恢复不变量。

## 12. 安全与失败语义

- `ocd` 仍是唯一公开 listener 和 deployment identity authority；
- Python isolate只看到声明的 vars、secrets 和 public bindings；
- RuntimeSource、SQLite/S3 handles、internal Fetcher/token、prepare service 和 control API 永不进入 tenant env；
- Python/FFI outbound 与 JavaScript 共用单一 `Network(allow = ["network", "local"], deny = ["unix", "unix-abstract"])` IP capability；Unix／abstract-Unix endpoint
  不开放，validation 及未委托 outbound 的执行保持 `globalOutbound = null`；
- tenant 不能通过 Python object、pickle、FFI、Frankenvalue 或 RPC 构造额外 capability；
- upload、prepare、restore、binding adapter 和 SDK mismatch 均返回稳定 sanitized error；
- traceback 可指向 tenant module，但必须清理绝对路径、源码外 secret、signed URL、authorization 和内部拓扑；
- snapshot 和 package artifact 遵守既有 atomic write、fsync、permission、symlink/path containment 和 data-dir lock 规则。

## 13. Day 1 删除清单

P20 采用直接 current-model 迁移，不保留过渡兼容层。完成时删除：

- Python 外层 `__open_compute__/entry.js` 或任何语言特判 wrapper；
- 已被 native/upstream binding替代的 open-compute public KV/D1/R2 facade；
- wrapper-only binding factory 和重复的 JS/Python injection path；
- beta/unsupported Python 状态、过时 loader-only example 和旧 capability fixture；
- package/runtime auto-download、request-time install 或 baseline fallback（若实施过程中出现）；
- G0 disposable code、临时 SDK patch、旧 pin selector 和 dual snapshot reader。

保留现有 facade source 仅限仍作为唯一 authoritative `wrappedBinding` implementation 的产品，并移动到 binding-owned module；不能
同时保留 wrapper facade 与 materializer facade 两份。

## 14. 不采用的方案

- **把 Python 编译成 JavaScript**：不兼容 Pyodide、FFI、package 和官方 entrypoint。
- **在 Python SDK 中重写 KV/D1/R2 client**：产生第二套公开 API 和 protocol，且绕开 upstream binding behavior。
- **所有 Python Worker 外套平台 JS main**：阻断 workerd 官方 entrypoint 和 snapshot lifecycle。
- **把当前 raw internal Fetcher 直接放进 Python env**：公开对象类型、方法和错误均不符合 Cloudflare binding。
- **在 request path 运行 uv/pip**：破坏离线启动、不可变部署、延迟和供应链边界。
- **用 Miniflare 作为生产 Python runtime**：引入 Node sidecar和第二套 lifecycle，且不能替代正式 workerd Gate。
- **长期保留 JS 与 Python 两套 binding**：同一产品会出现语义漂移，违反 Day 1 单一实现。
- **先宣称支持、再逐个补 binding**：Python compatibility 是跨 upload/runtime/binding/lifecycle 的系统合同，必须在 blocking matrix
  通过后一次性更新 capability 状态。

## 15. 完成条件

P20 只有同时满足以下条件才可移入 `docs/implemented/`：

1. `p20-python-main` 的注册 inventory 与唯一主链 case 存在；开发阶段选择 `./test/gate.py p20-python-main` 时必须通过，源码冻结后
   只由最终 `--workspace` 单轮执行该 case，不在同一 final iteration 重复运行；固定 pywrangler 普通部署 fixture 能通过标准
   Cloudflare v4 endpoint 创建、部署、调用、重启和回滚 Python Worker；
2. official Python entrypoint、Pyodide 和未 fork 的 workers-runtime-sdk 运行；
3. open-compute declared binding surface 对 Python/JavaScript 使用同一 language-neutral materializer，并通过 differential；
4. KV、D1、R2、Queue、DO、Service、Workflow 及其失败/权限/restart Gate 全部通过；
5. dedicated prepared artifact 的加密、identity、promotion、rollback、restart、corruption 和 secret rotation 验收通过；
6. package、framework、FFI、stdlib、outbound 和 observability matrix 通过；
7. Dynamic Python 单独标明 supported 或保留精确公开限制，不混同普通部署；
8. capability manifest、兼容矩阵、偏差、website、examples 和 release evidence 与实测一致；
9. 删除旧 facade/双路径/临时代码，并完成 format、lint、boundary、coverage 和一次 final workspace Gate；
10. 正式 workerd/Pyodide/SDK pin、source identity、digests、license 和 differential evidence 已记录且 release 可离线复现。
