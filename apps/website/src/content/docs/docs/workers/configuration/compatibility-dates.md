---
title: "Compatibility dates"
---

Every deployed Version has the standard `compatibility_date`. open-compute persists an explicitly submitted value unchanged and asks the formally pinned workerd binary to validate it. The official Workers upload API defaults an omitted date to `2021-11-02`; open-compute applies that same boundary rule before creating the immutable Version.

```sh
curl -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  "$CLOUDFLARE_API_BASE_URL/open-compute/capabilities"
```

Read `compatibility.binary_maximum_date`, `compatibility.future_dates_allowed`, and `compatibility.validation`. A date cannot be later than the binary maximum or the current UTC date. Dates are not a discrete list and open-compute does not impose its own minimum. The date selects behavior inside that same workerd binary as documented by [Cloudflare compatibility dates](https://developers.cloudflare.com/workers/configuration/compatibility-dates/).

| Topic                                    | Cloudflare      | open-compute                                                                         |
| ---------------------------------------- | --------------- | ------------------------------------------------------------------------------------ |
| Date selects workerd observable behavior | Yes             | Yes                                                                                  |
| Per-project `compatibility_date`         | Yes             | Persisted per immutable Version; API omission uses the official `2021-11-02` default |
| Admission authority                      | Workers runtime | Formally pinned workerd binary                                                       |
| Discovery                                | Documentation   | Exact binary's embedded compatibility catalog                                        |
