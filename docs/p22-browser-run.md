# P22：Cloudflare Browser Run 兼容设计

状态：planned。cf 入口已核对；运行方式确定为 operator 配置的本地浏览器或外部 CDP URL；本机资源探针已完成，
固定客户端兼容、session CDP 隔离和正式宿主 qualification 尚未完成，不能据此宣称 Browser Run 已实现。

本文细化 [P6 Cloudflare v4 API 管理面](implemented/p6-cloudflare-v4-wrangler-compatibility.md) 中的 `browser` binding、
Browser Run API、DevTools session 和 cf commands，应用入口遵循 [P20](implemented/p20-cf-cli-migration.md)。
浏览器不打包进 `ocd`；安装目录由 operator 准备，官方推荐 `chrome-headless-shell`，生产启动不下载、不自动升级、不搜索 PATH。
本地进程管理复用 [P17](implemented/p17-host-process-infrastructure.md) 的 verified spawn、process group、bounded stdio、
lease/orphan recovery；实例归属遵循 [R1](implemented/r1-single-daemon-instances.md)。BrowserManager 拥有浏览器特有的
readiness、context、session deadline 和空闲退出，不并入 WorkerdSupervisor。

## 1. 范围与结论

Cloudflare 已把 Browser Rendering 产品名更新为 **Browser Run**，但 API/config/binding 仍使用
`browser-rendering` / `browser`。open-compute 保留这些标准名字。

P22 Day 1 目标：

- `cloudflare.config.ts` 的 `worker.env.BROWSER: bindings.browser()` 与标准 upload metadata；
- pinned workerd 中固定 `@cloudflare/puppeteer` / `@cloudflare/playwright` 的 Browser Fetcher binding；
- 固定 Workers types 的 `BrowserRun.fetch()`、`BrowserRun.quickAction()`；
- BR0 冻结的 `cf browser-run devtools`、public Quick Actions、DevTools HTTP 与 CDP WebSocket；
- operator 选择一种运行方式，tenant binding/config/request 不能选择 binary、endpoint 或 launch flags。

两种运行方式互斥，均归属明确的 OCD instance：

1. **managed**：配置绝对 executable 路径并保留安装目录中的配套资源。每个 instance 最多拥有一个按需启动的浏览器主进程
   及其 Chromium 子进程组；不同 instance 不共享主进程。每个平台 session 分配独立临时 BrowserContext，context 中的
   cookies、站点存储和页面相互隔离。BrowserManager 负责启动、回收、空闲关闭与 crash/restart 收敛。
2. **cdp**：配置 browser-level CDP HTTP(S) discovery URL 或 WebSocket URL。使用目标浏览器的原生 context、profile、
   close 和其他 CDP 行为；operator 管理远端进程和隔离策略。`ocd` 不因本地空闲定时器启动、关闭或重启外部浏览器，
   不把本地 managed 的 context 隔离保证套到该目标上。显式客户端 CDP 命令按支持合同转发。

managed 模式只有临时浏览器状态，不提供持久 BrowserProfile、profile import/export 或跨进程重启登录恢复。
`user-data-dir` 是每个进程 generation 独立的临时宿主目录，退出并完成 reap 后安全清理，不是每个 session 的持久目录。
客户端 disconnect 后的 session 可在标准 keep-alive 内重连；session 关闭、超时或 lost 后登录态消失。

**共享主进程不等于原样透传独立 browser endpoint。** managed session 作为独立浏览器暴露时，必须实施第 9 节规定的
context/target/CDP 授权与窄的协议映射，并通过固定客户端验证。不能只分配 context ID 后透传 browser-level CDP，
也不能自动退回每 session 一个进程来掩盖 qualification 失败。该映射未通过时，对应 managed API 保持 unsupported。

## 2. Compatibility authority

实施和 qualification 固定：

- [Cloudflare Browser Run API](https://developers.cloudflare.com/api/resources/browser_rendering/)；
- [Chrome DevTools Protocol](https://developers.cloudflare.com/browser-run/cdp/)；
- [Browser session management](https://developers.cloudflare.com/browser-run/cdp/session-management/)；
- [Cloudflare CLI source](https://github.com/cloudflare/cf) 的 Browser Run 命令、schema 与 SDK transport；
- [Browser Run limits](https://developers.cloudflare.com/browser-run/limits/)；
- [Cloudflare Puppeteer](https://developers.cloudflare.com/browser-run/puppeteer/)与固定 package；
- [Cloudflare Browser Run changelog](https://developers.cloudflare.com/browser-run/changelog/)中的 standard/full CDP 声明；
- [Live View](https://developers.cloudflare.com/browser-run/features/live-view/)；
- [Browser Run rename changelog](https://developers.cloudflare.com/changelog/post/2026-04-15-br-rename/)；
- [Chromium Headless README](https://chromium.googlesource.com/chromium/src/+/master/headless/README.md)、
  [Chrome Headless Shell](https://developer.chrome.com/docs/automation-and-testing/headless-chrome-shell)与
  [Chrome for Testing asset matrix](https://github.com/GoogleChromeLabs/chrome-for-testing#supported-platforms)；
- [CDP Target.createBrowserContext](https://chromedevtools.github.io/devtools-protocol/tot/Target/#method-createBrowserContext)、
  [Headless context creation](https://chromium.googlesource.com/chromium/src/+/HEAD/headless/lib/browser/headless_devtools_manager_delegate.cc)、
  [Puppeteer BrowserContext](https://pptr.dev/api/puppeteer.browsercontext) 与
  [Cloudflare session reuse](https://developers.cloudflare.com/browser-run/features/reuse-sessions/)；
- 仓库固定 `cf@1.0.0-beta.12`、其 `cf/config`（`@cloudflare/config@0.23.0`）、官方 Vite 插件 v2 与 Build Output/upload producer；
- BR0 固定准确 CLI/package revision、命令 help/schema、SDK response handling 与真实 wire fixture；
- 固定 `@cloudflare/puppeteer`、`@cloudflare/playwright`、Workers types 与 Miniflare source snapshot；
- 固定 Cloudflare HTTP/WebSocket trace 和 OpenAPI revision/hash。

网页和 upstream source 用于发现合同；进入 Gate 的 route、query、body、header、raw response、WebSocket frame、close code、
错误与 package call sequence 都必须固定为 fixture。Browser Run 当前仍在快速演进，未进入 inventory 的新功能默认
unsupported。

Cloudflare 当前把默认 Browser Run 描述为 headless Chrome，并声明 standard/full CDP 与完整 Puppeteer API，但 CDP endpoint
仍为 Beta，且官方文档存在 Workers/browser service 约束。P22 不把整份 Chrome CDP schema 自动宣布为支持合同：正式范围
由固定 package 实际 call graph、逐 method inventory、Quick Action inventory 和 Cloudflare differential 共同确定。
managed 模式按固定客户端实际使用的 CDP 子集 qualification；context/target 映射和拒绝的 browser-global methods 必须
进入 capability/deviation matrix。外部 CDP 的版本与能力由目标决定，不能仅因连接成功就宣称完整 Cloudflare/CDP 兼容。

## 3. 三层协议，不混为一个 API

| 层                       | 调用方                           | 协议                                  | 是否公开               |
| ------------------------ | -------------------------------- | ------------------------------------- | ---------------------- |
| Cloudflare public API    | cf、SDK、用户 HTTP client  | `/client/v4/.../browser-rendering/**` | 是                     |
| Worker Browser binding   | `@cloudflare/puppeteer` / Worker | Fetcher + `/v1/**` HTTP/WebSocket     | 只对已绑定 Worker 可见 |
| Browser backend | `ocd` | managed process + scoped CDP / external native CDP | endpoint 不对 tenant 公开 |

Public API 中 JSON route 是否使用 v4 envelope 必须逐 route 固定，由当前 cf/官方 SDK transport 与真实响应共同资格化。
DevTools 的 raw JSON route 不能由 P6 的通用 `fetchResult()`/v4 envelope middleware 包装；image/pdf/body 与
WebSocket 保持原始媒体类型/upgrade。旧客户端的 response helper 不替代当前 producer/consumer 验证。

Worker binding 的 `/v1/**` 是固定 Cloudflare packages/Miniflare 可观察到的 service contract，但它不是 tenant 可直接
访问的 public management endpoint。Browser Runtime 只提供 `ocd` 内部的 process/CDP 能力，不能因此绕过 `ocd` 的
instance、binding 和 session scope。

## 4. cf 配置、Build Output 与 upload contract

### 4.1 `cloudflare.config.ts`

标准配置：

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig(({ mode }) => ({
  worker: {
    name: `browser-app-${mode}`,
    entrypoint: "./src/index.ts",
    compatibilityDate: "2026-09-03",
    workersDev: false,
    env: { BROWSER: bindings.browser() },
  },
}));
```

此例只展示当前官方配置形状，P22 尚未实施；不能据此宣称 Browser Run 已可部署。应用由官方 Vite 插件构建，
严格 TypeScript 检查在开发/CI 执行，`ocd cf` 只选择目标并透传 CLI，不解析配置或自行 bundling。

规则：

- binding 从当前 mode 的 `worker.env` 求值；mode/配置合并由 cf 拥有，不沿用旧 named environment inheritance 规则；
- server-side immutable state 只有 binding name；
- 当前 `bindings.browser()` 的 `dev` 选项只控制 local development，其实际 wire omission 由 BR0 固定，不进入 immutable Version；
- 不接受 endpoint、provider、browser、executable、args、headless、user_data_dir、team 或 user 等自定义 key；
- binding name 与所有其他 bindings 共用唯一性校验；
- 当前 instance 未配置可用 backend，或 P22 capability 未通过时，upload fail closed，不能删除 binding 后继续部署。

### 4.2 Multipart metadata

BR0 必须由固定 cf + 官方构建链捕获实际 multipart，并核对标准 binding wire：

```json
{
  "bindings": [{ "name": "BROWSER", "type": "browser" }]
}
```

descriptor 是 immutable Version state。backend 配置、executable identity、launch policy 和 session limits 属于 operator
runtime authority，不写进 tenant Version；每次 session 记录不含 secret 的 runtime contract digest，用于 fencing/reconciliation。

## 5. Worker binding contract

Miniflare 把 `browser` binding 组装成 service binding，固定 Cloudflare packages 把它当 Fetcher 使用。open-compute
沿用相同边界：

```text
tenant Worker + fixed Workers types / @cloudflare/puppeteer
  -> env.BROWSER.fetch() / env.BROWSER.quickAction() / WebSocket
  -> packages/runtime BrowserTransport
  -> ocd BrowserService
  -> instance-owned managed browser / configured external CDP
```

通过现有 workerd Fetcher 接口实现，也不在 tenant isolate 注入 Node/Chrome process handle。runtime facade 精确提供
`BrowserRun.fetch(input, init)` 和九个固定 `quickAction(action, options)` overload；后者转换为对应 Browser Run action
route 并原样返回标准 `Response`，不返回自定义 object。底层 `BrowserTransport` 只携带：

```text
instance_id, script_id, version_id, deployment_id,
binding_name, descriptor_sha256, capability_version
```

它不携带 engine endpoint/token/raw session ID。每次 fetch 都重新验证 immutable deployment snapshot 与 binding identity，
沿用现有 KV/D1/R2/Vectorize/AI Search 的 scoped transport pattern。

### 5.1 固定 binding route inventory

G0 先从固定 Puppeteer/Playwright 与 Miniflare source 抽取实际 call graph，目标包含：

```text
GET    /v1/acquire
GET    /v1/sessions
GET    /v1/limits
GET    /v1/history
GET    /v1/connectDevtools                 (WebSocket)
GET    /v1/devtools/session
GET    /v1/devtools/session/{session_id}
POST   /v1/devtools/browser
GET    /v1/devtools/browser/{session_id}
DELETE /v1/devtools/browser/{session_id}
GET    /v1/devtools/browser/{session_id}/json[/version|/list|/protocol]
PUT    /v1/devtools/browser/{session_id}/json/new
GET    /v1/devtools/browser/{session_id}/json/activate/{target_id}
GET    /v1/devtools/browser/{session_id}/json/close/{target_id}
GET    /v1/devtools/browser/{session_id}/page/{page_id}   (WebSocket)
```

route 只是 inventory seed，不是凭空承诺。HTTP method、query、headers、response JSON、legacy length-prefixed CDP framing、
native CDP WebSocket framing、session header 与 errors 由固定 packages/tests 锁定。

### 5.2 Session visibility

Cloudflare binding 的 `sessions()` / reconnect 可见范围由固定 package + Cloudflare differential 确认；平台 invariant 是
绝不跨 instance。Cloudflare `/client/v4` 继续使用 `account_id` wire 名称，内部 authority 与 transport 使用 `InstanceId`。
instance 内按 script/binding 的可见范围必须遵循官方合同，不能为了应用用户隔离改写标准 API。

每个 managed session 有独立 BrowserContext，但 instance 内的 session 发现/重连授权不因此变成 per-user。应用拥有
`user -> session` 映射和用户鉴权；彼此不可信的应用使用独立 instance。CDP attach 模式遵循目标的原生行为。

## 6. Public Browser Run API

### 6.1 DevTools 与 cf

2026-10-05 通过仓库固定 cf 的 `cli search`、命令 help 与 schema 核对以下入口：

```text
cf browser-run devtools browser create [--keep-alive <milliseconds>] [--lab] [--targets]
cf browser-run devtools session list [--limit <number>] [--offset <number>]
cf browser-run devtools browser live-view create <session-id> [--target-id <id>] [--expires-in-ms <milliseconds>]
cf browser-run devtools browser delete <session-id> [--force]
```

针对 open-compute 用 `ocd cf --instance <id> -- <command-after-cf>` 或显式 target 执行。
命令发现使用 `cf cli search`，请求详情使用 `cf schema <discovered-command>`；不猜测旧命令到新命令的对应关系。
`--keep-alive` 的单位为毫秒；Live View 是独立 POST，不把旧 view helper 的行为作为当前合同。当前 CLI 还暴露
recording、guardrails 等字段，只有进入 BR0 inventory 并通过资格检查后才能开放。以上仅确认 CLI 输入，未运行云端 mutation
或完成 P22 API/浏览器资格检查。

对应 route family：

```text
GET    /client/v4/accounts/{account_id}/browser-rendering/devtools/session
POST   /client/v4/accounts/{account_id}/browser-rendering/devtools/browser
GET    /client/v4/accounts/{account_id}/browser-rendering/devtools/browser/{session_id}/json
DELETE /client/v4/accounts/{account_id}/browser-rendering/devtools/browser/{session_id}
POST   /client/v4/accounts/{account_id}/browser-rendering/devtools/browser/{session_id}/live_view
```

以及固定 CDP target/version/protocol/new/activate/close/page routes。DevTools JSON 和 Live View response 的 envelope
须逐 route 固定；raw JSON/101 路径不套 v4 envelope，新增 Live View route 不从旧 helper 推断 response shape。
`devtoolsFrontendUrl` 必须指向 deployment-owned Live View/DevTools proxy，不能返回内部 browser endpoint 或 Cloudflare
`live.browser.run` origin。

`--lab`/WebMCP 是实验能力。Day 1 route 识别后明确拒绝 `lab=true`，除非固定 Browser Runtime、security review 与官方
differential 已单独通过；不能忽略该 flag 启动普通 browser。

### 6.2 Quick Actions

Day 1 public subset 按固定 OpenAPI 实现：

- content；
- screenshot；
- PDF；
- snapshot；
- scrape；
- links；
- markdown；
- accessibility tree；
- `/json` structured extraction 只有在 AI provider contract 通过后开放。

每个 operation 独立登记：request schema、navigation/options、response schema/media type、timeout、body/output bound、
engine capability 与错误。二进制 screenshot/PDF 直接 stream；HTML/text/JSON 是否套 envelope 不从其他 route 推断。

`crawl`、recording、WebMCP/lab、human-in-the-loop、持久 profile/import/export、任意 extension、tenant 自定义 executable
和未固定的新 beta endpoint 不在 Day 1。route/field 存在但未支持时返回明确 Cloudflare-style failure，不能忽略参数执行
一个语义更弱的 action。

`/json` 需要模型时复用 AI Search 的 operator-owned model catalog、secret reference、bounded request/response、timeout 和
stable AI provider error classes。tenant 不能在 Browser request 中提供 AI endpoint/key。模型不可用时 `/json` fail closed，
不影响不需要模型的 screenshot/content 等 operation。

## 7. Browser backend 与进程生命周期

### 7.1 安装与所有权

`ocd` 不嵌入浏览器 archive，不创建 browser.lock.json、解压缓存或自动下载路径。operator 显式准备兼容的
Chrome/Chromium 安装，推荐 `chrome-headless-shell`；只读 executable/resource 目录可由多个 instance 共用。
使用绝对 executable 路径；不能只复制 executable 丢掉 `.pak`、ICU 等相邻资源，也不能使用 operator 日常浏览器 profile。

managed 模式的新 generation 从明确的安装目录解析并验证 executable identity、版本、目标和所需资源，固定 launch policy；
使用 P17 已打开且验证的 executable 启动，防止检查后替换。记录可审计的 runtime contract digest；变化意味着新 generation，
不能把存活 session 透明换到另一 executable。支持版本/平台以 qualification matrix 为准，不声称任意 CDP 浏览器都等价。

cdp 模式只解析 configured HTTP(S) discovery 或 browser-level WS(S) endpoint，不接受 page-level endpoint；不猜测其他 URL，
不增加 provider registry、自动发现、安装或运行时 fallback。连接凭据使用现有 env/file secret references，不写入 tenant state。
该模式使用远端原生行为，远端 profile、进程 lifetime 和状态隔离由 operator/目标浏览器拥有。

### 7.2 instance、session 与 BrowserContext

```text
ocd
  -> instance_id
     -> managed browser process generation (0 or 1)
        -> session_id -> isolated ephemeral default BrowserContext
           -> pages / additional contexts owned by this session
```

- instance 没有 Browser Run 工作时没有浏览器进程；首次 admission 后启动，不随 instance autostart 预热。
- 一个实例的并发 acquire 合并为一次启动；各 caller 保留自己的排队 deadline 和 capacity permit。
- 每 session 创建空的 incognito context，默认页面创建被映射到该 context；context 只能属于一个 session/instance/generation。
- 同一 session 的页面按浏览器同源规则共享该 context 的登录态；不同 session 不共享 cookies、localStorage、IndexedDB、
  Cache Storage 或 Service Worker 注册状态。必须用实际页面和登录态 fixture 验证，不能只检查 context ID 不同。
- 用户需要多个互相隔离的登录态时使用多个 session，或在同一可信 session 内显式创建额外 context。
- temporary `user-data-dir` 按 process generation 分配；context 不是持久 profile。即使只是空白浏览器也会生成少量宿主文件，
  退出后清理整个已验证的 generation 临时目录，不保留到下次启动。
- 浏览器主进程崩溃会使该 instance 当前 generation 的所有 session lost；其他 instance 不受影响。
- 这里共享的是浏览器主进程，renderer/GPU/network 等仍由 Chromium 按其多进程架构创建，不使用 `--single-process`。

### 7.3 按需启动、空闲关闭与竞态

进程状态只需要以下本地生命周期，不建立跨实例浏览器池：

```text
stopped -> starting -> ready -> stopping -> stopped
                     ready = active or idle
starting/ready failure -> stopped + bounded retry backoff
```

1. **启动**：首次已授权、已取得 capacity 的 acquire 触发启动。静态 config validation 不启动浏览器。
   启动 deadline 内完成 private transport 建立和 CDP readiness probe；失败回收已创建的进程/目录、归还 permit。
   readiness 成功后关闭引擎自行创建的初始默认空白页面（如有），避免无任务的 renderer 常驻；分配 session context、
   登记 session，最后公布 opaque SessionId。页面只在用户或 action 请求时创建，不为每个空 context 预热 renderer。
2. **执行**：live session、pending context allocation 和 in-flight action 分别计数；每个 action/command 有 bounded deadline。
   session 的 keep-alive 与进程的 warm-idle timer 是两个不同计时器。
3. **session 回收**：显式 close、标准 session idle/keep-alive 到期或不可恢复错误触发 context dispose，关闭其所有页面和
   session 自行创建的额外 context，然后归还 session permit。客户端 WebSocket disconnect 仅释放连接，不立即销毁仍在
   标准 keep-alive 内的 session。没有命令的空连接也会按标准 idle semantics 到期，不能永久占用浏览器。
4. **warm idle**：仅当 live sessions、pending acquire/context allocation、in-flight action 都为零时，记录单调时钟
   `idle_since` 并开始 `browser_idle_timeout_ms`。下一次合法 acquire 取消尚未提交的 idle stop，复用现有主进程、创建全新
   context；不能复用已销毁 context 的登录态。推荐初始配置 **30 秒**，是待正式宿主测量后确认的运维起点，不是隐藏默认值。
5. **退出**：idle deadline 到达后，在同一 instance lifecycle owner 中重新检查 generation、计数和 idle epoch，再原子进入
   stopping。物理 `Browser.close` 只由 manager 对整个已空闲进程发送；在 grace deadline 后执行身份已验证的 process-group
   TERM/KILL，完整 reap、释放 FD/容量、清理当前 generation 临时目录，最终进入 stopped。
6. **与新请求竞争**：进入 stopping 前可以取消 idle stop；进入 stopping 后新 acquire 排队等待 reap，再触发新 generation。
   不允许连到即将退出的进程，也不同时启动该 instance 的两个浏览器。caller 的原始 acquire deadline 包含这段等待。
7. **故障**：active browser crash 标记整个 generation 的 session lost、关闭其连接并回收；下次合法请求在 bounded backoff
   后重新启动，不自动恢复登录态，也不无请求地循环重启。只关闭平台健康探针失败不能直接等同于 browser crash。
8. **instance/daemon stop**：停止接纳新 acquire，显式关闭所属 session，在 bounded grace 内停止并 reap 所属进程组。
   restart 按 P17 验证 executable/start identity/ancestry 后收敛 orphan；旧代次 token/session 不再可用。

用于 session idle 的活动信号必须按固定 Cloudflare trace qualification。正常用户 CDP command/action 才是候选有效活动；
`/health`、status/list、后台 readiness probe、WebSocket ping 和页面后台 timer 不能刷新进程 warm-idle timer。
长 navigation 的 in-flight lease 由 command/action deadline 保护，不能在执行中因为没有新命令就回收 context。
所有计时使用单调时钟；SQLite wall-clock timestamps 只用于审计。停止与 allocation 的决定串行化，I/O 不放入 SQLite transaction。

示例：一个 session 最后一次有效活动后，在标准约 60 秒 idle（实际规则由 BR0 冻结）到期时销毁 context；如无其他工作，
再经过配置的 30 秒 warm idle 退出进程。没有存活 session 的 Quick Action 完成后直接进入 warm idle，不额外等 session timeout。
进程 timeout 不截断仍合法存活的 session；要释放被遗忘 session 的资源，先执行 session 自己的 idle/keep-alive contract。

### 7.4 Operator config

以下为待实施配置形状，全部容量与 timeout 必须显式填写并验证为非零、有上限；示例值是推荐起点，不是正式默认值。
配置属于 instance，不能写入 `cloudflare.config.ts` 或 tenant request。两种 backend 互斥，`deny_unknown_fields` 拒绝混配。

```toml
[browser]
max_sessions = 8
max_pending_acquires = 16
acquire_timeout_ms = 10000
command_timeout_ms = 30000

[browser.backend]
kind = "managed"
executable = "/opt/chrome-headless-shell/chrome-headless-shell"
browser_idle_timeout_ms = 30000
shutdown_grace_ms = 3000
```

外部接入替换整个 backend block，不接受 managed 生命周期字段：

```toml
[browser.backend]
kind = "cdp"
url = "http://127.0.0.1:9222"
```

外部 endpoint 如需认证，使用现有 validated secret-reference 类型；credential 不进入 URL 日志、argv、status 或 API response。
managed 的 launch flags、sandbox、private transport 和 temporary directory 由平台固定，不开放 arbitrary args。
`max_sessions = 8` 仅演示字段形状，不是每实例内存安全保证；实际预算应按真实网页/renderer 测量选择。

### 7.5 Runtime contract、transport 与监督

`ResolvedBrowserRuntimeContract` 记录 backend kind、目标/版本、managed executable/resource identity、launch/sandbox policy、
固定 binding protocol 与 CDP method inventory，不含 secret。managed process 记录 instance_id、generation、process identity、
lease 和临时目录 owner。session 记录相同 generation 与 contract digest。外部 CDP 不建立本机进程 lease。

managed 优先使用 `--remote-debugging-pipe` 的私有 FD，不暴露可由 Worker loopback outbound 绕过认证的裸 Chrome 端口；
本次 Node 探针验证了 Chrome 的 fd 3/4 pipe 行为，但 P17 当前只提供 fd 0 control mapping，Rust FD ownership/mapping 和
restart/orphan 行为仍需 BR1 实现与验证。不能用 loopback-only 或 URL 随机串宣称 Chrome endpoint 已认证。

BrowserManager 拥有 readiness、session/CDP scope、capacity、deadline、warm-idle 与 stop 决策；P17 只拥有已验证的 process
spawn/group/stdout/stderr/graceful stop/forced stop/reap/orphan primitives。session 关闭不调用物理 Browser.close；实际浏览器
退出由 manager 在 generation 级执行。backend 变化后旧 session 不能透明迁移；不得保留旧内嵌 runtime 或 fallback 分支。

### 7.6 2026-10-05 本机资源探针

这是一次性本地能力/资源测量，**不是 P22 产品 Gate，也不是 Linux 或正式发布性能保证**。使用本机已有 Playwright cache
中的 `chrome-headless-shell`，无下载；Apple M2 Max、32 GiB、macOS 27.0.1 ARM64、Node 24.21.0。
版本 `153.0.8010.12`，CDP revision `971a7443b0c9b0a9b2860529b33331b76077ec62`，executable SHA-256：
`a0bfe7b4da4787b66058477d696cd1d09065d25f06a548947722b9af77ee8282`。

五次 fresh process 使用各自新的 private HOME/TMPDIR/user-data-dir，保留 sandbox、原生 CDP pipe、初始 `about:blank`；
参数额外包括 `--disable-background-networking --no-first-run --no-default-browser-check`。未清空 OS 文件缓存，因此第一条是
本次首次启动，后续是 OS cache 可能已热的 fresh process；不称为五次真正冷启动。CDP readiness 使用 Browser.getVersion，
页面 readiness 使用 attach + Runtime.evaluate 验证 document.readyState 完成，不把 spawn 返回视为就绪。

| 指标 | 本机结果 |
| --- | --- |
| CDP ready 中位数 / 范围 | 130 ms / 89–326 ms |
| 空白页面可执行脚本中位数 / 范围 | 153 ms / 114–389 ms |
| 空白主进程稳定 RSS | 约 80 MiB |
| 空白进程组稳定 RSS（4 个进程） | 约 242 MiB |
| 关闭初始空白页面后的无页面进程组 RSS（3 个进程，补充探针） | 171 MiB |
| 无页面的 1 / 5 / 10 个临时 context，进程组 RSS（补充探针） | 172 / 174 / 176 MiB |
| 额外 1 / 5 / 10 context 各带一个空白页面，进程组 RSS | 315 / 606 / 970 MiB（保留初始空白页面） |
| context 创建耗时（第 1 / 5 / 10 个抽样） | 1.28 / 0.79 / 0.52 ms |
| 同时创建 context 与空白页面可执行脚本耗时（对应抽样） | 56 / 44 / 32 ms |
| 销毁全部额外 context 后 2 秒，进程组 RSS | 263 MiB，仍高于空白初始基线 |
| Browser.close 至确认退出且无残留进程，5 次样本 | 43–104 ms |
| 空白临时 user-data-dir 的文件逻辑大小 | 约 1.71 MiB，退出后仍存在，须由 manager 清理 |

RSS 是 `/bin/ps` 对该浏览器 process group 所有进程的 resident set 加总，会重复计算共享页，**不是实际增量物理内存/PSS**。
额外 context + page 样本每次包含新的 renderer；不能把约 73 MiB 的 RSS 增量当作纯 context 的成本。补充探针关闭初始
页面后创建十个无页面 context，RSS 总增量约 5.2 MiB；该单样本也不是固定每 context 的承诺。所有七个测量浏览器
正常退出并确认 process group 零残留；synthetic cookies 在两个不同 context 中分别可见/不可见。未测真实站点导航、长时间
内存增长、localStorage/IndexedDB/service worker 或完整 Puppeteer/Playwright 客户端隔离。

原始命令、严格 TS7 检查的 disposable probe、各 generation stderr/profile 和 JSON 保存于
`.temp/browser-lifecycle/measure.ts`、`.temp/browser-lifecycle/2026-10-05T06-56-04.966Z/` 与补充探针
`.temp/browser-lifecycle/2026-10-05T06-58-43.026Z/`；临时测量目录作为证据保留，
不等于 production retention policy。正式宿主仍需测 acquire queue、FD、实际 RSS/physical footprint、网页负载、持续
context churn、idle exit/restart 和多 instance 工作集，随后调整 explicit capacity 与 idle timeout。

## 8. Session model、lease 与 reconciliation

managed SQLite authority 建议字段：

```text
browser_sessions
  id, instance_id, visibility_scope,
  runtime_contract_sha256, runtime_generation,
  engine_context_locator_ciphertext,
  state, keep_alive_ms, created_at, connected_at,
  last_activity_at, closing_at, closed_at, lost_at, close_reason
```

public session ID 是 opaque ID；raw context/target locator、endpoint/token、临时目录、process identity 不返回。
managed session 与 instance browser generation 关联，不再每 session 存一个独立 process lease。额外 context/target 的
ownership 必须由 manager 追踪并在 CDP 边界核验；host memory 是执行索引，不能覆盖 instance/session 的 SQLite authority。
外部 CDP 的 engine locator 单独受保护，不假造本机 process/profile identity。

```text
acquiring -> ready -> connected -> ready
ready/connected -> closing -> closed
acquiring/ready/connected/closing -> lost
```

connected 是是否有连接的展示状态，不把每次 disconnect 等同于整个 session 结束；多个并发连接按固定客户端合同计数。

规则：

- session admission permit 从 acquire 到 closed/lost 持有；浏览器进程 permit 从 starting 到完整 reap 持有，两者独立。
- pending acquire 有 bounded queue 和 deadline；启动、context allocation、stopping 等待都计入 acquire deadline。
- public ID 只在 browser readiness、context allocation 和 SQLite 登记成功后发布；分配失败只清理该次 context/permit，
  不关闭其他 session 正在使用的共享浏览器。context cleanup 无法确认时 fence 该 generation，不能把可能脏的环境分配给新 session。
- reconnect 只在所属 instance、visibility scope、存活 generation 和标准 keep-alive 内成功，不复活已关闭 context。
- close 必须幂等且只回收该 session 所有 context/target；unknown/closing/closed response 按 fixed route qualification。
- 所有 session 回收后才启动 process warm-idle timer；deadline 前新 session 总是全新临时 context。
- managed browser crash、instance stop 或 daemon restart 使关联 session lost、连接失效；不恢复 profile 或旧 CDP locator。
- external CDP 在本地 daemon restart 后废弃本地 transport generation，重新认证连接配置目标；不 signal、不猜测或回收远端进程。
- orphan signaling 仅限已验证 start identity、executable digest 和 process ancestry 的 managed browser process group。
  临时目录 cleanup 校验实例归属、代次、owner、no-follow 与 containment，不能使用 tenant 提供的路径或误删 operator 安装目录。

## 9. HTTP/CDP/WebSocket proxy

所有 public/binding traffic 经 `ocd`：

- HTTP request/response streaming 有 size/deadline/backpressure；
- WebSocket upgrade 前完成 instance/binding/session/target authorization；
- `Origin`、Authorization、Cookie、forwarded headers 与 runtime-internal token 分开处理；
- browser WebSocket URL/token 只在进程内构造，redirect/response body 不能把它泄露给 client；
- text/binary frame、fragmentation、ping/pong、close code/reason 与 half-close 按固定 CDP behavior 转发；
- per-connection message/frame/aggregate bytes 和 outbound queue 有 operator guard；
- client disconnect 释放 connection lease，但是否关闭 browser session 取决于标准 session contract；
- managed 必须按 session 隔离 CDP context/target，不能原样共享 browser-level endpoint：default Target.createTarget 映射到
  session context；context/target list 和事件只暴露所属 session；attach/get/close/storage/download 等不能越过所属 context。
- managed 的客户端 Browser.close 关闭该平台 session；物理 Browser.close 只由 manager 退出整个 browser generation 时使用。
  客户端额外创建的 context 也归属当前 session；Browser.close/default context/getTargets 等必须由固定客户端逐项验证。
- 映射只覆盖 qualified inventory，不写一个通用 CDP virtualizer；无法安全映射的 browser-global method 显式 unsupported，
  不静默透传。unknown command/event、flattened CDP session ID、子 target/OOPIF、跨 context storage 和 privileged browser attach
  进入同一边界验证；未完成这些资格检查不能宣称多个独立 Browser Run session 已兼容。
- external CDP 使用目标的 native CDP 行为，保留显式 Browser.close 等命令语义，不套用 managed 的 context/close 改写；
- legacy `/v1/connectDevtools` 的 length-prefix framing 与 native page WebSocket 分开测试。

cf DevTools/Live View response 所需 frontend URL（包括合同中的 `devtoolsFrontendUrl`）指向 `ocd` 自带的静态 DevTools frontend/proxy route 或可验证的
deployment-owned frontend。P22 不在启动时从公网下载 DevTools UI。若不能合法、可复现地随 release 提供兼容 frontend，
DevTools/Live View Gate 不通过，不能只返回 browser internal URL。

## 10. Quick Action 执行

Quick Action 采用统一 pipeline：

```text
authenticate -> validate fixed schema -> capacity admission
  -> acquire/reuse isolated engine session
  -> navigate/action under deadline
  -> bounded/streamed result validation
  -> close/release according to official contract
```

约束：

- URL、redirect、subresource、download、WebSocket 和 DNS 都发生在受控 Browser Runtime egress 边界；
- `ocd` 只把 validated action 发送到正式固定的 engine session，绝不直接 fetch tenant URL 代替浏览器；
- response body、DOM、screenshot/PDF、AI prompt/result 默认不进入 logs；
- screenshot dimensions/format、PDF options、selectors、wait conditions、headers/cookies、navigation timeout 逐字段 allowlist；
- unsupported field fail closed，不把 `waitForSelector` 等参数静默丢掉；
- output 不自动写入 R2/Artifacts/团队目录。Worker/调用方要持久化时显式调用对应 binding，保持权限和失败边界清晰；
- `/json` 的 AI call 与 browser session 共用 end-to-end deadline，不能无限等待 engine；
- engine quick-action capability 不足时对应 route `unsupported`，不通过执行自定义脚本模拟半套 semantics。

## 11. Isolation 与 security

managed Browser Run 执行不可信网页，外部安装不降低它作为解析器和主动网络客户端的边界。最低 invariant：

- 不同 instance 使用独立 browser process group/generation；同一 instance 的不同 session 使用独立临时 context，
  cookie/site storage 不跨 session 复用；context 是网站状态边界，CDP authorization 必须由第 9 节单独保证。
- managed browser filesystem 权限只允许 operator 的只读安装资源和本 generation 的临时 subtree，不能读取 `ocd` control
  SQLite、master key、secret files、control socket、artifact cache 或 workerd runtime files；
- executable 由 operator 绝对路径配置；launch flags/environment 由 runtime contract 固定。配置不能传 `--no-sandbox`、
  extension、proxy、user-data-dir、remote-debugging address 或任意 V8/browser flags；
- production 禁止以 `--no-sandbox` 或等价选项回退；目标宿主无法建立已验证 sandbox 时 Browser Run unavailable；
- managed 原生 CDP 走第 7.5 节私有 pipe，平台 transport 使用 generation fencing/认证；不把原生 Chrome 端口当作有 token 的服务；
- instance/session/target auth 在每次 HTTP 和 WebSocket upgrade 执行，不能只在 create 时检查；
- session ID 高熵且不可枚举；list/get/close 的 not-found/forbidden 不泄露跨 instance presence；
- URL、redirect、DNS、subresource、WebSocket 和 download 的 address-level egress 必须拒绝 private、loopback、link-local、
  metadata、Unix、IPv4-mapped private IPv6、DNS-to-private 和所有 platform listener；不能只依赖 hostname 检查、browser flag
  或候选引擎声称的 SSRF protection；无法在目标平台强制执行时 Browser Run unavailable；
- downloads、file chooser、clipboard、camera/mic、printing、WebUSB/WebBluetooth 与本地 filesystem 默认禁用；
- browser crash、renderer hang、CDP flood、zip bomb/download、巨大 DOM/canvas 都受 engine process/`ocd` 双层 limit；
- P7 logs 只记录 stable metadata/error class，清洗 URL query、headers、cookies、DOM、CDP payload 和 screenshots。

## 12. Limits 与 backpressure

不复制 Cloudflare plan 的并发/session/browser-minutes 数值，也不设置 LynxOS 20 人默认值。operator capacity 至少包括：

- active sessions、pending acquires、sessions per instance，以及 daemon 同时存活的 managed browser process groups；
- acquire/command/navigation deadline、标准 session idle/keep-alive、独立 process warm-idle timeout 与 shutdown grace；
- concurrent Quick Actions 和 CDP connections；
- HTTP body、result、WebSocket frame/message/queue bytes；
- screenshot/PDF dimensions/bytes、DOM/text/JSON output；
- engine request in-flight、spawn/close/reap concurrency；
- session history/metadata retention。

固定 Browser binding 的 `/v1/limits` 返回值要反映 effective deployment capacity，但字段/单位必须与官方 package
一致。它是 deployment capability，不伪装成 Cloudflare plan。P9 另外统计 Worker invocation subrequest/CPU；browser
session permit 不能因 Worker request 结束就漏归还或被错误释放。

## 13. Miniflare 参考边界

采用的 Miniflare 证据：

- `browser` binding 被组装为 service binding；
- `/v1/acquire`、sessions、limits、history 与 DevTools route shape；
- Durable Object 风格的 session identity/lifecycle；
- Chrome HTTP/CDP 与 WebSocket proxy，包括 target JSON；
- 当前 cf binding `dev.remote` 的开发期语义（BR0 固定对应 Miniflare 输入）。

明确不复制：

- 启动时自动下载 Chrome；
- Node `child_process` browser launcher；
- in-memory Durable Object/session authority；
- hard-coded concurrency `6` 或任何 Cloudflare plan number；
- Miniflare loopback `/browser/launch|status|close|sessionIds` 作为 public API；
- dev-only retry/auth/error messages；
- 把一个开发机 Chrome 当作 multi-account production isolation。

`cf dev` 的本地体验由上游 cf、官方 Vite 插件 v2 与 Miniflare 负责；P22 qualification 针对真实 `ocd` + pinned workerd +
显式 operator 安装的 managed browser，外部 CDP native attach 的 qualification 单独记录。

## 14. Error 与 observability contract

稳定 Browser Runtime error classes：

```text
invalid_request, unsupported, capacity_exhausted, acquire_timeout,
runtime_corrupt, runtime_unavailable, browser_launch_failed, browser_crashed,
session_not_found, session_lost, target_not_found,
cdp_unavailable, navigation_failed, action_timeout, malformed_response
```

Public code/message/status 由固定 Cloudflare route fixture mapping；内部 class 不直接作为 vendor error body。retryability、
`Retry-After` 与 close code 有明确表，不能根据 browser stderr/message regex 猜测。

推荐 metrics/log dimensions：

```text
instance_id, script_id, operation, backend_kind,
runtime_contract_sha256, runtime_generation, result_class, session_state,
queue_wait_ms, acquire_ms, duration_ms, bytes_in, bytes_out,
process_state, process_start_ms, process_stop_ms, idle_stop_count, active_contexts
```

session/target ID 只记录 bounded/digest form；URL host 仅在 operator 明确允许的低基数审计日志中出现，不作默认 metrics label。
P7 realtime tail 可以显示 Worker 触发的 browser call outcome，但不能带 DOM、CDP message、cookie、header、AI content 或
engine/process/internal endpoint detail。

## 15. 实施顺序

### BR0：冻结合同

- 固定 cf/config、mode、Build Output、commands/upload metadata；
- 固定 Puppeteer/Playwright/types package versions、integrity 与 Browser binding call graph；
- 固定 public OpenAPI、raw DevTools/WebSocket traces、Quick Action schemas；
- 建 route/field/media/frame/error/capability inventory；
- 记录 Browser Rendering -> Browser Run 只改产品名、不改兼容 path 的规则。

### BR-G0：end-to-end feasibility Gate

- 使用显式准备的 `chrome-headless-shell`，验证 Darwin ARM64、Linux GNU ARM64/x64 的版本、资源、sandbox 与 private pipe。
- 真实 ocd + pinned workerd 中固定 Puppeteer/Playwright 的 launch/connect/default newPage/额外 context/close 通过；
  shared-process 的 managed session 映射、target/event/storage 权限与官方行为逐项 differential。
- Quick Actions、页面渲染、screenshot/PDF/font/navigation 和错误 inventory 逐项 qualification。
- 测量空白和真实网页工作集、startup/acquire、context churn、idle exit/restart、stop/reap 与多 instance 隔离。
- external CDP 只资格化 native attach/relay；不假造 managed 生命周期或远端隔离证明。
- 明确 method/capability/deviation matrix 与安装要求，不以本机资源探针代替产品 Gate。

Exit：managed session 隔离、固定客户端和声明安全/协议范围通过后才进入正式实施；有缺口则相应 capability unsupported，
不能退回 embedded runtime、每 session process 或另一个引擎的隐藏 fallback。

### BR1：operator backend 与 instance browser supervisor

- 实施 mutually exclusive managed/cdp config，executable/resource/version validation、runtime contract digest 与 secret references。
- 基于 P17 实施私有 CDP fd 3/4 ownership/mapping；按需启动、single-start、readiness 与 bounded failure cleanup。
- session/context/action 计数、process warm-idle timer、idle epoch/generation fence、stopping admission 与 next-generation restart。
- graceful close、validated TERM/KILL、完整 reap、temporary generation directory cleanup 与 orphan recovery。
- health/status/metrics 与 sanitized error classes；只对 live instance 的有需求浏览器进行监督，不启动永驻浏览器池。

### BR2：session authority 与 runtime binding

- session schema/state/lease/admission、restart `lost`、orphan reap 与 contract fencing；
- P6 multipart decode、immutable Version descriptor、settings/download/rollback；
- `packages/runtime` BrowserTransport Fetcher；
- fixed `/v1/**` HTTP and legacy/native WebSocket behavior；
- instance/session visibility differential（Cloudflare account wire 保留）。

### BR3：DevTools 与 cf

- raw public DevTools routes、session/target APIs；
- CDP WebSocket proxy；
- deployment-owned `devtoolsFrontendUrl` / Live View；
- fixed cf session list、browser create/delete 与 Live View subprocess Gate。

### BR4：Quick Actions

- content/screenshot/PDF/snapshot/scrape/links/markdown/accessibility tree；
- per-route schema/media/streaming/error limits；
- `/json` 与 operator AI provider 集成；
- unsupported beta/experimental route/field fail closed。

### BR5：isolation、limits 与 operations

- browser sandbox/egress/temporary-context/instance-process isolation contract；
- P9 accounting、P7 logs/tail、metrics/readiness；
- overload/browser crash/CDP flood/restart/upgrade/soak；
- deploy/runbook/backup（metadata only）/incident cleanup。

### BR6：qualification

- fixed cf、Puppeteer、Playwright、official SDK subprocess/in-runtime matrix；
- public API JSON/raw/binary/WebSocket differential；
- Cloudflare remote differential 或独立 credential-blocked acceptance；
- P6/reference/capability/deviation/examples/Dashboard 同步。

## 16. 必测矩阵

| case                                         | 预期                                                                            |
| -------------------------------------------- | ------------------------------------------------------------------------------- |
| `cloudflare.config.ts` Browser binding                     | multipart 精确 `{name,type:"browser"}`                                          |
| local-only binding `dev`                          | 不进入 Version state                                                            |
| unsupported backend/runtime contract mismatch | upload/API fail closed，无 PATH/download/fallback |
| managed executable/resource identity | 绝对路径、版本、资源与 runtime contract 匹配，不内嵌浏览器 |
| first Browser Run use | 仅所属 instance 启动一个浏览器；无调用的 instance 无浏览器进程 |
| concurrent first acquires | single-start，独立 context/permit/deadline，无双主进程 |

| fixed Puppeteer launch/close                 | 正式 pinned workerd 中成功，无 custom package                                         |
| fixed Puppeteer reconnect/sessions           | visibility、IDs、errors 与固定 authority 一致                                   |
| fixed Playwright supported flow              | 同一 Browser Fetcher contract 通过                                              |
| cf session/browser/Live View commands              | raw JSON、target、URL、exit code 与 fixed CLI 一致                              |
| `lab=true` 未支持                            | 明确拒绝，不静默降级                                                            |
| public screenshot/PDF                        | 正确 media type/bytes/streaming，无 JSON 包装错误                               |
| content/markdown/links/a11y                  | schema、encoding、bounds 与 fixed API 一致                                      |
| `/json` without AI provider                  | 明确 unavailable；其他 action 不受影响                                          |
| cross-account session ID                     | list/get/connect/close 全拒绝且不泄露存在性                                     |
| raw CDP URL/token/profile/process ID         | response/log/error/Worker env 均不可见                                          |
| WebSocket text/binary/fragment/ping/close    | 无破坏转发，bounded queue                                                       |
| slow/aborted client                          | backpressure/cancel 生效，无 leaked connection/session permit                   |
| browser crash/hang | 本 instance generation 全部 session lost；其他 instance 不受影响，bounded reap/backoff |
| `ocd` restart with live sessions             | 旧 generation/token 失效，session `lost`；验证后清理 orphan，不猜测 endpoint    |
| max sessions/pending queue                   | stable capacity error/Retry-After，无无限排队                                   |
| huge frame/result/DOM/canvas                 | 两层 limits 生效，服务保持可用                                                  |
| private/network metadata navigation          | 顶层/redirect/DNS/subresource/WebSocket/download 均由 address-level egress 拒绝 |
| production sandbox unavailable               | Browser Run unavailable；不增加 `--no-sandbox` fallback                         |
| runtime revision/policy change               | contract fencing；旧 session 不透明迁移                                         |
| managed cookie/site-storage isolation | 两 session 的 cookies/localStorage/IndexedDB/Cache Storage/service workers 不串状态 |
| managed target/context/events/storage | list/attach/close/事件/flattened sessions 不越界；未知全局方法明确拒绝 |
| session close vs process close | 仅销毁所属 contexts；其他 session 可继续执行 |
| session idle / disconnect / in-flight action | 符合固定 keep-alive，空连接不永久保活，不中断 deadline 内的执行 |
| warm-idle timeout | 所有 session/action/acquire 清空后按 explicit timeout 完整 stop/reap，释放内存和 FD |
| request before/after idle stop commit | commit 前取消，commit 后等待 reap 再启动，无错误连接/双主进程 |
| stale idle callback / generation | 不能关闭新代次或仍有工作的 browser |
| context churn and idle restart | 无 context/permit leak；新 generation 无旧登录态；temporary directory 正确清理 |
| native external CDP | 原生命令语义，local idle timer 不关闭外部浏览器，无本机 process lease |

## 17. Definition of Done

P22 只有同时满足以下条件才可归档：

- 固定 cf 的 mode/config、Build Output/upload、session/browser/Live View 命令对真实 `ocd` 通过；
- 固定 `@cloudflare/puppeteer` 与声明支持的 `@cloudflare/playwright` API 在 正式 pinned workerd 中通过，无 Browser 专用 fork patch/custom client；
- public Browser Run Quick Actions、raw DevTools JSON、binary body 与 CDP WebSocket 按逐 route fixture 通过；
- `ocd` 是平台唯一公开入口，instance/binding/session/target authorization 与内部 browser/CDP identity 分离；
- managed 每 instance 一个按需 browser process group，每 session 独立临时 context；CDP scope 映射已通过固定客户端与安全资格检查；
- operator 安装的 browser executable/resources/version/capabilities 有支持矩阵，生产无 embedded browser、PATH discovery 或下载；
- 两种 backend 配置互斥，external CDP 使用原生行为，不受 managed 空闲退出控制；
- 按需 single-start、session deadline、warm-idle stop、stop/admission race、完整 reap 与 temporary directory cleanup 通过；
- session lease、overload、browser crash/hang、`ocd` restart、orphan cleanup、contract change 与 soak 通过；
- production sandbox 与 public-only address-level egress 在全部正式目标强制执行，无 `--no-sandbox` 或 hostname-only fallback；
- capacity 全部是 operator config/capability，不复制 Cloudflare plan 或 LynxOS 20 人默认值；
- Miniflare 只作为固定行为/开发参考，production 没有 in-memory/hard-coded/download fallback；
- P7/P9 与 AI provider（仅 `/json`）集成的 supported/planned 状态准确；
- Cloudflare differential 完成，或 credential 限制拆成独立 active acceptance；
- P6、reference、capability manifest、examples、runbook 与 Dashboard 同步。

文档变更本身只运行 `git diff --check`、链接和固定命令/源码核对。实现属于 protocol、runtime、process、WebSocket、security、
persistence 与 release 变更，必须执行仓库 `AGENTS.md` 要求的 focused tests、coverage 与最终 workspace Gate。
