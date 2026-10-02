---
title: "Static Assets"
---

Static files freeze into the same immutable deployment. Configure them under `assets`.

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "site",
    entrypoint: "src/index.ts",
    assets: {
      runWorkerFirst: false,
      htmlHandling: "auto-trailing-slash",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
    },
  },
});
```

`worker.assets.htmlHandling`: `auto-trailing-slash` (default), `force-trailing-slash`, `drop-trailing-slash`, `none`. `worker.assets.notFoundHandling`: `none` (default), `404-page`, `single-page-application`. `worker.assets.runWorkerFirst` may be a boolean or a list of path rules starting with `/` or `!/`. Assets-only projects omit `worker.entrypoint` and cannot declare an execution environment or Worker-first.

When a binding is present, `env.<binding>.fetch()` serves assets only and never enters the tenant Worker.

## Compatibility

| Topic                                                                              | Cloudflare                                                                                 | open-compute                                                                |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| HTML trailing slash, SPA, Worker-first, `_headers` / `_redirects` routing concepts | Yes — [Cloudflare Static Assets](https://developers.cloudflare.com/workers/static-assets/) | Aligned                                                                     |
| Object storage                                                                     | Global CDN                                                                                 | Immutable objects on the selected Local/S3 authority, served from this node |
| Global CDN placement / replication / purge propagation / product quotas            | Yes                                                                                        | Not provided                                                                |
| Pages migration wizard                                                             | Yes                                                                                        | Not provided                                                                |

Set the assets directory with Vite `publicDir`; declare the fetch binding with `bindings.assets()`. Configure source map output with Vite `build.sourcemap`.
