# 17 — Offline dependency plan

The task container has **no network** at run time and **no `apt` installation** (hub
correction). Everything the build and the test suite need must be present in the image.

## Toolchain

Pinned exactly, not by channel name:

- `rust 1.96.0` — matches the toolchain that resolved the versions below on this machine
  (`rustc 1.96.0 (ac68faa20 2026-05-25)`), so the lockfile is known to resolve
- `rust-toolchain.toml` in the package pins `channel = "1.96.0"` and
  `components = ["rustfmt", "clippy"]` so the container cannot silently drift to a newer
  toolchain

The base image is **digest-pinned** (hub correction). A tag alone is not acceptable; the
`FROM` line carries `@sha256:…`. The exact digest is resolved at build time and recorded in
the package once the official guide fixes the Dockerfile format.

## Runtime dependencies — exact pins

Versions are the ones already resolved in the source repository's lockfile, so they are
known to compile together under 1.96.0. All are `=`-pinned (exact), not caret ranges.

| Crate | Pin | Why needed | Features |
|---|---|---|---|
| `tokio` | `=1.48.0` | async traits on the ports; `current_thread` runtime for determinism | `rt`, `macros`, `sync` only |
| `async-trait` | `=0.1.89` | `async fn` in the five port traits | default |
| `serde` | `=1.0.228` | `Snapshot` round-trip through `StateStore` | `derive` |
| `serde_json` | `=1.0.146` | snapshot encoding in `FakeStateStore` | default |
| `thiserror` | `=1.0.69` | `PortError`, `SubmitError`, `EngineError` | default |

Five crates. Everything else in the source repository is excluded with its subsystem
(doc 03).

### Deliberate removals

- **`tokio` features `full` → `rt, macros, sync`.** `full` drags in `net`, `fs`, `process`,
  and `signal`. None are used, and `net` in particular is exactly what must not be
  reachable. The narrow feature set makes "this task cannot make a network call" a
  structural property rather than a promise.
- **`chrono`** — replaced by integer-millisecond `Timestamp` / `Duration` newtypes (doc 10).
  Removes a locale and leap-second surface, removes any temptation to call `Utc::now()`, and
  drops a transitive tree.
- **`anyhow`** — the task uses concrete error enums so the verifier can assert on variants.
  `anyhow`'s type erasure is the wrong shape here.
- **`rand`** — the seeded PRNG for V13 is a small deterministic generator implemented in the
  harness. A dozen lines of a fixed algorithm is more reproducible across crate versions
  than depending on `rand`'s generator internals, which have changed representation between
  0.8 and 0.9. It also avoids shipping a crate whose presence might suggest the engine is
  allowed nondeterminism.
- **`reqwest`, `ethers`, `tokio-tungstenite`, `dotenv`, `teloxide`** — all network, chain,
  and messaging surface. Excluded with their subsystems.

### Dev-dependencies

None. The suite uses the built-in test harness. No `proptest`, no `criterion`, no
`tokio-test` — `#[tokio::test(flavor = "current_thread")]` from the pinned `tokio` `macros`
feature is sufficient.

## Vendoring

The package vendors its dependency sources so the build is offline and reproducible:

- `cargo vendor` output committed under a vendor directory
- `.cargo/config.toml` with a `source.crates-io` replacement pointing at it
- `Cargo.lock` committed
- `CARGO_NET_OFFLINE=true` set in the image

This makes `cargo build --offline` and `cargo test --offline` succeed with no registry
access. Size is modest — five direct crates with `tokio` on a reduced feature set.

**Alternative if the official guide forbids vendoring:** pre-populate
`$CARGO_HOME/registry` in the image during the build stage (which does have network) and
rely on the committed lockfile. Same offline guarantee, different mechanism. The choice is
deferred to the guide.

## Verification before packaging

The plan is only credible if checked. Before the ZIP is built:

1. `cargo build --offline --locked` from a clean target directory
2. `cargo test --offline --locked` — full suite
3. run the container with networking disabled and repeat both
4. confirm no `apt-get`, `apt`, `pip`, `npm`, or `curl` appears in the Dockerfile
5. confirm the `FROM` line carries a digest
6. confirm `Cargo.lock` is committed and `--locked` does not modify it
7. run the suite twice and diff the output — byte-identical, since no clock and no entropy
   enter it

## Build time

Five crates on a reduced feature set builds in well under the `[agent].timeout_sec = 7200`
budget with room to spare. The image can pre-build dependencies so the agent's first
`cargo test` is fast — a slow first build wastes agent budget on compilation rather than on
the problem.
