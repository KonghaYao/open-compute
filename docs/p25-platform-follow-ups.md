# P25：平台后续能力

状态：**planned**。本页保留尚未进入独立阶段的现有产品待办；开始实施时应冻结每项合同，若范围扩大则拆成独立编号。

## 待办

- [ ] cf 原生 Python builder 可用并通过资格检查后，删除 `scripts/build-python.ts` 的临时 PyWrangler/Wrangler 构建桥接，更新 Python 用户文档和构建测试。当前仅 Python 构建调用用户安装的 PyWrangler，不安装、不校验版本；上传、部署、开发与资源管理统一使用 cf。
- [ ] 跟进 [cloudflare/workers-py#287](https://github.com/cloudflare/workers-py/issues/287)。官方 SDK 发布修复后，重新验证 Flask 流式 body、并发 request/app context 和 cancellation cleanup，再恢复支持声明。当前按已知上游问题暂缓，原始 SDK 与失败证据保留。
- [ ] 支持从显式 `.env` 输入读取 instance 配置值，不隐式发现 cwd 或用户目录。
- [ ] 完成 operator-facing logger 合同，包括输出、级别、敏感信息清理和持久化边界。
- [ ] 评估并实现 lazy Worker startup，同时保持 readiness、首请求失败语义和受监督进程恢复合同。

## 已转交或撤销的方向

原 `wrangler.toml` 项目输入与 Wrangler transport 待办撤销，由 [P20：Cloudflare CLI 单轨迁移与官方应用构建链](implemented/p20-cf-cli-migration.md) 统一接管。OCD 不实现旧项目兼容层；检测旧配置后提示用户显式运行官方 `cf migrate --bundler vite`。

上面的 `.env` 待办只针对 instance 配置，不是应用项目 dotenv；应用配置与 CLI 凭据文件规则由官方 cf 负责，见 P20。

workerd compatibility authority 已拆入 R3。Q1 coverage 与 P24 macOS 签名已有各自的活动方案，不在这里重复跟踪。
