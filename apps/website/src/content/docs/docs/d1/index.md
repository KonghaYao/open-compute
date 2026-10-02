---
title: "D1"
---

D1 is a SQLite SQL database that you query from a Worker. On this platform, each database is a local-primary SQLite file on the node running ocd.

For example, you can use D1 for:

- Querying relational data from a Worker
- Importing a schema and running SQL
- Batching statements in one transaction

```ts
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const { results } = await env.DB.prepare(
      "SELECT * FROM Customers WHERE CompanyName = ?",
    )
      .bind("Bs Beverages")
      .all();
    return Response.json(results);
  },
} satisfies ExportedHandler<{ DB: D1Database }>;
```

Bind an existing database with cf's standard D1 field:

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "d1-app",
    entrypoint: "src/index.ts",
    env: {
      DB: bindings.d1({ id: "<d1-database-id>", name: "app" }),
    },
  },
});
```

`database_id` identifies an existing database on this platform. Binding grammar: [bindings](/docs/workers/configuration/bindings/). Use project-local cf for types and `ocd cf deploy` for a real target.

## Compatibility

| Topic                        | Cloudflare                                                        | open-compute                                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Worker API                   | [D1 Worker API](https://developers.cloudflare.com/d1/worker-api/) | Same: `prepare` / `bind` / `run` / `all` / `first` / `raw` / `exec` / `batch`, sessions, opaque bookmarks, prepared-statement / result / meta |
| Topology                     | Hosted D1 with read replicas                                      | Local primary SQLite on the node running ocd                                                                                                  |
| Read replicas                | Available                                                         | Not provided                                                                                                                                  |
| Region routing               | Available                                                         | Not provided                                                                                                                                  |
| `served_by` geography        | Region / colo metadata                                            | Not provided; `served_by_*` is not a geography product                                                                                        |
| Bookmarks                    | Cross-replica causality                                           | Local ordering on the same database                                                                                                           |
| `rows_read` / `rows_written` | Billing counters                                                  | Local SQLite execution counts                                                                                                                 |
| `dump()`                     | Rejected on hosted non-alpha                                      | Rejected (`D1_DUMP_ERROR`)                                                                                                                    |
| REST / `client/v4`           | Available                                                         | Compatible account-scoped database and query operations                                                                                       |

Next: [Develop with bindings](/docs/develop/) · [Compatibility and limits](/docs/reference/)
