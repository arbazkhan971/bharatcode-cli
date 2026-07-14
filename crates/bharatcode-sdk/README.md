# bharatcode-sdk

The bindings layer for BharatCode. It houses shared ACP and SDK types and exposes
a cross-language version of the BharatCode API.

With `--features uniffi` the crate compiles to native bindings for Python and
Kotlin (namespace `bharatcode_sdk` / `dev.bharatcode.sdk`). The published surface is
currently a `ping` -> `pong` stub in `src/bindings.rs` — the scaffold for the
real implementation.

```bash
just python   # build bindings + run examples/uniffi/ping.py
just kotlin   # build bindings + run examples/uniffi/Ping.kt
```

Both print `pong: aaif.io`.
