---
title: "Compatibility dates"
---

每个部署的 Version 都有标准 `compatibility_date`。open-compute 原样持久化显式提交值，并交给正式 pinned workerd binary 校验。官方 Workers upload API 在省略日期时使用 `2021-11-02`；open-compute 在创建不可变 Version 前实现同一 boundary 规则。

```sh
curl -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  "$CLOUDFLARE_API_BASE_URL/open-compute/capabilities"
```

读取响应中的 `compatibility.binary_maximum_date`、`compatibility.future_dates_allowed` 和 `compatibility.validation`。日期不能晚于 binary maximum 或当前 UTC 日期；日期不是离散列表，open-compute 也不另设最小日期。日期只选择同一个 workerd binary 内的行为，语义见 [Cloudflare compatibility dates](https://developers.cloudflare.com/workers/configuration/compatibility-dates/)。

| 主题                        | Cloudflare      | open-compute                                                    |
| --------------------------- | --------------- | --------------------------------------------------------------- |
| 日期选择 workerd 可观察行为 | 是              | 是                                                              |
| 每项目 `compatibility_date` | 是              | 按不可变 Version 持久化；API 省略时使用官方 `2021-11-02` 默认值 |
| admission authority         | Workers runtime | 正式 pinned workerd binary                                      |
| 能力发现                    | 文档            | exact binary 内嵌的 compatibility catalog                       |
