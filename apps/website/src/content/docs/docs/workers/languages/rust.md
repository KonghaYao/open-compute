---
title: "Rust"
description: "Build a workers-rs project to WebAssembly and deploy it to open-compute."
---

Cloudflare supports Rust Workers through the `workers-rs` crate. `worker-build` compiles Rust to WebAssembly and emits an ES module shim; project-local cf uploads both as a standard module Worker. This output shape is supported by open-compute.

## Create the project

Install the `wasm32-unknown-unknown` target and generate the official template:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-generate
cargo generate cloudflare/workers-rs
```

Keep cf installed in the generated project. The open-compute launcher deliberately resolves the nearest project-local cf rather than a global executable.

## Write the Worker

Use the `event` macro in `src/lib.rs` to expose the fetch handler. This example reads a KV binding:

```rust
use worker::*;

#[event(fetch)]
async fn main(_request: Request, env: Env, _ctx: Context) -> Result<Response> {
    let greeting = env
        .kv("CACHE")?
        .get("greeting")
        .text()
        .await?
        .unwrap_or_else(|| "Hello from Rust!".into());
    Response::ok(greeting)
}
```

The cf configuration points at the generated shim. Run `worker-build` explicitly before the official Vite build:

```ts
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "hello-rust",
    entrypoint: "build/worker/shim.mjs",
    compatibilityDate: "2026-09-08",
    env: {
      CACHE: bindings.kv({ id: "<namespace-id>" }),
    },
  },
});
```

All dependencies must compile for `wasm32-unknown-unknown`. Install `worker-build` explicitly or keep the build command produced by the official template.

## Develop and deploy

Use cf for the local loop, then use open-compute to select and authenticate the real target:

```sh
worker-build --release
npm run build
ocd cf deploy --prebuilt --mode production
```

Configure the official Vite plugin to bundle the generated shim and WASM. `ocd cf deploy --prebuilt` uploads that Build Output; it does not run `worker-build`. No Rust toolchain is needed on the production host after the immutable deployment has been created.

Cloudflare reference: [Rust language support](https://developers.cloudflare.com/workers/languages/rust/).
