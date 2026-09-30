---
title: "AI Search"
---

AI Search indexes files you upload, then runs keyword, vector, or hybrid retrieval and optional chat. Markdown Conversion is exposed on the same standard `env.AI` binding via `toMarkdown()` / `supported()`.

open-compute uses **operator-configured providers**: OpenAI-compatible embeddings/chat endpoints and dedicated Cohere v2 or generic `/v1/rerank` endpoints. Full Workers AI model inference (`run()`, `models()`, AutoRAG, and unrelated inference) is **not** provided.

> **Text queries only:** AI Search currently accepts text `query` values and messages with string content. Image, file, and mixed text-image queries are not supported and fail closed instead of being downgraded to text search. OCR and optional image descriptions during document ingestion convert source files into indexable text; they do not add multimodal query support.

For example, you can use AI Search for:

- Uploading documents or indexing bounded R2 sources, then searching them from a Worker
- Hybrid retrieval before generating an answer
- Metadata relevance boosting followed by dedicated reranking
- Converting Office/PDF/HTML inputs to Markdown with `env.AI.toMarkdown()`

```ts
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const result = await env.SEARCH.search({ query: "cache invalidation" });
    return Response.json(result);
  },
} satisfies ExportedHandler<{ SEARCH: AiSearchInstance }>;
```

Bind a namespace and/or instance, plus the platform `ai` binding when you need Markdown Conversion:

```json
{
  "name": "search-app",
  "main": "src/index.ts",
  "ai_search_namespaces": [{ "binding": "SEARCH_NS", "namespace": "team" }],
  "ai_search": [{ "binding": "SEARCH", "instance_name": "docs" }],
  "ai": { "binding": "AI" }
}
```

## Manual external sources (open-compute extension)

`open-compute:manual` is a namespaced API superset for applications that already own immutable file revisions. It never scans a source and does not persist a second complete source object; it keeps only the locator and normal derived parse/index state. Configure one loopback provider per source authority:

```toml
[ai.source_providers.files]
endpoint = "http://127.0.0.1:9080/provider"
source = "files"
credential = { env = "FILES_SOURCE_TOKEN" }
max_source_bytes = 67108864
```

The provider implements authenticated `POST <endpoint>/resolve` and `POST <endpoint>/read` for exact `{ source, key, revision }` requests. `resolve` returns `revision`, `contentType`, `size`, and `sha256`; `read` repeats those facts in `Content-Type`, `X-Open-Compute-Revision`, `X-Open-Compute-Size`, and `X-Open-Compute-Sha256`. Redirects, compression, revision drift, digest mismatch, and oversized bodies fail closed.

Use the types exported only by `open-compute:ai-search`:

```ts
import type { OpenComputeAiSearchNamespace } from "open-compute:ai-search";

const index = await env.SEARCH_NS.openComputeCreateManual("files", {
  id: "documents",
  embedding_model: "company/qwen-embedding",
  index_method: { keyword: true, vector: true },
});
await index.items.openComputeUpsert({
  key: "files/blob-123",
  revision: "immutable-revision",
  contentType: "application/pdf",
  metadata: { team_id: "team-1" },
  waitForCompletion: false,
});
```

Repeating the same exact revision is idempotent. A changed revision uses the existing generation fence, and delete removes only the locator and derived index state. Manual instances and fields are absent from the official Cloudflare management surface.

For manual instances, item info, list, download, and search results include `open_compute_source` with the provider, source namespace, key, and exact revision so the application can re-authorize the original object.

Official reference: [Cloudflare AI Search](https://developers.cloudflare.com/ai-search/). Binding grammar: [bindings](/docs/workers/configuration/bindings/).

## Compatibility

| Topic                     | Cloudflare                                            | open-compute                                                                         |
| ------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------ |
| AI Search Worker API      | Namespace / instance / items / jobs / search / chat   | Same declared surface                                                                |
| Query modality            | Text, image, or multimodal when the model supports it | **Text only**                                                                        |
| Markdown Conversion       | `env.AI.toMarkdown()` / `supported()`                 | Same pinned overloads                                                                |
| Embeddings / chat models  | Cloudflare-hosted Workers AI                          | Operator-pinned OpenAI-compatible providers                                          |
| Reranking model           | Cloudflare-hosted Workers AI                          | Operator-pinned `cohere_rerank_v2` or `rerank_v1` provider                           |
| Search response           | `query_kind` plus retrieval and reranking scores      | Text success returns `query_kind: "text"`; reranked chunks include `reranking_score` |
| Full Workers AI inference | `run()` / `models()` / AutoRAG                        | **Not provided**                                                                     |
| Object bytes              | Hosted storage                                        | Selected Local or S3 authority                                                       |
| Placement / replication   | Global                                                | Single-node                                                                          |
| Manual external source    | Not an official member                                | `open-compute:manual` namespaced extension                                           |

Next: [Develop with bindings](/docs/develop/) · [Compatibility and limits](/docs/reference/)
