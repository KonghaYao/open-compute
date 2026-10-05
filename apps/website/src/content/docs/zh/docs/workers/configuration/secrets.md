---
title: "Secrets"
---

使用项目内 cf。将 secret 文件保存在仓库外并限制为 owner-only，值不能出现在 argv、package scripts、日志、target 记录或应用配置中。bulk 使用 JSON Merge Patch：字符串设置/覆盖值，`null` 删除，未提及的值保持不变。

```sh
ocd cf --target staging workers secrets bulk --worker app-staging --body @/secure/secrets.json
ocd cf --target staging workers secrets list --worker app-staging
ocd cf --target staging workers secrets delete API_TOKEN --worker app-staging --force
ocd cf --target staging deploy --mode staging --secrets-file /secure/deploy-secrets.json
```

目标 deployer token 只用于管理请求。项目和构建脚本可以读取其进程环境，因此 CI 应先无部署凭据构建，再 prebuilt 部署。CLI 参数以固定版本 help/schema 为准，不承诺未验证的 stdin 输入。

secret mutation 继续遵循 immutable Version 模型。平台加密持久化 secret，读取接口只公开名称和类型；rollback 恢复该 Version 的绑定。

```json
{
  "secrets": {
    "API_TOKEN": {
      "name": "API_TOKEN",
      "type": "secret_text",
      "text": "<value>"
    },
    "OLD_SECRET": null
  }
}
```

bulk 文件使用上面的 API 对象；`deploy --secrets-file` 使用名称到字符串值的平面映射。
