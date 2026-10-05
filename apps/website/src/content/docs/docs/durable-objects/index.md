---
title: "Durable Objects"
---

Durable Objects bind compute and strongly consistent storage to one object. On this platform every object lives on its instance's single local `workerd` process.

For example, you can use Durable Objects for:

- Coordinating state among multiple clients
- Strongly consistent per-object storage
- Alarms and WebSocket hibernation

```ts
export class Counter {
  constructor(
    private readonly ctx: DurableObjectState,
    private readonly env: Env,
  ) {}
  async fetch(request: Request): Promise<Response> {
    const n = ((await this.ctx.storage.get<number>("n")) ?? 0) + 1;
    await this.ctx.storage.put("n", n);
    return Response.json({ n });
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const id = env.COUNTER.idFromName("global");
    return env.COUNTER.get(id).fetch(request);
  },
} satisfies ExportedHandler<{ COUNTER: DurableObjectNamespace }>;
```

Bind in `cloudflare.config.ts` with cf's standard Durable Object field:

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "do-app",
    entrypoint: "src/index.ts",
    exports: {
      Counter: {
        type: "durable-object",
        storage: "sqlite",
      },
    },
    env: {
      COUNTER: bindings.durableObject({
        worker: "do-app",
        exportName: "Counter",
      }),
    },
  },
});
```

The class is part of the uploaded Worker; Durable Object lifecycle uses the official `worker.exports` declarations. Grammar: [bindings](/docs/workers/configuration/bindings/).

## Compatibility

| Topic               | Cloudflare                                                                    | open-compute                                                                                                                                                   |
| ------------------- | ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Worker / class API  | [Durable Objects API](https://developers.cloudflare.com/durable-objects/api/) | Same: namespace `idFromName` / `newUniqueId` / `idFromString` / `get` / `getByName`, stub `fetch` / RPC, `state.storage` KV and SQL, transactions, output gate |
| Placement           | Geographic scheduling, `locationHint` / jurisdiction / migration              | All objects on the instance's local workerd; `locationHint` / jurisdiction / migration have no geo effect                                                      |
| Alarms              | Available                                                                     | 7 methods supported: `getAlarm` / `setAlarm` / `deleteAlarm` and the `alarm()` handler                                                                         |
| Hibernation         | Available                                                                     | Supported                                                                                                                                                      |
| Binding             | cf `bindings.durableObject`                                                   | Standard `name` and `class_name`; `class_name` required                                                                                                        |
| `Fetcher.connect()` | General outbound                                                              | Declared capability tunnel                                                                                                                                     |

Next: [Alarms](/docs/durable-objects/alarms/) · [Develop with bindings](/docs/develop/) · [Compatibility and limits](/docs/reference/)
