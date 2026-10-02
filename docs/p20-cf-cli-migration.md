# P20：Cloudflare CLI 单轨迁移与官方应用构建链

状态：**planned / Day1 设计确定（2026-10-02）**。本文规定实现后的唯一工作流，不表示 `ocd cf` 已存在或已通过真实部署验收。本次为文档变更，不修改运行代码、依赖 pin 或已发布数据。

## 1. 结论与范围

open-compute 不再维护 Wrangler 项目工作流，也不再维护通用 TS Worker 编译器。应用配置、迁移、类型生成、构建与 Cloudflare API 客户端尽量采用官方实现；`ocd` 只增加自托管目标选择所必需的薄边界。

最终只有一条正式路径：

```text
cloudflare.config.ts + 官方构建工具配置
  -> 官方 cf / 构建工具：生成类型、编译模块、准备 assets
  -> Cloudflare Build Output
  -> 官方 cf：上传模块、管理资源、创建 Version / Deployment
       ^
       | ocd cf 仅选择 instance/target、注入凭据、执行项目内 cf
       v
  -> OCD /client/v4：认证、协议校验、现有产品 authority
  -> 内部 CanonicalBundle / immutable Version / workerd
```

关键决定：

1. 将 `ocd wrangler` **替换**为 `ocd cf`；不并存、不留别名、不做命令翻译或 fallback，不建立双 CLI 抽象。
2. JS/TS 应用默认且正式认证的构建链为 **cf + Cloudflare Vite 插件 v2**。删除自有 `oc build` 与标准部分的 `oc types`，不把它们改成无意义的官方命令 wrapper。
3. 旧项目由用户显式运行官方 `cf migrate --bundler vite`。OCD 只检测并提示，不编写迁移器、不自动修改项目。
4. 服务端仍以官方 Cloudflare API wire contract 为边界；不解析 TypeScript 项目配置，不接管 bundler，不新增私有 Build Output 上传协议。
5. 同一次实现变更更新现有调用方、测试、示例、CI 和活动文档，并删除被替代实现。实现可以分步骤完成，但发布结果不得含两条正式应用工具链。

这里的“只支持 cf”是开发与认证入口的选择，不是按 HTTP User-Agent 封禁 Wrangler。官方 SDK、其他合规 API 客户端仍可调用已声明的 API；为官方协议所需的字段也不能仅因名字与 Wrangler 有关而删除。

本阶段不改变 daemon/instance 模型、显式 `compute.toml` 的 data 目录、`OCD_DIR`、Caddy、资源存储或 workerd 监督模型；不引入新的安装器、target registry、认证体系或构建服务。

## 2. 官方依据与待认证基线

设计核对日期为 2026-10-02。当前查询到的已发布 cf 为 `1.0.0-beta.11`，作为首个待认证候选；**不是已认证版本**。cf、配置和 Build Output 仍处于 beta，实施时必须固定实际安装的发布版本与 lockfile，不能把 `main` 上的代码当成发布证据。[S1] [S2]

官方事实与本阶段采用方式：

| 事实 | 本阶段决定 |
| --- | --- |
| 官方 `cf migrate` 使用 `@cloudflare/codemods`；支持 JSON/JSONC/TOML，可能需要人工完成转换 | 直接提示官方命令，不复制 codemod [S3] [S4] |
| cf 委托框架或官方构建工具；Vite 插件 v2 不依赖 Wrangler | 正式 JS/TS 路径选择官方 Vite 插件，不自己接 Rolldown/esbuild [S5] [S6] |
| cf 使用 Node.js 22.18+；Bun 运行 cf 配置加载不受支持 | Bun 继续管理本仓库依赖；cf 由 Node 执行 [S7] |
| cf 支持标准 API base URL、account、token 环境变量 | 复用现有 target/instance 机制并注入这些值 [S8] |
| `beta.4` 已发布 Worker secret update/bulk，`beta.7` 修复 redeploy 保留 secrets | 不把单 secret 写入当成保留 Wrangler 的理由 [S2] |
| 官方映射页仍列出实时 Worker tail 缺口，且其 secret 描述落后于 changelog | 以固定版本 help/schema、发布源码和实际测试为准；日志处理见第 10 节 [S2] [S9] |

复用现有上游刷新与认证流程，记录 cf、Vite、插件、类型工具和相关 runtime 的准确版本。认证基线与用户项目 pin 是两个概念：capabilities 公布测试基线，不要求每个客户端精确相等；解析标准 SemVer（含 prerelease），版本差异只做诊断，安全/协议能力由真实约束决定。不得维护多个版本专用 adapter。

## 3. 责任边界与复杂度预算

| 所有者 | 负责 | 不负责 |
| --- | --- | --- |
| 用户项目 + 官方工具 | 配置求值、mode、类型生成、编译、模块解析、框架 adapter、assets、Build Output、API 请求与上游交互 | OCD instance 管理 |
| `ocd cf` | 项目目录、目标解析、项目内 cf 定位、有限预检、环境注入、进程透传 | 解析 bindings、执行 codemod、构建、产物改写、资源 provisioning、输出翻译 |
| OCD API / 产品层 | 认证授权、有限输入校验、已支持资源及生命周期、内部版本与运行时边界 | 读取用户源文件、安装依赖、替代官方 CLI、代理未支持产品到 Cloudflare |
| 项目 CI | 显式安装、严格类型检查、构建、测试、带凭据部署 | 依赖 wrapper 暗中运行项目脚本 |

不 fork cf，不 monkey-patch 官方包，不增加 `ocd build/types/migrate/deploy/secret/...` 平行命令族，不创建通用 bundler 插件框架。官方 CLI 缺功能时，优先修复上游或明确限制；不得用扩大 OCD wrapper 来掩盖缺口。

## 4. `ocd cf`：薄目标 launcher

### 4.1 命令与目标

目标接口沿用现有语义：`--target`、`--instance`、`--config` 互斥；未显式选择时沿用既有本机唯一目标解析规则。`--config` 指 OCD 配置，不是应用配置；`--project` 只设置应用工作目录。示例为实现后的用法：

```sh
ocd cf --project examples/hello-worker --target staging deploy --mode staging
ocd cf --target production deploy --prebuilt --mode production
ocd cf --instance dev d1 list
```

`ocd` 的选择器位于 cf 子命令之前。cf 子命令开始后的 argv 保持原始字节和顺序，不重新拼接 shell、不把 `--env` 转换成 `--mode`、不把 target 名称隐式当作 mode。`--worker` 等项目选项由 cf 自己解释。

保留现有 target registry、token 文件、HTTPS/loopback 约束和本机 descriptor 校验。capabilities 中将 Wrangler 认证字段直接替换为 cf 认证字段，同步更新所有生产者和消费者，不保留旧字段或双 schema。

`ocd cf` 的必要价值是**在线目标上下文**。`cf init/migrate/dev/build/workers types`、help/schema/search 与不上传的本地验证直接使用项目内官方 cf；不要求启动 daemon，也不增加一个无目标 launcher 模式。经 `ocd cf` 发起的调用仍按统一流程解析目标，不为每个离线命令建立特殊分支。

### 4.2 可执行文件与进程

从明确的项目目录按既有规则查找最近的项目内 `node_modules/.bin/cf`，支持 workspace hoist；不退回全局 cf、不在运行时执行 npx/bunx、不自动安装包或 Node。缺少工具时返回安装提示。官方 CLI 安装、升级由用户或 CI 显式完成。

cf 必须由满足其要求的 Node 执行；不要用 `bun --bun` 执行 cf。复用既有进程启动与信号边界，不引入 JS 代理进程来读取配置。Unix 保留 `exec`/TTY/signal/退出码语义，端到端覆盖 cf 自身启动的构建子进程。launcher 自己的目标/版本诊断仅写 stderr；不向 stdout 混入 banner，不解析、重排 cf 的 JSON 或错误。

### 4.3 凭据与环境

先清理现有冲突的 Cloudflare 凭据/endpoint 别名，再显式写入：

```text
CLOUDFLARE_API_BASE_URL=<selected-api-base>
CLOUDFLARE_ACCOUNT_ID=<selected-instance-account-id>
CLOUDFLARE_API_TOKEN=<selected-deployer-token>
CF_SEND_TELEMETRY=false
DO_NOT_TRACK=1
```

其中 API base 使用 registry/descriptor 已规范化、包含 `/client/v4` 的完整值，**不得重复追加** 该后缀。凭据复用已有来源，不新造短命 token 服务。清理会改变默认 authority 的遗留 account/region 上下文；显式业务参数仍交给上游和服务端校验。保持当前依赖实际读取的日志脱敏开关，例如 `WRANGLER_LOG_SANITIZE=true`；这是上游接口名称，不是保留 Wrangler 实现。[S8] [S13]

不读取或写入 cf OAuth profile，不要求 `cf auth login`，不把 OCD token 保存到 cf 的配置/项目文件或 argv。不重新实现 cf 的 `.env` 加载规则，也不读取用户 `.env` 来代替 target 凭据。测试必须证明进程环境的目标凭据不被 `.env`、profile 或旧环境变量覆盖。[S2] [S8]

项目 TS 配置、插件和构建脚本属于用户显式执行的代码，不是沙箱。`ocd cf deploy` 若构建，会把进程环境交给官方子进程；不能声称可阻止恶意项目读取 token。CI 推荐无部署凭据构建，然后只给 `deploy --prebuilt` 提供凭据。预构建也不应被宣传为执行不可信第三方工具的安全沙箱。

## 5. 旧配置预检与官方迁移

### 5.1 只做文件级诊断

对 wrapper 内需要项目配置的构建/上传操作，在加载凭据、联系目标或运行 cf 前检查 `--project` 确定的目录。默认检查当前目录，不递归扫描 monorepo、不向上寻找其他应用配置、不加载 Wrangler parser、不求值 TypeScript。

预检只识别为此必需的已认证命令前缀和 `--prebuilt`/help 标志；不复制 yargs、整个 cf command tree 或资源参数 schema。以 cf 官方 argv 语义测试选项值、`--` 分隔符和布尔标志；不能用“argv 任意位置出现 prebuilt 字符串”跳过检查。未识别的新命令不按 Wrangler 语义猜测，错误由上游处理；上游新增构建入口时通过认证更新这张小表。

适用入口包括非 prebuilt 的 `deploy`、`workers versions create`、`workers triggers deploy`，以及 wrapper 透传中触发配置准备的 `dev/build/workers check/previews deploy`。列出入口只为防止隐式项目修改，不表示 OCD 已支持 Preview 等产品能力。[S5]

| 条件 | 行为 |
| --- | --- |
| help/schema/search、普通资源 API 操作 | 不因 cwd 有旧配置而阻断 |
| 有效 `--prebuilt` 路径 | 不要求源码配置；产物与 mode 校验交给 cf |
| 存在 `cloudflare.config.ts` | 交给 cf；遗留 Wrangler 文件不阻断，迁移 TODO 由官方配置中的错误报告 |
| 没有新配置，存在 `wrangler.json/jsonc/toml` | 非零退出，打印实际发现的文件和官方迁移提示；不修改文件 |
| 没有新配置，也没有旧配置 | 项目型操作非零退出，提示显式使用 `cf init` 或配置官方 builder；不触发 cf autoconfig |

这张预检表是用户体验边界，不是安全沙箱或“所有非 cf 项目”的检测器。OCD 不审计用户整个依赖树，也不通过文件名推断上传产物是否合法。对于直接运行官方 cf 的用户，官方自己的行为仍然适用。[S3] [S5]

错误示例：

```text
Error [WRANGLER_PROJECT_UNSUPPORTED]
Found ./wrangler.json, but cloudflare.config.ts is missing.
open-compute supports the Cloudflare CLI project workflow only.

Migrate explicitly with Cloudflare's official tool:
  cf migrate ./wrangler.json --bundler vite

Complete the migration's required follow-up steps, then retry.
No files were changed by ocd.
```

多个旧文件时列出候选，要求用户向迁移器传确切路径，不自动选择。没有 cf 的用户可按文档显式使用下述一次性命令；OCD 不执行它。

### 5.2 用户迁移步骤

在依赖已安装且工作区干净的项目中，使用官方迁移器：

```sh
# 已安装 cf 时使用项目固定版本
cf migrate ./wrangler.json --bundler vite --dry-run
cf migrate ./wrangler.json --bundler vite

# 尚未安装时，可显式运行本文核对过的候选版本
npx --yes cf@1.0.0-beta.11 migrate ./wrangler.json --bundler vite
```

提示必须包含 `--bundler vite`：官方自动选择逻辑在未声明 Vite 插件时会选 Wrangler。`--dry-run` 不写文件；正式转换可以写出文件后因必需人工步骤返回非零。不要自动加 `--force`，也不要因退出码 1 就再次覆盖已生成配置。[S2] [S4]

迁移不会替用户配置完整的 Vite 构建。安装插件并添加下面的标准文件，按官方输出完成 TODO；不要承诺任意项目“一键无损迁移”。本仓库保持 Bun workspace 与单一 `bun.lock`；工作区 package/catalog 的变更需审查，不引入其他包管理器锁文件。`--no-install` 可用于显式管理 workspace 安装，但不是已完成迁移的证明。[S3]

```sh
# 用户安装示意；本仓库正式依赖必须写入准确版本及 lockfile
bun add -d vite @cloudflare/vite-plugin@beta
```

```ts
// vite.config.ts
import { cloudflare } from "@cloudflare/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [cloudflare()],
});
```

用户自行更新 scripts、tsconfig、CI 和 assets 等构建设置，并删除已不再使用的旧配置与 Wrangler 直接依赖。OCD 不替用户执行删除，也不在新旧文件同时存在时偷偷同步两份配置。

## 6. 配置语义：采用官方模型，不做第二次归一化

应用入口为官方 `cloudflare.config.ts`，应用构建设置放在 Vite/框架自己的配置中；`compute.toml` 仍只描述 OCD 平台。Rust 不加载 `cf/config`、不另造 binding grammar、不复刻官方配置 resolver。普通 JS/TS 示例、框架示例和 CI 都只维护一份应用部署配置。

mode 和 target 完全独立。沿用官方 mode 默认值；正式部署示例显式传 `--mode`，不由 launcher 添加默认 mode、合并 env、给 Worker 自动加后缀或替换资源 ID。迁移审查重点是保持所需的 Worker/class/resource 身份及隔离，不能只机械替换命令名。[S9] [S10]

Durable Object 生命周期使用官方 `worker.exports`。只声明仍存活的 class 和尚未执行的生命周期变化，不复制历史 rename/delete；OCD 仅承诺已支持的 SQLite 等合同。Workflow exports/bindings、assets 目录、D1 migration 路径等由官方迁移提示及项目配置显式完成，不在 launcher 中推导或补写。[S3] [S9]

特别约束：预构建输出可能已经记录 `accountId`，当前发布实现优先使用记录值，而不重新求值源码。不要仅凭环境变量文档就宣称任何 prebuilt 账号都会被覆盖。跨 target 的公共产物应不硬编码 account；显式记录的账号必须匹配目标。用真实测试验证错账号不能写入其他 instance，依赖现有服务端授权边界；若固定上游版本与目标隔离冲突，则阻止资格通过并优先修复上游/授权边界，不通过改写 Build Output 或二次执行配置兜底。[S2] [S12]

## 7. 官方构建链与 `oc` 工具链删除

### 7.1 不强制所有编译器都是 Vite

JS/TS 默认使用 cf + 官方 Vite 插件；纯后端 Worker 不需要 HTML 或前端框架。其他工具只有在官方 cf 能消费其合规产物时才可通过同一部署入口使用；这不是 OCD 承诺维护任意 esbuild/Rolldown adapter。cf 的官方 delegate 发现与选择规则仍由上游维护。[S5] [S6]

部署预构建内容必须是 cf 接受的完整 Cloudflare Build Output，不是任意 `dist/index.js` 或 OCD 私有 `.bundle`。OCD 不直接依赖尚不稳定的 `@cloudflare/build-output-utils` JS API，不定义第二套产物格式，也不把 Build Output 布局复制到 Rust。[S5] [S11]

“薄实现”不等于把 Wrangler 藏在下面继续使用：本仓库的正式应用路径移除 Wrangler build backend、`wrangler.config.ts` 和基于 Wrangler config reader 的代码。不维护对用户依赖树的封禁扫描；第三方客户端偶然可以调用 API，不构成 Wrangler 工作流认证承诺。

### 7.2 删除项与保留项

现有工具链同时承担配置投影、严格 tsc、Rolldown、framework output 导入、assets 扫描、Env 类型生成和客户端 bundle 编码。迁移按职责直接替换，不能只删除 CLI 入口而保留原编译器给 fixture 偷用。[R1]

| 当前职责 | 实现后的处理 |
| --- | --- |
| Wrangler config reader、规范化 WorkerProject、`.wrangler/deploy/config.json` 导入 | 删除；改用官方新配置/框架产物路径 |
| 自有 TS Worker 编译、模块解析条件、external/polyfill/bundler 规则 | 删除；应用交给官方构建链 |
| 应用侧 assets 扫描与部署编排 | 交给上游；OCD 服务端大小、路径、完整性和权限校验保留 |
| `oc build`、`oc types` 以及标准 binding 类型映射 | 删除；用户直接运行 `cf build` / `cf workers types` |
| 客户端私有 bundle 打包路径 | 不再用于正常应用部署；无真实内部使用方的 helper/命令一起删除 |
| 服务端 CanonicalBundle、immutable Version、模块 admission | 保留，这是内部运行时与存储合同，不是第二套编译器 |
| workerd、系统 Worker、dashboard、SDK 等平台自身构建 | 不因应用迁移而全量替换；维持各自现有职责 |

当前 multipart 接收层本来就会从官方上传的模块构造 `CanonicalBundle`。因此无需把官方产物再送入 `oc build`，也无需为了本次迁移改变内部 bundle 格式、SQLite 数据或快照。已经发布的数据库 migration 保持不可变。[R2]

检查所有真实调用方，包括直接导入 `compileWorker` 的 Postgres driver fixture；应用 fixture 使用同一官方路径，不为测试留下旧编译器。平台内部构建若还有独立使用 Rolldown 的合理职责，保留其显式所有权，不能因名称相同误删，也不能借此保留已废弃的通用应用工具链。[R3]

### 7.3 类型与平台扩展

标准 Env/runtime 类型使用 `cf workers types` 或官方插件生成结果，不二次生成一份竞争的 Env。严格类型检查继续由项目/CI 中的 TypeScript 承担；不能将转译成功等同于类型检查通过。

现有生成器还引用 `@open-compute/workers-types` 与 `open-compute:ai` 等专有类型。审查并保留确有需要的显式扩展声明/适配类型，不用全局合并强行覆盖不兼容的官方 Env 属性，不用 `any`、类型检查豁免或假的完整 Cloudflare 能力声明让迁移通过。[R4]

原生 extension 的安装授权、outbound 等平台配置继续在 OCD 配置中；应用端优先沿用已支持的官方 binding 表达。若某个扩展无法由官方配置/wire 表达，记录具体缺口，不以新增自有编译器、配置 DSL 或通用 facade 框架作为本阶段解决方案。

## 8. 开发与 CI 的唯一示例路径

初始化/迁移、开发、类型检查、构建直接用官方命令；实际 OCD 部署才需要 `ocd cf`。以下示例中的 `cf` 必须解析到项目固定安装，且实际由 Node 运行：

```json
{
  "type": "module",
  "scripts": {
    "dev": "cf dev",
    "typecheck": "cf workers types --mode production && tsc --noEmit",
    "build": "bun run typecheck && cf build --mode production"
  }
}
```

```sh
# 在 Worker package 中；Node 满足 cf 要求，依赖由 Bun 显式安装
bun run build
cf deploy --prebuilt --dry-run --mode production

# 只有此步骤获得所选 target 的部署凭据
ocd cf --target production deploy --prebuilt --mode production
```

cf 不会替用户执行 `package.json` 中的 build script；直接 `cf deploy` 不能保证先运行上述 tsc。不要在 wrapper 中偷偷补一次 typecheck/build。仓库维护的应用必须在 CI 中先完成严格类型检查；通用服务端只校验上传模块及运行时合同，不能声称能证明客户端执行过 tsc。[S5] [S14]

CI 可以直接注入三个标准 Cloudflare 环境变量并执行项目内 cf，无需创建开发机 target registry；这是同一个官方客户端路径，不是第二套部署实现。区分 Node 执行与 Bun 包管理，保留单一 workspace lock，使用 frozen 安装，不在 job 中跟随 latest。[S7] [S8] [S14]

官方 `.cloudflare/` 产物、生成类型和构建器标准输出留在项目规定位置，不搬进实例 data 目录，也不伪装成 OCD 管理缓存。仓库对这类官方生成目录做统一忽略；Gate 日志、录制流量及失败证据仍进入既有 `.temp/`。生产 daemon 启动与请求路径不运行 Node/Bun/Vite/tsc，也不联网下载工具或用户依赖。

`cf dev` 和 `--local` 使用官方本地开发资源，不等于连接本机 OCD。OCD 集成测试必须真实部署到明确的 instance/target。官方类型或本地模拟器支持某项能力，也不代表 OCD 已实现它。

## 9. 服务端：认证新客户端，保留同一 API authority

不建立 cf 专用路由、不检查客户端品牌、不 fork SDK。用固定版本 cf 的实际请求更新现有 API 兼容实现，包括 multipart metadata、模块 MIME、assets 多阶段上传、资源查询/创建、Version 与 Deployment、secret 继承、触发器和日志查询。

保持字段与语义的显式校验。新增官方字段时，根据已声明能力实现或明确拒绝，不用删除 `deny_unknown_fields`、忽略 mutation 或回传伪成功来通过测试。标准协议中仍有意义的 migration 等字段不因为项目已换 cf 就被一律删掉；其必要性取决于官方 wire contract，而不是旧客户端名称。

资源 provisioning 由 cf 发起，OCD 实现相同资源 API；不再在 launcher 中重复创建资源或把 ID 写回配置。命名、分页、raw/binary 响应、上传重定向及对象数据路径都要验证，不只测试列表命令。

不同产品更新可能是多个 API mutation。保持每个端点现有的一致性和失败语义，不能在文档里虚构“整个 cf deploy 跨所有产品原子”；中途失败不得破坏现有活动版本，已经完成的附属变更需可观察。也不新增全局分布式事务或客户端 mutation retry 来兜底。

## 10. 已知限制与相邻阶段

**实时终端 tail：**本阶段接受官方 cf 尚无等价终端流式入口的限制，使用已有 Dashboard Live Tail/日志查询；保留服务端日志能力。Dashboard 不是终端 tail 的等价承诺。不留 `ocd wrangler`，不新写 `ocd tail`，不把“创建 tail session”误报成已实现 WebSocket 日志客户端。上游补齐后在同一 cf 路径认证即可。[S9]

**Secrets：**采用已发布的 secret update/bulk 或版本上传的 secrets-file 路径，覆盖设置、覆盖更新、删除和 redeploy 保留。终端输入/管道和非交互参数以所固定版本 help/schema 为准；不在文档硬编码未核对的参数形状。[S2]

**Python/Rust/其他框架：**上游列出 delegate 不代表其整个部署路径已在 OCD 验收。P20 的正式 JS/TS 构建选择不等于要求其他语言使用 Vite，也不恢复 pywrangler/Wrangler 作为 OCD 正式入口。P21 的 Python runtime、包处理、snapshot 与语言无关 binding 工作仍属于 P21；其旧部署工具描述在联合实施时按 P20 的单轨 CLI 规则替换，官方语言构建链不可用时如实记录前置阻塞。[S6]

**Preview、Containers、Browser Run 等：**cf 暴露命令不自动扩大本平台支持范围；仍由各阶段和兼容矩阵决定。原生 extension、本地权限和实例数据模型均不是 CLI 迁移的重设计对象。

P20 是后续 CLI/应用构建实现的权威方案，替代 P12 中继续使用 Wrangler 的入口决策及 P25 的 Wrangler 项目输入待办。既有 implemented 文档保留真实历史验收，不提前改成 cf 已通过。根级开发规则中“Rolldown 负责全部应用 bundling”等旧约束，在代码切换时同步改为上述应用/平台自身构建边界。

## 11. 实施顺序：最终只交付一种实现

### A. 冻结与真实资格验证

固定 cf、Vite/plugin 与 Node 的候选版本；用简单 JS/TS、assets、DO、Workflow 和已有主要资源路径验证自定义 API base、上传和 target 隔离。复用现有 Gate 和 fixture，不维护独立 prototype 部署器。根据实际 HTTP 差异修正 API 或明确阻塞；发现上游问题优先提交上游，不留下永久本地补丁。

### B. 一次替换生产路径

将现有 launcher 改为 cf，加入第 5 节预检和第 4 节环境规则，直接替换 capability/错误/诊断中的旧入口。迁移应用配置、脚本和所有 `oc` 使用方，删除通用自有应用工具链与 Wrangler 路径；不保留一个“以后再删”的 deprecated 模式。

### C. 同步文档并完成验收

更新 onboarding、示例、CI、agent 指引、兼容矩阵和相关活动方案；公开终端 tail 限制。实现与回归通过后才将 P20 移入 implemented 并记录实际证据。代码切换前本方案的 planned 状态不变，文档/配置转换成功不能替代真实部署测试。

## 12. 验收与退出条件

使用既有统一 Gate，每项记录准确的客户端版本、命令、实际服务端/运行时结果和失败证据，不把“退出码 0”作为唯一判据。

| 验收面 | 必须覆盖 |
| --- | --- |
| 单轨与删除 | 无正式 Wrangler launcher/alias/fallback，无自有通用应用编译器及遗留调用方；依赖与活动文档一致 |
| 旧项目 UX | JSON/JSONC/TOML、多个旧文件、新旧并存、迁移 TODO、缺配置、monorepo；预检不改文件、不下载、不读取 token、不联系目标 |
| 预检边界 | help/资源命令不误拦，真实 prebuilt 无源码可用，选项值不被误当成标志，后续 argv 字节不变 |
| 进程 | 项目内/hoisted cf，缺包/版本错误、SemVer prerelease、Node 要求、TTY、Ctrl-C/SIGTERM、退出码、JSON stdout |
| 目标与凭据 | local/remote target、冲突 env/.env/profile、错误 account/预构建 account、token 脱敏；错误目标不能写到其他 instance |
| 外连边界 | 受支持 API、assets、R2 对象与响应中 URL 的真实路径；OCD token 不发往 Cloudflare/telemetry 或非选定服务；不承诺任意项目代码的网络隔离 |
| 应用构建 | 严格 tsc、TS/npm、ESM/CJS 依赖、nodejs_compat、拆分模块、Wasm/text/data、source maps、assets-only、框架与 Postgres fixture |
| 类型 | 标准官方 Env、跨 mode 类型检查、平台独有补充类型；无重复声明冲突、any/skip check 或夸大支持面 |
| 生命周期 | 首次/重复部署、纯 Version 上传及激活、已支持的 rollback；DO SQLite 数据保持与声明生命周期、Workflow、Cron/Queue 配置 |
| 资源与 secrets | 各已声明产品的真实读写、分页及错误；secret 更新/删除/继承、provisioning，以及相同名称在不同 target/mode 下的隔离 |
| 失败与运维 | 中途失败的可观察状态、旧活动版本仍可服务、daemon PID/监督合同不变；Dashboard Live Tail 仍可用 |
| CI/离线 | 无凭据构建、一次构建后 prebuilt 部署、dry-run 无上传；生产启动不依赖 Node/Bun/CLI、不隐式下载 |

注意 cf 的部分非交互破坏性操作会因未确认而取消，但仍以 0 退出。测试清理和 mutation 验收必须查询最终状态；不全局自动加 `--force`。[S14]

实现放行条件为：正式范围内真实 cf -> OCD 路径通过，旧路径与消费者已删除，安全/数据与失败语义不退化，文档声明与实际能力一致。尚未实现的其他语言/产品继续留在各自阶段，不拖入双实现；已支持的常用功能若出现除明确接受的 tail UX 外的回退，则修复或阻止发布，不默默缩减覆盖。

## 13. 资料与现有实现入口

下列外部材料按第 2 节日期核对；发布事实优先使用固定 tag，动态文档作为说明。实施时只重新核对受影响部分，不复制一整套上游规范。

[S1]: https://github.com/cloudflare/cf/releases/tag/cf%401.0.0-beta.11
[S2]: https://github.com/cloudflare/cf/blob/cf%401.0.0-beta.11/packages/cli/CHANGELOG.md
[S3]: https://developers.cloudflare.com/cf/wrangler/migrate/
[S4]: https://github.com/cloudflare/cf/blob/main/packages/cli/src/lib/wrangler-migration.ts
[S5]: https://developers.cloudflare.com/cf/projects/
[S6]: https://github.com/cloudflare/cf/blob/cf%401.0.0-beta.11/packages/cli/src/commands/dev/known-impls.ts
[S7]: https://developers.cloudflare.com/cf/get-started/
[S8]: https://developers.cloudflare.com/cf/environment-variables/
[S9]: https://developers.cloudflare.com/cf/wrangler/reference/
[S10]: https://developers.cloudflare.com/cf/projects/cloudflare-config/
[S11]: https://github.com/cloudflare/workers-sdk/blob/main/packages/build-output-utils/README.md
[S12]: https://github.com/cloudflare/cf/blob/cf%401.0.0-beta.11/packages/cli/src/commands/deploy/shared.ts
[S13]: https://github.com/cloudflare/cf/blob/main/packages/cli/src/lib/deploy-context.ts
[S14]: https://developers.cloudflare.com/cf/ci/
[R1]: ../packages/toolchain/README.md
[R2]: ../crates/service/src/workers_http/v4/multipart.rs
[R3]: ../test/applications/postgres-driver/build.ts
[R4]: ../packages/toolchain/src/generate-types.ts

相关设计：[P12 历史工作流](implemented/p12-wrangler-project-workflow.md)、[P21 Python](p21-python-workers.md)、[P25 后续能力](p25-platform-follow-ups.md)。代码迁移时删除的实现入口可改指对应提交记录，不能为保留文档链接而保留死代码。
