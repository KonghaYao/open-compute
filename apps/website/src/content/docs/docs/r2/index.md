---
title: "R2"
---

R2 is object storage that lets you store and retrieve unstructured data from a Worker. The Worker binding API matches Cloudflare. Object bytes are held by the platform-wide Local or S3 backend selected by the operator.

For example, you can use R2 for:

- Storage for unstructured objects
- Serving files from a Worker
- Multipart uploads on either supported backend

```ts
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const key = url.pathname.slice(1);
    if (request.method === "PUT") {
      await env.BUCKET.put(key, request.body);
      return new Response("ok");
    }
    const object = await env.BUCKET.get(key);
    if (object === null) return new Response("missing", { status: 404 });
    return new Response(object.body, { headers: { etag: object.httpEtag } });
  },
} satisfies ExportedHandler<{ BUCKET: R2Bucket }>;
```

Bind an existing logical bucket with cf's standard R2 field:

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "r2-app",
    entrypoint: "src/index.ts",
    env: {
      BUCKET: bindings.r2({ name: "files" }),
    },
  },
});
```

`bucket_name` names an existing logical bucket in the account. Binding grammar: [bindings](/docs/workers/configuration/bindings/). Use pinned cf or the official SDK for bucket and object operations.

## Compatibility

| Topic                       | Cloudflare                                                                                | open-compute                                                                                              |
| --------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Worker API                  | [R2 Workers API](https://developers.cloudflare.com/r2/api/workers/workers-api-reference/) | Same: `head` / `get` / `put` / `delete` / `list`, conditional writes, checksums, multipart, HTTP metadata |
| Object bytes                | Cloudflare R2 storage                                                                     | Configured Local or S3 authority on one node                                                              |
| Global placement            | Available                                                                                 | Not provided                                                                                              |
| r2.dev public product       | Available                                                                                 | Not provided                                                                                              |
| Jurisdictional restrictions | Available                                                                                 | Not provided                                                                                              |
| REST / `client/v4`          | Available                                                                                 | Compatible account-scoped bucket and object operations                                                    |

Next: [Develop with bindings](/docs/develop/) · [Compatibility and limits](/docs/reference/)
