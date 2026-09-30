# Fixed Pyodide bundle dependency

`pyodide_314.0.6_2026-08-17_6.capnp.bin.gz` is the platform-independent Python runtime bundle
used by the formally pinned workerd revision. It is a Git LFS build input, not a release sidecar.
The sole authority is [`workerd.lock.json`](../../packages/runtime/workerd.lock.json), which records
the version, file names, compressed SHA-256, decompressed SHA-256, workerd Bazel target, and pinned
gzip compressor identity.

The decompressed bytes are published by workerd's Pyodide bundle authority and selected by
`//src/pyodide:pyodide.capnp.bin@rule@314.0.6` at workerd revision
`e3bdb07f52affc6a618f02ed2b731a581b0b2f69`. Their SHA-256 is
`3c3fd5a4179230e21e018e28b0e735e7c4abd8fe8e4ff39d2fd6bb2ea3a2e260`, matching the integrity
declared by that workerd source. Bun 1.3.14 `node:zlib` gzip level 9 with timestamp zero, no
filename, and OS marker 255 produces the checked-in 6,326,148-byte archive with SHA-256
`043d493bd89473409cdbf8504b0a35cdc31122b3effd87067cacbf19fa8e976b`.

Root `bun run build` verifies the LFS object and both digests. Cargo embeds the verified gzip in
the native `ocd` executable. Under the data-directory lock, runtime materialization decompresses
it into the private content-addressed runtime package and passes that directory to workerd through
`--pyodide-bundle-disk-cache-dir`. Production startup and Python execution do not download it.
