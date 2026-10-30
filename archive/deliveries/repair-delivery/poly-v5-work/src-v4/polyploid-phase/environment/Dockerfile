
# Task environment for the autotetraploid phasing task.
#
# The base image is pinned by digest, not by tag. A tag can be repointed at new
# content, which would silently change the toolchain under a task that is meant
# to be reproducible for as long as it is used.
#
# Nothing is installed here. The crate has no third party dependencies, so the
# image needs no package manager step and no network access at build time. If a
# future change to this file needs the network, that is a signal the change is
# wrong.
#
# This image must never contain the tests or the reference solution. The
# verifier runs in a separate image and mounts what it needs. Copying either one
# into the agent's environment would hand over the answer.

FROM rust@sha256:540c902e99c384163b688bbd8b5b8520e94e7731b27f7bd0eaa56ae1960627ab

# Offline for every cargo invocation in the image and in the agent's session.
# The crate vendors nothing because it depends on nothing; this makes an
# accidental network fetch fail loudly rather than succeed quietly.
ENV CARGO_NET_OFFLINE=true

WORKDIR /app

# Copied explicitly, path by path. A blanket `COPY . /app` would pick up
# whatever happens to sit beside this file, which is how a solution or a test
# ends up shipped by accident.
COPY Cargo.toml /app/Cargo.toml
COPY Cargo.lock /app/Cargo.lock
COPY src /app/src
COPY spec /app/spec
COPY examples /app/examples
COPY cross.json /app/cross.json
COPY README.md /app/README.md

# Warms the build so the agent's first `cargo build` does not pay for a cold
# compile of the frozen modules. The placeholder solver compiles, so this
# succeeds; it proves the environment is usable before the agent touches it.
RUN cargo build --offline --release && cargo build --offline

CMD ["/bin/bash"]
