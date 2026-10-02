---
title: "Compatibility flags"
---

Projects use cf's standard `compatibility_flags` array. open-compute preserves it unchanged; the pinned workerd binary accepts or rejects the complete date/flag combination.

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "hello-typescript",
    entrypoint: "src/index.ts",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: [],
  },
});
```

Use cf's snake_case field names. `GET /client/v4/open-compute/capabilities` returns every enable/disable input reflected from the exact binary, including default dates, implications, and experimental status. This catalog is for discovery; deployment still runs workerd validation. Internal system flags remain executable identity and are not copied into project configuration.

The formal self-host runtime runs workerd's experimental process mode, so catalog entries marked experimental can be admitted when the binary accepts them. Cloudflare's hosted Dynamic Workers documentation says experimental flags cannot be enabled in production; do not treat local admission as hosted-production availability.

| Topic                                    | Cloudflare                                                                                                     | open-compute                                  |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| Flag names and semantics                 | [Cloudflare compatibility flags](https://developers.cloudflare.com/workers/configuration/compatibility-flags/) | Same names from workerd                       |
| Project `compatibility_flags`            | Yes                                                                                                            | Persisted and validated per immutable Version |
| Unknown, duplicate, or conflicting flags | Upload fails                                                                                                   | Pinned workerd rejects the candidate          |
| Live supported set                       | Dashboard / cf                                                                                                 | Exact binary's embedded compatibility catalog |
