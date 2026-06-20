# Third-Party Licenses

BharatCode is licensed under the **Apache License, Version 2.0** (see [`LICENSE`](LICENSE)). It is a derivative work of [Goose](https://github.com/block/goose) (Apache-2.0); upstream attribution is recorded in [`NOTICE`](NOTICE) and [`MODIFICATIONS.md`](MODIFICATIONS.md).

This file aggregates the licenses of the third-party Rust crates that the `bharatcode` CLI links against. The dependency set is scoped to the actual release build:

```
cargo tree -e no-dev -p goose-cli --no-default-features --features portable-default
```

Licenses and repository URLs are taken from each crate's published Cargo metadata (the SPDX `license` field). First-party BharatCode workspace crates are excluded. Crates pulled in only by the desktop UI, optional code-mode, or other non-portable-default features are **not** listed here, since they are not part of this build.

Generated from `cargo metadata --format-version 1`.

## Summary

- Third-party crates in the `bharatcode` CLI portable-default tree: **619**
- Distinct SPDX license expressions: **41**

| SPDX license expression | Crate count |
|---|---|
| `MIT OR Apache-2.0` | 262 |
| `MIT` | 153 |
| `Apache-2.0` | 51 |
| `Apache-2.0 OR MIT` | 32 |
| `MIT/Apache-2.0` | 28 |
| `Unicode-3.0` | 20 |
| `Unlicense OR MIT` | 6 |
| `Zlib` | 6 |
| `Apache-2.0/MIT` | 5 |
| `BSD-3-Clause` | 5 |
| `ISC` | 4 |
| `MIT OR Apache-2.0 OR Zlib` | 4 |
| `Unlicense/MIT` | 4 |
| `Apache-2.0 OR ISC OR MIT` | 3 |
| `BSD-2-Clause` | 3 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 2 |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 |
| `BSD-3-Clause OR Apache-2.0` | 2 |
| `CC0-1.0 OR MIT-0 OR Apache-2.0` | 2 |
| `CDLA-Permissive-2.0` | 2 |
| `MPL-2.0` | 2 |
| `Zlib OR Apache-2.0 OR MIT` | 2 |
| `(Apache-2.0 OR MIT) AND BSD-3-Clause` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `0BSD OR MIT OR Apache-2.0` | 1 |
| `Apache-2.0 / MIT` | 1 |
| `Apache-2.0 AND ISC` | 1 |
| `Apache-2.0 OR BSD-3-Clause` | 1 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `Apache-2.0 OR BSL-1.0 OR MIT` | 1 |
| `BSD-3-Clause AND MIT` | 1 |
| `BSD-3-Clause/MIT` | 1 |
| `bzip2-1.0.6` | 1 |
| `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception` | 1 |
| `ISC AND (Apache-2.0 OR ISC)` | 1 |
| `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)` | 1 |
| `LGPL-3.0-or-later` | 1 |
| `MIT AND BSD-3-Clause` | 1 |
| `MIT OR Zlib OR Apache-2.0` | 1 |
| `MIT-0` | 1 |
| `Unlicense OR MIT OR Apache-2.0 OR CC0-1.0` | 1 |

## Crates by license

### (Apache-2.0 OR MIT) AND BSD-3-Clause

| Crate | Version | Repository |
|---|---|---|
| `encoding_rs` | 0.8.35 | [link](https://github.com/hsivonen/encoding_rs) |

### (MIT OR Apache-2.0) AND Unicode-3.0

| Crate | Version | Repository |
|---|---|---|
| `unicode-ident` | 1.0.24 | [link](https://github.com/dtolnay/unicode-ident) |

### 0BSD OR MIT OR Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `adler2` | 2.0.1 | [link](https://github.com/oyvindln/adler2) |

### Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `agent-client-protocol` | 0.11.1 | [link](https://github.com/agentclientprotocol/rust-sdk) |
| `agent-client-protocol-derive` | 0.11.1 | [link](https://github.com/agentclientprotocol/rust-sdk) |
| `agent-client-protocol-schema` | 0.12.0 | [link](https://github.com/agentclientprotocol/agent-client-protocol) |
| `approx` | 0.5.1 | [link](https://github.com/brendanzab/approx) |
| `aws-config` | 1.8.18 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-credential-types` | 1.2.14 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-runtime` | 1.7.4 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-sdk-bedrockruntime` | 1.133.0 | [link](https://github.com/awslabs/aws-sdk-rust) |
| `aws-sdk-sagemakerruntime` | 1.104.0 | [link](https://github.com/awslabs/aws-sdk-rust) |
| `aws-sdk-sso` | 1.101.0 | [link](https://github.com/awslabs/aws-sdk-rust) |
| `aws-sdk-ssooidc` | 1.103.0 | [link](https://github.com/awslabs/aws-sdk-rust) |
| `aws-sdk-sts` | 1.106.0 | [link](https://github.com/awslabs/aws-sdk-rust) |
| `aws-sigv4` | 1.4.5 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-async` | 1.2.14 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-eventstream` | 0.60.20 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-http` | 0.63.6 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-json` | 0.62.7 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-observability` | 0.2.6 | [link](https://github.com/awslabs/smithy-rs) |
| `aws-smithy-query` | 0.60.15 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-runtime` | 1.11.3 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-runtime-api` | 1.12.3 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-runtime-api-macros` | 1.0.0 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-schema` | 0.1.0 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-types` | 1.5.0 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-smithy-xml` | 0.60.15 | [link](https://github.com/smithy-lang/smithy-rs) |
| `aws-types` | 1.3.16 | [link](https://github.com/smithy-lang/smithy-rs) |
| `bytesize` | 1.3.3 | [link](https://github.com/bytesize-rs/bytesize/) |
| `calendrical_calculations` | 0.2.4 | [link](https://github.com/unicode-org/icu4x) |
| `gethostname` | 1.1.0 | [link](https://codeberg.org/swsnr/gethostname.rs.git) |
| `hf-hub` | 1.0.0-rc.1 | [link](https://github.com/huggingface/hf-hub) |
| `hf-xet` | 1.5.2 | [link](https://github.com/huggingface/xet-core) |
| `memo-map` | 0.3.3 | [link](https://github.com/mitsuhiko/memo-map) |
| `minijinja` | 2.20.0 | [link](https://github.com/mitsuhiko/minijinja) |
| `opentelemetry` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry) |
| `opentelemetry-appender-tracing` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-appender-tracing) |
| `opentelemetry-http` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-http) |
| `opentelemetry-otlp` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-otlp) |
| `opentelemetry-proto` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-proto) |
| `opentelemetry-stdout` | 0.32.0 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-stdout) |
| `opentelemetry_sdk` | 0.32.1 | [link](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-sdk) |
| `prost` | 0.14.3 | [link](https://github.com/tokio-rs/prost) |
| `prost-derive` | 0.14.3 | [link](https://github.com/tokio-rs/prost) |
| `rmcp` | 1.7.0 | [link](https://github.com/modelcontextprotocol/rust-sdk/) |
| `rmcp-macros` | 1.7.0 | [link](https://github.com/modelcontextprotocol/rust-sdk/) |
| `sync_wrapper` | 1.0.2 | [link](https://github.com/Actyx/sync_wrapper) |
| `unicode-linebreak` | 0.1.5 | [link](https://github.com/axelf4/unicode-linebreak) |
| `xet-client` | 1.5.2 | [link](https://github.com/huggingface/xet-core) |
| `xet-core-structures` | 1.5.2 | [link](https://github.com/huggingface/xet-core) |
| `xet-data` | 1.5.2 | [link](https://github.com/huggingface/xet-core) |
| `xet-runtime` | 1.5.2 | [link](https://github.com/huggingface/xet-core) |
| `zopfli` | 0.8.3 | [link](https://github.com/zopfli-rs/zopfli) |

### Apache-2.0 / MIT

| Crate | Version | Repository |
|---|---|---|
| `fnv` | 1.0.7 | [link](https://github.com/servo/rust-fnv) |

### Apache-2.0 AND ISC

| Crate | Version | Repository |
|---|---|---|
| `ring` | 0.17.14 | [link](https://github.com/briansmith/ring) |

### Apache-2.0 OR BSD-3-Clause

| Crate | Version | Repository |
|---|---|---|
| `seccompiler` | 0.5.0 | [link](https://github.com/rust-vmm/seccompiler) |

### Apache-2.0 OR BSL-1.0

| Crate | Version | Repository |
|---|---|---|
| `ryu` | 1.0.23 | [link](https://github.com/dtolnay/ryu) |

### Apache-2.0 OR BSL-1.0 OR MIT

| Crate | Version | Repository |
|---|---|---|
| `whoami` | 2.1.2 | [link](https://github.com/ardaku/whoami) |

### Apache-2.0 OR ISC OR MIT

| Crate | Version | Repository |
|---|---|---|
| `hyper-rustls` | 0.27.9 | [link](https://github.com/rustls/hyper-rustls) |
| `rustls` | 0.23.40 | [link](https://github.com/rustls/rustls) |
| `rustls-native-certs` | 0.8.3 | [link](https://github.com/rustls/rustls-native-certs) |

### Apache-2.0 OR MIT

| Crate | Version | Repository |
|---|---|---|
| `atomic-waker` | 1.1.2 | [link](https://github.com/smol-rs/atomic-waker) |
| `autocfg` | 1.5.1 | [link](https://github.com/cuviper/autocfg) |
| `bit-set` | 0.8.0 | [link](https://github.com/contain-rs/bit-set) |
| `bit-vec` | 0.8.0 | [link](https://github.com/contain-rs/bit-vec) |
| `cmov` | 0.5.3 | [link](https://github.com/RustCrypto/utils) |
| `concurrent-queue` | 2.5.0 | [link](https://github.com/smol-rs/concurrent-queue) |
| `const-oid` | 0.10.2 | [link](https://github.com/RustCrypto/formats) |
| `ctor` | 0.6.3 | [link](https://github.com/mmastrac/rust-ctor) |
| `ctor-proc-macro` | 0.0.7 | [link](https://github.com/mmastrac/rust-ctor) |
| `ctutils` | 0.4.2 | [link](https://github.com/RustCrypto/utils) |
| `dtor` | 0.1.1 | [link](https://github.com/mmastrac/rust-ctor) |
| `dtor-proc-macro` | 0.0.6 | [link](https://github.com/mmastrac/rust-ctor) |
| `equivalent` | 1.0.2 | [link](https://github.com/indexmap-rs/equivalent) |
| `event-listener` | 5.4.1 | [link](https://github.com/smol-rs/event-listener) |
| `fastrand` | 2.4.1 | [link](https://github.com/smol-rs/fastrand) |
| `futures-lite` | 2.6.1 | [link](https://github.com/smol-rs/futures-lite) |
| `idna_adapter` | 1.2.1 | [link](https://github.com/hsivonen/idna_adapter) |
| `indexmap` | 2.14.0 | [link](https://github.com/indexmap-rs/indexmap) |
| `parking` | 2.2.1 | [link](https://github.com/smol-rs/parking) |
| `pin-project` | 1.1.13 | [link](https://github.com/taiki-e/pin-project) |
| `pin-project-internal` | 1.1.13 | [link](https://github.com/taiki-e/pin-project) |
| `pin-project-lite` | 0.2.17 | [link](https://github.com/taiki-e/pin-project-lite) |
| `portable-atomic` | 1.13.1 | [link](https://github.com/taiki-e/portable-atomic) |
| `process-wrap` | 9.1.0 | [link](https://github.com/watchexec/process-wrap) |
| `rustc-hash` | 2.1.2 | [link](https://github.com/rust-lang/rustc-hash) |
| `signature` | 2.2.0 | [link](https://github.com/RustCrypto/traits/tree/master/signature) |
| `smithy-transport-reqwest` | 0.1.0 |  |
| `utf8_iter` | 1.0.4 | [link](https://github.com/hsivonen/utf8_iter) |
| `utf8parse` | 0.2.2 | [link](https://github.com/alacritty/vte) |
| `uuid` | 1.23.3 | [link](https://github.com/uuid-rs/uuid) |
| `zeroize` | 1.8.2 | [link](https://github.com/RustCrypto/utils) |
| `zeroize_derive` | 1.4.3 | [link](https://github.com/RustCrypto/utils/tree/master/zeroize/derive) |

### Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT

| Crate | Version | Repository |
|---|---|---|
| `linux-raw-sys` | 0.12.1 | [link](https://github.com/sunfishcode/linux-raw-sys) |
| `rustix` | 1.1.4 | [link](https://github.com/bytecodealliance/rustix) |

### Apache-2.0/MIT

| Crate | Version | Repository |
|---|---|---|
| `bit_field` | 0.10.3 | [link](https://github.com/phil-opp/rust-bit-field) |
| `bytecount` | 0.6.9 | [link](https://github.com/llogiq/bytecount) |
| `bytes-utils` | 0.1.4 | [link](https://github.com/vorner/bytes-utils) |
| `flume` | 0.11.1 | [link](https://github.com/zesterer/flume) |
| `rustc-hash` | 1.1.0 | [link](https://github.com/rust-lang-nursery/rustc-hash) |

### BSD-2-Clause

| Crate | Version | Repository |
|---|---|---|
| `arrayref` | 0.3.9 | [link](https://github.com/droundy/arrayref) |
| `git-version` | 0.3.9 | [link](https://github.com/fusion-engineering/rust-git-version) |
| `git-version-macro` | 0.3.9 | [link](https://github.com/fusion-engineering/rust-git-version) |

### BSD-2-Clause OR Apache-2.0 OR MIT

| Crate | Version | Repository |
|---|---|---|
| `zerocopy` | 0.8.48 | [link](https://github.com/google/zerocopy) |
| `zerocopy-derive` | 0.8.48 | [link](https://github.com/google/zerocopy) |

### BSD-3-Clause

| Crate | Version | Repository |
|---|---|---|
| `alloc-no-stdlib` | 2.0.4 | [link](https://github.com/dropbox/rust-alloc-no-stdlib) |
| `alloc-stdlib` | 0.2.2 | [link](https://github.com/dropbox/rust-alloc-no-stdlib) |
| `exr` | 1.74.0 | [link](https://github.com/johannesvollmer/exrs) |
| `lebe` | 0.5.3 | [link](https://github.com/johannesvollmer/lebe) |
| `subtle` | 2.6.1 | [link](https://github.com/dalek-cryptography/subtle) |

### BSD-3-Clause AND MIT

| Crate | Version | Repository |
|---|---|---|
| `brotli` | 8.0.2 | [link](https://github.com/dropbox/rust-brotli) |

### BSD-3-Clause OR Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `moxcms` | 0.8.1 | [link](https://github.com/awxkee/moxcms.git) |
| `pxfm` | 0.1.29 | [link](https://github.com/awxkee/pxfm) |

### BSD-3-Clause/MIT

| Crate | Version | Repository |
|---|---|---|
| `brotli-decompressor` | 5.0.0 | [link](https://github.com/dropbox/rust-brotli-decompressor) |

### bzip2-1.0.6

| Crate | Version | Repository |
|---|---|---|
| `libbz2-rs-sys` | 0.2.5 | [link](https://github.com/trifectatechfoundation/libbzip2-rs) |

### CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception

| Crate | Version | Repository |
|---|---|---|
| `blake3` | 1.8.5 | [link](https://github.com/BLAKE3-team/BLAKE3) |

### CC0-1.0 OR MIT-0 OR Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `constant_time_eq` | 0.4.2 | [link](https://github.com/cesarb/constant_time_eq) |
| `dunce` | 1.0.5 | [link](https://gitlab.com/kornelski/dunce) |

### CDLA-Permissive-2.0

| Crate | Version | Repository |
|---|---|---|
| `webpki-roots` | 0.26.11 | [link](https://github.com/rustls/webpki-roots) |
| `webpki-roots` | 1.0.7 | [link](https://github.com/rustls/webpki-roots) |

### ISC

| Crate | Version | Repository |
|---|---|---|
| `rustls-webpki` | 0.103.13 | [link](https://github.com/rustls/webpki) |
| `simple_asn1` | 0.6.4 | [link](https://github.com/acw/simple_asn1) |
| `untrusted` | 0.7.1 | [link](https://github.com/briansmith/untrusted) |
| `untrusted` | 0.9.0 | [link](https://github.com/briansmith/untrusted) |

### ISC AND (Apache-2.0 OR ISC)

| Crate | Version | Repository |
|---|---|---|
| `aws-lc-rs` | 1.17.0 | [link](https://github.com/aws/aws-lc-rs) |

### ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)

| Crate | Version | Repository |
|---|---|---|
| `aws-lc-sys` | 0.41.0 | [link](https://github.com/aws/aws-lc-rs) |

### LGPL-3.0-or-later

| Crate | Version | Repository |
|---|---|---|
| `ansi_colours` | 1.2.3 | [link](https://github.com/mina86/ansi_colours) |

### MIT

| Crate | Version | Repository |
|---|---|---|
| `async-stream` | 0.3.6 | [link](https://github.com/tokio-rs/async-stream) |
| `async-stream-impl` | 0.3.6 | [link](https://github.com/tokio-rs/async-stream) |
| `atoi` | 2.0.0 | [link](https://github.com/pacman82/atoi-rs) |
| `axum` | 0.8.9 | [link](https://github.com/tokio-rs/axum) |
| `axum-core` | 0.5.6 | [link](https://github.com/tokio-rs/axum) |
| `base64-simd` | 0.8.0 | [link](https://github.com/Nugine/simd) |
| `bincode` | 1.3.3 | [link](https://github.com/servo/bincode) |
| `bytes` | 1.11.1 | [link](https://github.com/tokio-rs/bytes) |
| `cfb` | 0.10.0 | [link](https://github.com/mdsteele/rust-cfb) |
| `cfg_aliases` | 0.2.1 | [link](https://github.com/katharostech/cfg_aliases) |
| `cliclack` | 0.5.4 | [link](https://github.com/fadeevab/cliclack) |
| `color_quant` | 1.1.0 | [link](https://github.com/image-rs/color_quant.git) |
| `comfy-table` | 7.2.2 | [link](https://github.com/nukesor/comfy-table) |
| `console` | 0.16.3 | [link](https://github.com/console-rs/console) |
| `const-str` | 1.1.0 | [link](https://github.com/Nugine/const-str) |
| `convert_case` | 0.10.0 | [link](https://github.com/rutrum/convert-case) |
| `core_maths` | 0.1.1 | [link](https://github.com/robertbastian/core_maths) |
| `countio` | 0.3.0 | [link](https://github.com/spire-rs/countio) |
| `croner` | 3.0.1 | [link](https://github.com/hexagon/croner-rust) |
| `darling` | 0.20.11 | [link](https://github.com/TedDriggs/darling) |
| `darling` | 0.23.0 | [link](https://github.com/TedDriggs/darling) |
| `darling_core` | 0.20.11 | [link](https://github.com/TedDriggs/darling) |
| `darling_core` | 0.23.0 | [link](https://github.com/TedDriggs/darling) |
| `darling_macro` | 0.20.11 | [link](https://github.com/TedDriggs/darling) |
| `darling_macro` | 0.23.0 | [link](https://github.com/TedDriggs/darling) |
| `data-encoding` | 2.11.0 | [link](https://github.com/ia0/data-encoding) |
| `derive_more` | 2.1.1 | [link](https://github.com/JelteF/derive_more) |
| `derive_more-impl` | 2.1.1 | [link](https://github.com/JelteF/derive_more) |
| `doc-comment` | 0.3.4 | [link](https://github.com/GuillaumeGomez/doc-comment) |
| `docx-rs` | 0.4.20 | [link](https://github.com/bokuweb/docx-rs) |
| `dotenvy` | 0.15.7 | [link](https://github.com/allan2/dotenvy) |
| `ecb` | 0.1.2 | [link](https://github.com/magic-akari/ecb) |
| `email_address` | 0.2.9 | [link](https://github.com/johnstonskj/rust-email_address.git) |
| `endian-type` | 0.2.0 | [link](https://github.com/Lolirofle/endian-type.git) |
| `fancy-regex` | 0.14.0 | [link](https://github.com/fancy-regex/fancy-regex) |
| `fancy-regex` | 0.17.0 | [link](https://github.com/fancy-regex/fancy-regex) |
| `fax` | 0.2.7 | [link](https://github.com/pdf-rs/fax) |
| `fluent-uri` | 0.3.2 | [link](https://github.com/yescallop/fluent-uri-rs) |
| `fs_extra` | 1.3.0 | [link](https://github.com/webdesus/fs_extra) |
| `generic-array` | 0.14.7 | [link](https://github.com/fizyk20/generic-array.git) |
| `h2` | 0.4.14 | [link](https://github.com/hyperium/h2) |
| `html_parser` | 0.7.0 | [link](https://github.com/mathiversen/html-parser) |
| `http-body` | 0.4.6 | [link](https://github.com/hyperium/http-body) |
| `http-body` | 1.0.1 | [link](https://github.com/hyperium/http-body) |
| `http-body-util` | 0.1.3 | [link](https://github.com/hyperium/http-body) |
| `hyper` | 1.9.0 | [link](https://github.com/hyperium/hyper) |
| `hyper-util` | 0.1.20 | [link](https://github.com/hyperium/hyper-util) |
| `imagesize` | 0.14.0 | [link](https://github.com/Roughsketch/imagesize) |
| `include_dir` | 0.7.4 | [link](https://github.com/Michael-F-Bryan/include_dir) |
| `include_dir_macros` | 0.7.4 | [link](https://github.com/Michael-F-Bryan/include_dir) |
| `indicatif` | 0.18.4 | [link](https://github.com/console-rs/indicatif) |
| `is-docker` | 0.2.0 | [link](https://github.com/TheLarkInn/is-docker) |
| `is-wsl` | 0.4.0 | [link](https://github.com/TheLarkInn/is-wsl) |
| `jsonschema` | 0.30.0 | [link](https://github.com/Stranger6667/jsonschema) |
| `jsonwebtoken` | 10.4.0 | [link](https://github.com/Keats/jsonwebtoken) |
| `libm` | 0.2.16 | [link](https://github.com/rust-lang/compiler-builtins) |
| `libsqlite3-sys` | 0.30.1 | [link](https://github.com/rusqlite/rusqlite) |
| `lopdf` | 0.41.0 | [link](https://github.com/J-F-Liu/lopdf.git) |
| `lru` | 0.18.0 | [link](https://github.com/jeromefroe/lru-rs.git) |
| `lz4_flex` | 0.13.1 | [link](https://github.com/pseitz/lz4_flex) |
| `matchers` | 0.2.0 | [link](https://github.com/hawkw/matchers) |
| `mime_guess` | 2.0.5 | [link](https://github.com/abonander/mime_guess) |
| `mio` | 1.2.0 | [link](https://github.com/tokio-rs/mio) |
| `nanoid` | 0.5.0 | [link](https://github.com/mrdimidium/nanoid.git) |
| `nibble_vec` | 0.1.0 | [link](https://github.com/michaelsproul/rust_nibble_vec) |
| `nix` | 0.31.3 | [link](https://github.com/nix-rust/nix) |
| `nom` | 8.0.0 | [link](https://github.com/rust-bakery/nom) |
| `nu-ansi-term` | 0.50.3 | [link](https://github.com/nushell/nu-ansi-term) |
| `onig` | 6.5.3 | [link](https://github.com/iwillspeak/rust-onig) |
| `onig_sys` | 69.9.3 | [link](https://github.com/rust-onig/rust-onig) |
| `open` | 5.3.5 | [link](https://github.com/Byron/open-rs) |
| `outref` | 0.5.2 | [link](https://github.com/Nugine/outref) |
| `pem` | 3.0.6 | [link](https://github.com/jcreekmore/pem-rs.git) |
| `phf` | 0.12.1 | [link](https://github.com/rust-phf/rust-phf) |
| `phf_shared` | 0.12.1 | [link](https://github.com/rust-phf/rust-phf) |
| `pulldown-cmark` | 0.13.4 | [link](https://github.com/raphlinus/pulldown-cmark) |
| `quick-xml` | 0.36.2 | [link](https://github.com/tafia/quick-xml) |
| `quick-xml` | 0.37.5 | [link](https://github.com/tafia/quick-xml) |
| `radix_trie` | 0.3.0 | [link](https://github.com/michaelsproul/rust_radix_trie) |
| `referencing` | 0.30.0 | [link](https://github.com/Stranger6667/jsonschema) |
| `rgb` | 0.8.53 | [link](https://github.com/kornelski/rust-rgb) |
| `rustyline` | 18.0.0 | [link](https://github.com/kkawakam/rustyline) |
| `safe-transmute` | 0.11.3 | [link](https://github.com/nabijaczleweli/safe-transmute-rs) |
| `schemars` | 1.2.1 | [link](https://github.com/GREsau/schemars) |
| `schemars_derive` | 1.2.1 | [link](https://github.com/GREsau/schemars) |
| `sha2-asm` | 0.6.4 | [link](https://github.com/RustCrypto/asm-hashes) |
| `sharded-slab` | 0.1.7 | [link](https://github.com/hawkw/sharded-slab) |
| `simd-adler32` | 0.3.9 | [link](https://github.com/mcountryman/simd-adler32) |
| `slab` | 0.4.12 | [link](https://github.com/tokio-rs/slab) |
| `smawk` | 0.3.2 | [link](https://github.com/mgeisler/smawk) |
| `spin` | 0.9.8 | [link](https://github.com/mvdnes/spin-rs.git) |
| `statrs` | 0.18.0 | [link](https://github.com/statrs-dev/statrs) |
| `std_prelude` | 0.2.12 | [link](https://github.com/vitiral/std_prelude) |
| `strsim` | 0.11.1 | [link](https://github.com/rapidfuzz/strsim-rs) |
| `strum` | 0.27.2 | [link](https://github.com/Peternator7/strum) |
| `strum` | 0.28.0 | [link](https://github.com/Peternator7/strum) |
| `strum_macros` | 0.27.2 | [link](https://github.com/Peternator7/strum) |
| `strum_macros` | 0.28.0 | [link](https://github.com/Peternator7/strum) |
| `synstructure` | 0.13.2 | [link](https://github.com/mystor/synstructure) |
| `syntect` | 5.3.0 | [link](https://github.com/trishume/syntect) |
| `sys-info` | 0.9.1 | [link](https://github.com/FillZpp/sys-info-rs) |
| `sysinfo` | 0.38.4 | [link](https://github.com/GuillaumeGomez/sysinfo) |
| `textwrap` | 0.16.2 | [link](https://github.com/mgeisler/textwrap) |
| `tiff` | 0.11.3 | [link](https://github.com/image-rs/image-tiff) |
| `tiff` | 0.9.1 | [link](https://github.com/image-rs/image-tiff) |
| `tiktoken-rs` | 0.11.0 | [link](https://github.com/zurawiki/tiktoken-rs) |
| `tokio` | 1.52.3 | [link](https://github.com/tokio-rs/tokio) |
| `tokio-macros` | 2.7.0 | [link](https://github.com/tokio-rs/tokio) |
| `tokio-retry` | 0.3.2 | [link](https://github.com/djc/tokio-retry) |
| `tokio-stream` | 0.1.18 | [link](https://github.com/tokio-rs/tokio) |
| `tokio-tungstenite` | 0.29.0 | [link](https://github.com/snapview/tokio-tungstenite) |
| `tokio-util` | 0.7.18 | [link](https://github.com/tokio-rs/tokio) |
| `tower` | 0.5.3 | [link](https://github.com/tower-rs/tower) |
| `tower-http` | 0.6.11 | [link](https://github.com/tower-rs/tower-http) |
| `tower-layer` | 0.3.3 | [link](https://github.com/tower-rs/tower) |
| `tower-service` | 0.3.3 | [link](https://github.com/tower-rs/tower) |
| `tracing` | 0.1.44 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-appender` | 0.2.5 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-attributes` | 0.1.31 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-core` | 0.1.36 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-futures` | 0.2.5 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-log` | 0.2.0 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-opentelemetry` | 0.33.0 | [link](https://github.com/tokio-rs/tracing-opentelemetry) |
| `tracing-serde` | 0.2.0 | [link](https://github.com/tokio-rs/tracing) |
| `tracing-subscriber` | 0.3.23 | [link](https://github.com/tokio-rs/tracing) |
| `tree-sitter` | 0.26.9 | [link](https://github.com/tree-sitter/tree-sitter) |
| `tree-sitter-go` | 0.25.0 | [link](https://github.com/tree-sitter/tree-sitter-go) |
| `tree-sitter-java` | 0.23.5 | [link](https://github.com/tree-sitter/tree-sitter-java) |
| `tree-sitter-javascript` | 0.25.0 | [link](https://github.com/tree-sitter/tree-sitter-javascript) |
| `tree-sitter-kotlin-ng` | 1.1.0 | [link](https://github.com/tree-sitter-grammars/tree-sitter-kotlin) |
| `tree-sitter-language` | 0.1.7 | [link](https://github.com/tree-sitter/tree-sitter) |
| `tree-sitter-python` | 0.25.0 | [link](https://github.com/tree-sitter/tree-sitter-python) |
| `tree-sitter-ruby` | 0.23.1 | [link](https://github.com/tree-sitter/tree-sitter-ruby) |
| `tree-sitter-rust` | 0.24.2 | [link](https://github.com/tree-sitter/tree-sitter-rust) |
| `tree-sitter-swift` | 0.7.3 | [link](https://github.com/alex-pinkus/tree-sitter-swift) |
| `tree-sitter-typescript` | 0.23.2 | [link](https://github.com/tree-sitter/tree-sitter-typescript) |
| `try-lock` | 0.2.5 | [link](https://github.com/seanmonstar/try-lock) |
| `twox-hash` | 2.1.0 | [link](https://github.com/shepmaster/twox-hash) |
| `umya-spreadsheet` | 2.3.3 | [link](https://github.com/MathNya/umya-spreadsheet) |
| `unit-prefix` | 0.5.2 | [link](https://codeberg.org/commons-rs/unit-prefix) |
| `unsafe-libyaml` | 0.2.11 | [link](https://github.com/dtolnay/unsafe-libyaml) |
| `urlencoding` | 2.1.3 | [link](https://github.com/kornelski/rust_urlencoding) |
| `uuid-simd` | 0.8.0 | [link](https://github.com/Nugine/simd) |
| `vsimd` | 0.8.0 | [link](https://github.com/Nugine/simd) |
| `want` | 0.3.1 | [link](https://github.com/seanmonstar/want) |
| `which` | 8.0.3 | [link](https://github.com/harryfei/which-rs.git) |
| `winnow` | 0.7.15 | [link](https://github.com/winnow-rs/winnow) |
| `winnow` | 1.0.3 | [link](https://github.com/winnow-rs/winnow) |
| `zip` | 0.6.6 | [link](https://github.com/zip-rs/zip.git) |
| `zip` | 2.4.2 | [link](https://github.com/zip-rs/zip2.git) |
| `zip` | 8.6.0 | [link](https://github.com/zip-rs/zip2) |
| `zmij` | 1.0.21 | [link](https://github.com/dtolnay/zmij) |
| `zstd` | 0.13.3 | [link](https://github.com/gyscos/zstd-rs) |

### MIT AND BSD-3-Clause

| Crate | Version | Repository |
|---|---|---|
| `matchit` | 0.8.4 | [link](https://github.com/ibraheemdev/matchit) |

### MIT OR Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `aes` | 0.8.4 | [link](https://github.com/RustCrypto/block-ciphers) |
| `ahash` | 0.8.12 | [link](https://github.com/tkaitchuck/ahash) |
| `allocator-api2` | 0.2.21 | [link](https://github.com/zakarumych/allocator-api2) |
| `anstream` | 1.0.0 | [link](https://github.com/rust-cli/anstyle.git) |
| `anstyle` | 1.0.14 | [link](https://github.com/rust-cli/anstyle.git) |
| `anstyle-parse` | 1.0.0 | [link](https://github.com/rust-cli/anstyle.git) |
| `anstyle-query` | 1.1.5 | [link](https://github.com/rust-cli/anstyle.git) |
| `anyhow` | 1.0.102 | [link](https://github.com/dtolnay/anyhow) |
| `arboard` | 3.6.1 | [link](https://github.com/1Password/arboard) |
| `arc-swap` | 1.9.1 | [link](https://github.com/vorner/arc-swap) |
| `arrayvec` | 0.7.6 | [link](https://github.com/bluss/arrayvec) |
| `async-compression` | 0.4.42 | [link](https://github.com/Nullus157/async-compression) |
| `async-trait` | 0.1.89 | [link](https://github.com/dtolnay/async-trait) |
| `base64` | 0.22.1 | [link](https://github.com/marshallpierce/rust-base64) |
| `bat` | 0.26.1 | [link](https://github.com/sharkdp/bat) |
| `bitflags` | 2.11.1 | [link](https://github.com/bitflags/bitflags) |
| `block-buffer` | 0.10.4 | [link](https://github.com/RustCrypto/utils) |
| `block-buffer` | 0.12.0 | [link](https://github.com/RustCrypto/utils) |
| `block-padding` | 0.3.3 | [link](https://github.com/RustCrypto/utils) |
| `bon` | 3.9.3 | [link](https://github.com/elastio/bon) |
| `bon-macros` | 3.9.3 | [link](https://github.com/elastio/bon) |
| `bstr` | 1.12.1 | [link](https://github.com/BurntSushi/bstr) |
| `bumpalo` | 3.20.3 | [link](https://github.com/fitzgen/bumpalo) |
| `bzip2` | 0.6.1 | [link](https://github.com/trifectatechfoundation/bzip2-rs) |
| `cbc` | 0.1.2 | [link](https://github.com/RustCrypto/block-modes) |
| `cc` | 1.2.62 | [link](https://github.com/rust-lang/cc-rs) |
| `cfg-if` | 1.0.4 | [link](https://github.com/rust-lang/cfg-if) |
| `chacha20` | 0.10.0 | [link](https://github.com/RustCrypto/stream-ciphers) |
| `chrono` | 0.4.45 | [link](https://github.com/chronotope/chrono) |
| `chrono-tz` | 0.10.4 | [link](https://github.com/chronotope/chrono-tz) |
| `cipher` | 0.4.4 | [link](https://github.com/RustCrypto/traits) |
| `clap` | 4.6.1 | [link](https://github.com/clap-rs/clap) |
| `clap_builder` | 4.6.0 | [link](https://github.com/clap-rs/clap) |
| `clap_complete` | 4.6.5 | [link](https://github.com/clap-rs/clap) |
| `clap_complete_nushell` | 4.6.0 | [link](https://github.com/clap-rs/clap) |
| `clap_derive` | 4.6.1 | [link](https://github.com/clap-rs/clap) |
| `clap_lex` | 1.1.0 | [link](https://github.com/clap-rs/clap) |
| `clap_mangen` | 0.3.0 | [link](https://github.com/clap-rs/clap) |
| `clircle` | 0.6.1 | [link](https://github.com/niklasmohrin/clircle) |
| `cmake` | 0.1.58 | [link](https://github.com/rust-lang/cmake-rs) |
| `colorchoice` | 1.0.5 | [link](https://github.com/rust-cli/anstyle.git) |
| `compression-codecs` | 0.4.38 | [link](https://github.com/Nullus157/async-compression) |
| `compression-core` | 0.4.32 | [link](https://github.com/Nullus157/async-compression) |
| `cookie` | 0.18.1 | [link](https://github.com/SergioBenitez/cookie-rs) |
| `cookie_store` | 0.22.1 | [link](https://github.com/pfernie/cookie_store) |
| `cpufeatures` | 0.2.17 | [link](https://github.com/RustCrypto/utils) |
| `cpufeatures` | 0.3.0 | [link](https://github.com/RustCrypto/utils) |
| `crc` | 3.4.0 | [link](https://github.com/mrhooray/crc-rs.git) |
| `crc-catalog` | 2.5.0 | [link](https://github.com/akhilles/crc-catalog.git) |
| `crc32fast` | 1.5.0 | [link](https://github.com/srijs/rust-crc32fast) |
| `crossbeam-channel` | 0.5.15 | [link](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-deque` | 0.8.6 | [link](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-epoch` | 0.9.18 | [link](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-queue` | 0.3.12 | [link](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-utils` | 0.8.21 | [link](https://github.com/crossbeam-rs/crossbeam) |
| `crypto-common` | 0.1.7 | [link](https://github.com/RustCrypto/traits) |
| `crypto-common` | 0.2.2 | [link](https://github.com/RustCrypto/traits) |
| `deranged` | 0.5.8 | [link](https://github.com/jhpratt/deranged) |
| `derive_builder` | 0.20.2 | [link](https://github.com/colin-kiegel/rust-derive-builder) |
| `derive_builder_core` | 0.20.2 | [link](https://github.com/colin-kiegel/rust-derive-builder) |
| `derive_builder_macro` | 0.20.2 | [link](https://github.com/colin-kiegel/rust-derive-builder) |
| `digest` | 0.10.7 | [link](https://github.com/RustCrypto/traits) |
| `digest` | 0.11.3 | [link](https://github.com/RustCrypto/traits) |
| `dirs` | 6.0.0 | [link](https://github.com/soc/dirs-rs) |
| `dirs-sys` | 0.5.0 | [link](https://github.com/dirs-dev/dirs-sys-rs) |
| `displaydoc` | 0.2.5 | [link](https://github.com/yaahc/displaydoc) |
| `document-features` | 0.2.12 | [link](https://github.com/slint-ui/document-features) |
| `dyn-clone` | 1.0.20 | [link](https://github.com/dtolnay/dyn-clone) |
| `either` | 1.16.0 | [link](https://github.com/rayon-rs/either) |
| `enumflags2` | 0.7.12 | [link](https://github.com/meithecatte/enumflags2) |
| `enumflags2_derive` | 0.7.12 | [link](https://github.com/meithecatte/enumflags2) |
| `errno` | 0.3.14 | [link](https://github.com/lambda-fairy/rust-errno) |
| `etcetera` | 0.11.0 | [link](https://github.com/lunacookies/etcetera) |
| `fdeflate` | 0.3.7 | [link](https://github.com/image-rs/fdeflate) |
| `find-msvc-tools` | 0.1.9 | [link](https://github.com/rust-lang/cc-rs) |
| `fixedbitset` | 0.5.7 | [link](https://github.com/petgraph/fixedbitset) |
| `flate2` | 1.1.9 | [link](https://github.com/rust-lang/flate2-rs) |
| `form_urlencoded` | 1.2.2 | [link](https://github.com/servo/rust-url) |
| `fraction` | 0.15.4 | [link](https://github.com/dnsl48/fraction.git) |
| `fs-err` | 3.3.0 | [link](https://github.com/andrewhickman/fs-err) |
| `futures` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-channel` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-concurrency` | 7.7.1 | [link](https://github.com/yoshuawuyts/futures-concurrency) |
| `futures-core` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-executor` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-intrusive` | 0.5.0 | [link](https://github.com/Matthias247/futures-intrusive) |
| `futures-io` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-macro` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-sink` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-task` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `futures-util` | 0.3.32 | [link](https://github.com/rust-lang/futures-rs) |
| `gearhash` | 0.1.3 | [link](https://github.com/srijs/rust-gearhash) |
| `getrandom` | 0.2.17 | [link](https://github.com/rust-random/getrandom) |
| `getrandom` | 0.3.4 | [link](https://github.com/rust-random/getrandom) |
| `getrandom` | 0.4.2 | [link](https://github.com/rust-random/getrandom) |
| `gif` | 0.13.3 | [link](https://github.com/image-rs/image-gif) |
| `gif` | 0.14.2 | [link](https://github.com/image-rs/image-gif) |
| `half` | 2.7.1 | [link](https://github.com/VoidStarKat/half-rs) |
| `hashbrown` | 0.15.5 | [link](https://github.com/rust-lang/hashbrown) |
| `hashbrown` | 0.17.1 | [link](https://github.com/rust-lang/hashbrown) |
| `hashlink` | 0.10.0 | [link](https://github.com/kyren/hashlink) |
| `heapify` | 0.2.0 | [link](https://github.com/ethereal-sheep/heapify) |
| `heck` | 0.5.0 | [link](https://github.com/withoutboats/heck) |
| `hex` | 0.4.3 | [link](https://github.com/KokaKiwi/rust-hex) |
| `hmac` | 0.12.1 | [link](https://github.com/RustCrypto/MACs) |
| `hmac` | 0.13.0 | [link](https://github.com/RustCrypto/MACs) |
| `home` | 0.5.12 | [link](https://github.com/rust-lang/cargo) |
| `http` | 0.2.12 | [link](https://github.com/hyperium/http) |
| `http` | 1.4.2 | [link](https://github.com/hyperium/http) |
| `httparse` | 1.10.1 | [link](https://github.com/seanmonstar/httparse) |
| `httpdate` | 1.0.3 | [link](https://github.com/pyfisch/httpdate) |
| `humantime` | 2.3.0 | [link](https://github.com/chronotope/humantime) |
| `hybrid-array` | 0.4.12 | [link](https://github.com/RustCrypto/hybrid-array) |
| `iana-time-zone` | 0.1.65 | [link](https://github.com/strawlab/iana-time-zone) |
| `idna` | 1.1.0 | [link](https://github.com/servo/rust-url/) |
| `image` | 0.24.9 | [link](https://github.com/image-rs/image) |
| `image` | 0.25.10 | [link](https://github.com/image-rs/image) |
| `indoc` | 2.0.7 | [link](https://github.com/dtolnay/indoc) |
| `inout` | 0.1.4 | [link](https://github.com/RustCrypto/utils) |
| `ipnet` | 2.12.0 | [link](https://github.com/krisprice/ipnet) |
| `is_terminal_polyfill` | 1.70.2 | [link](https://github.com/polyfill-rs/is_terminal_polyfill) |
| `itertools` | 0.14.0 | [link](https://github.com/rust-itertools/itertools) |
| `itoa` | 1.0.18 | [link](https://github.com/dtolnay/itoa) |
| `jobserver` | 0.1.34 | [link](https://github.com/rust-lang/jobserver-rs) |
| `jpeg-decoder` | 0.3.2 | [link](https://github.com/image-rs/jpeg-decoder) |
| `jsonrpcmsg` | 0.1.2 | [link](https://github.com/fruafr/rust-jsonrpcmsg) |
| `landlock` | 0.4.5 | [link](https://github.com/landlock-lsm/rust-landlock) |
| `lazy_static` | 1.5.0 | [link](https://github.com/rust-lang-nursery/lazy-static.rs) |
| `libc` | 0.2.186 | [link](https://github.com/rust-lang/libc) |
| `litrs` | 1.0.0 | [link](https://github.com/LukasKalbertodt/litrs) |
| `lock_api` | 0.4.14 | [link](https://github.com/Amanieu/parking_lot) |
| `log` | 0.4.30 | [link](https://github.com/rust-lang/log) |
| `md-5` | 0.10.6 | [link](https://github.com/RustCrypto/hashes) |
| `mime` | 0.3.17 | [link](https://github.com/hyperium/mime) |
| `num` | 0.4.3 | [link](https://github.com/rust-num/num) |
| `num-bigint` | 0.4.6 | [link](https://github.com/rust-num/num-bigint) |
| `num-complex` | 0.4.6 | [link](https://github.com/rust-num/num-complex) |
| `num-conv` | 0.2.2 | [link](https://github.com/jhpratt/num-conv) |
| `num-derive` | 0.4.2 | [link](https://github.com/rust-num/num-derive) |
| `num-integer` | 0.1.46 | [link](https://github.com/rust-num/num-integer) |
| `num-iter` | 0.1.45 | [link](https://github.com/rust-num/num-iter) |
| `num-rational` | 0.4.2 | [link](https://github.com/rust-num/num-rational) |
| `num-traits` | 0.2.19 | [link](https://github.com/rust-num/num-traits) |
| `oauth2` | 5.0.0 | [link](https://github.com/ramosbugs/oauth2-rs) |
| `once_cell` | 1.21.4 | [link](https://github.com/matklad/once_cell) |
| `oneshot` | 0.1.13 | [link](https://github.com/faern/oneshot) |
| `openssl-probe` | 0.2.1 | [link](https://github.com/rustls/openssl-probe) |
| `os_str_bytes` | 6.6.1 | [link](https://github.com/dylni/os_str_bytes) |
| `parking_lot` | 0.12.5 | [link](https://github.com/Amanieu/parking_lot) |
| `parking_lot_core` | 0.9.12 | [link](https://github.com/Amanieu/parking_lot) |
| `pastey` | 0.2.3 | [link](https://github.com/as1100k/pastey) |
| `path_abs` | 0.5.1 | [link](https://github.com/vitiral/path_abs) |
| `percent-encoding` | 2.3.2 | [link](https://github.com/servo/rust-url/) |
| `pest` | 2.8.6 | [link](https://github.com/pest-parser/pest) |
| `pest_derive` | 2.8.6 | [link](https://github.com/pest-parser/pest) |
| `pest_generator` | 2.8.6 | [link](https://github.com/pest-parser/pest) |
| `pest_meta` | 2.8.6 | [link](https://github.com/pest-parser/pest) |
| `pin-utils` | 0.1.0 | [link](https://github.com/rust-lang-nursery/pin-utils) |
| `pkg-config` | 0.3.33 | [link](https://github.com/rust-lang/pkg-config-rs) |
| `png` | 0.17.16 | [link](https://github.com/image-rs/image-png) |
| `png` | 0.18.1 | [link](https://github.com/image-rs/image-png) |
| `powerfmt` | 0.2.0 | [link](https://github.com/jhpratt/powerfmt) |
| `ppv-lite86` | 0.2.21 | [link](https://github.com/cryptocorrosion/cryptocorrosion) |
| `prettyplease` | 0.2.37 | [link](https://github.com/dtolnay/prettyplease) |
| `proc-macro-error` | 1.0.4 | [link](https://gitlab.com/CreepySkeleton/proc-macro-error) |
| `proc-macro-error-attr` | 1.0.4 | [link](https://gitlab.com/CreepySkeleton/proc-macro-error) |
| `proc-macro2` | 1.0.106 | [link](https://github.com/dtolnay/proc-macro2) |
| `quote` | 1.0.45 | [link](https://github.com/dtolnay/quote) |
| `rand` | 0.10.1 | [link](https://github.com/rust-random/rand) |
| `rand` | 0.8.6 | [link](https://github.com/rust-random/rand) |
| `rand` | 0.9.4 | [link](https://github.com/rust-random/rand) |
| `rand_chacha` | 0.3.1 | [link](https://github.com/rust-random/rand) |
| `rand_chacha` | 0.9.0 | [link](https://github.com/rust-random/rand) |
| `rand_core` | 0.10.1 | [link](https://github.com/rust-random/rand_core) |
| `rand_core` | 0.6.4 | [link](https://github.com/rust-random/rand) |
| `rand_core` | 0.9.5 | [link](https://github.com/rust-random/rand) |
| `rayon` | 1.12.0 | [link](https://github.com/rayon-rs/rayon) |
| `rayon-core` | 1.13.0 | [link](https://github.com/rayon-rs/rayon) |
| `redb` | 3.1.3 | [link](https://github.com/cberner/redb) |
| `ref-cast` | 1.0.25 | [link](https://github.com/dtolnay/ref-cast) |
| `ref-cast-impl` | 1.0.25 | [link](https://github.com/dtolnay/ref-cast) |
| `regex` | 1.12.4 | [link](https://github.com/rust-lang/regex) |
| `regex-automata` | 0.4.14 | [link](https://github.com/rust-lang/regex) |
| `regex-lite` | 0.1.9 | [link](https://github.com/rust-lang/regex) |
| `regex-syntax` | 0.8.11 | [link](https://github.com/rust-lang/regex) |
| `reqwest` | 0.12.28 | [link](https://github.com/seanmonstar/reqwest) |
| `reqwest` | 0.13.4 | [link](https://github.com/seanmonstar/reqwest) |
| `reqwest-middleware` | 0.5.2 | [link](https://github.com/TrueLayer/reqwest-middleware) |
| `roff` | 1.1.1 | [link](https://github.com/rust-cli/roff-rs) |
| `rustc_version` | 0.4.1 | [link](https://github.com/djc/rustc-version-rs) |
| `rustls-pki-types` | 1.14.1 | [link](https://github.com/rustls/pki-types) |
| `rustls-platform-verifier` | 0.7.0 | [link](https://github.com/rustls/rustls-platform-verifier) |
| `rustversion` | 1.0.22 | [link](https://github.com/dtolnay/rustversion) |
| `scopeguard` | 1.2.0 | [link](https://github.com/bluss/scopeguard) |
| `semver` | 1.0.28 | [link](https://github.com/dtolnay/semver) |
| `serde` | 1.0.228 | [link](https://github.com/serde-rs/serde) |
| `serde_core` | 1.0.228 | [link](https://github.com/serde-rs/serde) |
| `serde_derive` | 1.0.228 | [link](https://github.com/serde-rs/serde) |
| `serde_derive_internals` | 0.29.1 | [link](https://github.com/serde-rs/serde) |
| `serde_json` | 1.0.150 | [link](https://github.com/serde-rs/json) |
| `serde_path_to_error` | 0.1.20 | [link](https://github.com/dtolnay/path-to-error) |
| `serde_repr` | 0.1.20 | [link](https://github.com/dtolnay/serde-repr) |
| `serde_spanned` | 1.1.1 | [link](https://github.com/toml-rs/toml) |
| `serde_with` | 3.20.0 | [link](https://github.com/jonasbb/serde_with/) |
| `serde_with_macros` | 3.20.0 | [link](https://github.com/jonasbb/serde_with/) |
| `serde_yaml` | 0.9.34+deprecated | [link](https://github.com/dtolnay/serde-yaml) |
| `sha1` | 0.10.6 | [link](https://github.com/RustCrypto/hashes) |
| `sha2` | 0.10.9 | [link](https://github.com/RustCrypto/hashes) |
| `sha2` | 0.11.0 | [link](https://github.com/RustCrypto/hashes) |
| `shlex` | 1.3.0 | [link](https://github.com/comex/rust-shlex) |
| `shlex` | 2.0.1 | [link](https://github.com/comex/rust-shlex) |
| `signal-hook-registry` | 1.4.8 | [link](https://github.com/vorner/signal-hook) |
| `smallvec` | 1.15.1 | [link](https://github.com/servo/rust-smallvec) |
| `socket2` | 0.6.4 | [link](https://github.com/rust-lang/socket2) |
| `sqlx` | 0.8.6 | [link](https://github.com/launchbadge/sqlx) |
| `sqlx-core` | 0.8.6 | [link](https://github.com/launchbadge/sqlx) |
| `sqlx-macros` | 0.8.6 | [link](https://github.com/launchbadge/sqlx) |
| `sqlx-macros-core` | 0.8.6 | [link](https://github.com/launchbadge/sqlx) |
| `sqlx-sqlite` | 0.8.6 | [link](https://github.com/launchbadge/sqlx) |
| `stable_deref_trait` | 1.2.1 | [link](https://github.com/storyyeller/stable_deref_trait) |
| `static_assertions` | 1.1.0 | [link](https://github.com/nvzqz/static-assertions-rs) |
| `streaming-iterator` | 0.1.9 | [link](https://github.com/sfackler/streaming-iterator) |
| `syn` | 1.0.109 | [link](https://github.com/dtolnay/syn) |
| `syn` | 2.0.117 | [link](https://github.com/dtolnay/syn) |
| `tar` | 0.4.46 | [link](https://github.com/composefs/tar-rs) |
| `tempfile` | 3.27.0 | [link](https://github.com/Stebalien/tempfile) |
| `terminal-colorsaurus` | 1.0.3 | [link](https://github.com/tautropfli/terminal-colorsaurus) |
| `terminal-trx` | 0.2.6 | [link](https://github.com/tautropfli/terminal-trx) |
| `thin-vec` | 0.2.18 | [link](https://github.com/mozilla/thin-vec) |
| `thiserror` | 1.0.69 | [link](https://github.com/dtolnay/thiserror) |
| `thiserror` | 2.0.18 | [link](https://github.com/dtolnay/thiserror) |
| `thiserror-impl` | 1.0.69 | [link](https://github.com/dtolnay/thiserror) |
| `thiserror-impl` | 2.0.18 | [link](https://github.com/dtolnay/thiserror) |
| `thread_local` | 1.1.9 | [link](https://github.com/Amanieu/thread_local-rs) |
| `time` | 0.3.47 | [link](https://github.com/time-rs/time) |
| `time-core` | 0.1.8 | [link](https://github.com/time-rs/time) |
| `time-macros` | 0.2.27 | [link](https://github.com/time-rs/time) |
| `tokio-rustls` | 0.26.4 | [link](https://github.com/rustls/tokio-rustls) |
| `toml` | 0.9.12+spec-1.1.0 | [link](https://github.com/toml-rs/toml) |
| `toml_datetime` | 0.7.5+spec-1.1.0 | [link](https://github.com/toml-rs/toml) |
| `toml_parser` | 1.1.2+spec-1.1.0 | [link](https://github.com/toml-rs/toml) |
| `toml_writer` | 1.1.1+spec-1.1.0 | [link](https://github.com/toml-rs/toml) |
| `ttf-parser` | 0.25.1 | [link](https://github.com/harfbuzz/ttf-parser) |
| `tungstenite` | 0.29.0 | [link](https://github.com/snapview/tungstenite-rs) |
| `typed-path` | 0.12.3 | [link](https://github.com/chipsenkbeil/typed-path) |
| `typenum` | 1.20.0 | [link](https://github.com/paholg/typenum) |
| `ucd-trie` | 0.1.7 | [link](https://github.com/BurntSushi/ucd-generate) |
| `unicase` | 2.9.0 | [link](https://github.com/seanmonstar/unicase) |
| `unicode-bidi` | 0.3.18 | [link](https://github.com/servo/unicode-bidi) |
| `unicode-normalization` | 0.1.25 | [link](https://github.com/unicode-rs/unicode-normalization) |
| `unicode-segmentation` | 1.13.2 | [link](https://github.com/unicode-rs/unicode-segmentation) |
| `unicode-width` | 0.2.2 | [link](https://github.com/unicode-rs/unicode-width) |
| `unicode-xid` | 0.2.6 | [link](https://github.com/unicode-rs/unicode-xid) |
| `url` | 2.5.8 | [link](https://github.com/servo/rust-url) |
| `utoipa` | 4.2.3 | [link](https://github.com/juhaku/utoipa) |
| `utoipa-gen` | 4.3.1 | [link](https://github.com/juhaku/utoipa) |
| `webbrowser` | 1.2.1 | [link](https://github.com/amodm/webbrowser-rs) |
| `weezl` | 0.1.12 | [link](https://github.com/image-rs/weezl) |
| `x11rb` | 0.13.2 | [link](https://github.com/psychon/x11rb) |
| `x11rb-protocol` | 0.13.2 | [link](https://github.com/psychon/x11rb) |
| `xterm-color` | 1.0.2 | [link](https://github.com/tautropfli/terminal-colorsaurus) |
| `zstd-safe` | 7.2.4 | [link](https://github.com/gyscos/zstd-rs) |

### MIT OR Apache-2.0 OR Zlib

| Crate | Version | Repository |
|---|---|---|
| `tinyvec_macros` | 0.1.1 | [link](https://github.com/Soveu/tinyvec_macros) |
| `zune-core` | 0.5.1 | [link](https://github.com/etemesi254/zune-image) |
| `zune-inflate` | 0.2.54 |  |
| `zune-jpeg` | 0.5.15 | [link](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) |

### MIT OR Zlib OR Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `miniz_oxide` | 0.8.9 | [link](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |

### MIT-0

| Crate | Version | Repository |
|---|---|---|
| `borrow-or-share` | 0.2.4 | [link](https://github.com/yescallop/borrow-or-share) |

### MIT/Apache-2.0

| Crate | Version | Repository |
|---|---|---|
| `bitflags` | 1.3.2 | [link](https://github.com/bitflags/bitflags) |
| `cfg-if` | 0.1.10 | [link](https://github.com/alexcrichton/cfg-if) |
| `content_inspector` | 0.2.4 | [link](https://github.com/sharkdp/content_inspector) |
| `filetime` | 0.2.29 | [link](https://github.com/alexcrichton/filetime) |
| `fs2` | 0.4.3 | [link](https://github.com/danburkert/fs2-rs) |
| `ident_case` | 1.0.1 | [link](https://github.com/TedDriggs/ident_case) |
| `num-cmp` | 0.1.0 | [link](https://github.com/lifthrasiir/num-cmp) |
| `pathdiff` | 0.2.3 | [link](https://github.com/Manishearth/pathdiff) |
| `psl-types` | 2.0.11 | [link](https://github.com/addr-rs/psl-types) |
| `publicsuffix` | 2.3.0 | [link](https://github.com/rushmorem/publicsuffix) |
| `quick-error` | 2.0.1 | [link](http://github.com/tailhook/quick-error) |
| `rangemap` | 1.7.1 | [link](https://github.com/jeffparsons/rangemap) |
| `serde_urlencoded` | 0.7.1 | [link](https://github.com/nox/serde_urlencoded) |
| `shell-words` | 1.1.1 | [link](https://github.com/tmiasko/shell-words) |
| `shellexpand` | 3.1.2 | [link](https://gitlab.com/ijackson/rust-shellexpand) |
| `siphasher` | 1.0.3 | [link](https://github.com/jedisct1/rust-siphash) |
| `sse-stream` | 0.2.3 | [link](https://github.com/4t145/sse-stream/) |
| `stringprep` | 0.1.5 | [link](https://github.com/sfackler/rust-stringprep) |
| `symlink` | 0.1.0 | [link](https://gitlab.com/chris-morgan/symlink) |
| `thousands` | 0.2.0 | [link](https://github.com/tov/thousands-rs) |
| `tokio-cron-scheduler` | 0.15.1 | [link](https://github.com/mvniekerk/tokio-cron-scheduler) |
| `unicode-properties` | 0.1.4 | [link](https://github.com/unicode-rs/unicode-properties) |
| `v_escape-base` | 0.1.0 | [link](https://github.com/zzau13/v_escape) |
| `v_htmlescape` | 0.17.0 | [link](https://github.com/zzau13/v_escape) |
| `vcpkg` | 0.2.15 | [link](https://github.com/mcgoo/vcpkg-rs) |
| `version_check` | 0.9.5 | [link](https://github.com/SergioBenitez/version_check) |
| `xmlparser` | 0.13.6 | [link](https://github.com/RazrFalcon/xmlparser) |
| `zstd-sys` | 2.0.16+zstd.1.5.7 | [link](https://github.com/gyscos/zstd-rs) |

### MPL-2.0

| Crate | Version | Repository |
|---|---|---|
| `colored` | 3.1.1 | [link](https://github.com/mackwic/colored) |
| `option-ext` | 0.2.0 | [link](https://github.com/soc/option-ext.git) |

### Unicode-3.0

| Crate | Version | Repository |
|---|---|---|
| `icu_calendar` | 2.1.1 | [link](https://github.com/unicode-org/icu4x) |
| `icu_collections` | 2.1.1 | [link](https://github.com/unicode-org/icu4x) |
| `icu_locale` | 2.1.1 | [link](https://github.com/unicode-org/icu4x) |
| `icu_locale_core` | 2.2.0 | [link](https://github.com/unicode-org/icu4x) |
| `icu_normalizer` | 2.1.1 | [link](https://github.com/unicode-org/icu4x) |
| `icu_normalizer_data` | 2.1.1 | [link](https://github.com/unicode-org/icu4x) |
| `icu_properties` | 2.1.2 | [link](https://github.com/unicode-org/icu4x) |
| `icu_properties_data` | 2.1.2 | [link](https://github.com/unicode-org/icu4x) |
| `icu_provider` | 2.2.0 | [link](https://github.com/unicode-org/icu4x) |
| `litemap` | 0.8.2 | [link](https://github.com/unicode-org/icu4x) |
| `potential_utf` | 0.1.5 | [link](https://github.com/unicode-org/icu4x) |
| `tinystr` | 0.8.3 | [link](https://github.com/unicode-org/icu4x) |
| `writeable` | 0.6.3 | [link](https://github.com/unicode-org/icu4x) |
| `yoke` | 0.8.2 | [link](https://github.com/unicode-org/icu4x) |
| `yoke-derive` | 0.8.2 | [link](https://github.com/unicode-org/icu4x) |
| `zerofrom` | 0.1.8 | [link](https://github.com/unicode-org/icu4x) |
| `zerofrom-derive` | 0.1.7 | [link](https://github.com/unicode-org/icu4x) |
| `zerotrie` | 0.2.4 | [link](https://github.com/unicode-org/icu4x) |
| `zerovec` | 0.11.6 | [link](https://github.com/unicode-org/icu4x) |
| `zerovec-derive` | 0.11.3 | [link](https://github.com/unicode-org/icu4x) |

### Unlicense OR MIT

| Crate | Version | Repository |
|---|---|---|
| `aho-corasick` | 1.1.4 | [link](https://github.com/BurntSushi/aho-corasick) |
| `byteorder` | 1.5.0 | [link](https://github.com/BurntSushi/byteorder) |
| `byteorder-lite` | 0.1.0 | [link](https://github.com/image-rs/byteorder-lite) |
| `globset` | 0.4.18 | [link](https://github.com/BurntSushi/ripgrep/tree/master/crates/globset) |
| `ignore` | 0.4.26 | [link](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) |
| `memchr` | 2.8.0 | [link](https://github.com/BurntSushi/memchr) |

### Unlicense OR MIT OR Apache-2.0 OR CC0-1.0

| Crate | Version | Repository |
|---|---|---|
| `more-asserts` | 0.3.1 | [link](https://github.com/thomcc/rust-more-asserts) |

### Unlicense/MIT

| Crate | Version | Repository |
|---|---|---|
| `csv` | 1.4.0 | [link](https://github.com/BurntSushi/rust-csv) |
| `csv-core` | 0.1.13 | [link](https://github.com/BurntSushi/rust-csv) |
| `same-file` | 1.0.6 | [link](https://github.com/BurntSushi/same-file) |
| `walkdir` | 2.5.0 | [link](https://github.com/BurntSushi/walkdir) |

### Zlib

| Crate | Version | Repository |
|---|---|---|
| `const_panic` | 0.2.15 | [link](https://github.com/rodrimati1992/const_panic/) |
| `foldhash` | 0.1.5 | [link](https://github.com/orlp/foldhash) |
| `konst` | 0.4.3 | [link](https://github.com/rodrimati1992/konst/) |
| `konst_proc_macros` | 0.4.1 | [link](https://github.com/rodrimati1992/konst/) |
| `typewit` | 1.15.2 | [link](https://github.com/rodrimati1992/typewit/) |
| `zlib-rs` | 0.6.3 | [link](https://github.com/trifectatechfoundation/zlib-rs) |

### Zlib OR Apache-2.0 OR MIT

| Crate | Version | Repository |
|---|---|---|
| `bytemuck` | 1.25.0 | [link](https://github.com/Lokathor/bytemuck) |
| `tinyvec` | 1.11.0 | [link](https://github.com/Lokathor/tinyvec) |

