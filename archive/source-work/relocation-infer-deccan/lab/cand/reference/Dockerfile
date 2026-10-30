# Task environment for the relocation-recovery task.
#
# The base image is pinned by digest, not by tag, so the toolchain cannot
# change under a task that must stay reproducible for as long as it is used.
#
# Nothing is installed here. The crate has no third-party dependencies, so the
# image needs no package-manager step and no network access at build time.
#
# This image never contains the verifier, the hidden corpora or the reference
# solution. The verifier runs in a separate image and mounts what it needs.

FROM rust@sha256:540c902e99c384163b688bbd8b5b8520e94e7731b27f7bd0eaa56ae1960627ab

ENV CARGO_NET_OFFLINE=true
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

WORKDIR /app

# Copied explicitly, path by path. A blanket `COPY . /app` would pick up
# whatever sits beside this file, which is how a solution ends up shipped.
COPY Cargo.toml /app/Cargo.toml
COPY Cargo.lock /app/Cargo.lock
COPY src /app/src
COPY spec /app/spec
COPY corpus /app/corpus

# Warms the build so the first `cargo build` in the session does not pay for a
# cold compile of the frozen modules; the placeholder compiles, so this proves
# the environment is usable before anyone touches it.
RUN cargo build --offline --release && cargo build --offline

CMD ["/bin/bash"]
