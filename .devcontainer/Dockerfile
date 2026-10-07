# A Linux image for building and testing officeconv: the release toolchain, nightly for
# fuzzing, cargo-fuzz, cargo-deny, and Python 3 and git for tools/compare. It's for
# contributors only, not for running officeconv. CONTRIBUTING.md says how to use it.

# Exactly the rust-version in Cargo.toml, which releases build with. CI fails if the two differ.
FROM rust:1.92.0-slim-bookworm@sha256:f1f73538ebe623fd3673a35aff3df358ae1084c64c55646516e5b17b321b6c9b

ARG CARGO_FUZZ_VERSION=0.13.2
ARG CARGO_DENY_VERSION=0.20.2

# g++ builds libFuzzer for cargo-fuzz.
RUN apt-get update \
    && apt-get install -y --no-install-recommends g++ git python3 \
    && rm -rf /var/lib/apt/lists/*

# rust-src is for rust-analyzer in an editor attached to the container. Nightly is only for
# fuzzing, so it's whichever one is current when the image is built.
RUN rustup component add rustfmt clippy rust-src \
    && rustup toolchain install nightly --profile minimal \
    && cargo install --locked "cargo-fuzz@$CARGO_FUZZ_VERSION" "cargo-deny@$CARGO_DENY_VERSION" \
    && rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git"

# The repository is mounted from the host, and its owner rarely matches the container's user.
RUN git config --system --add safe.directory '*'

# Build output and Cargo's download cache live outside the mounted repository, where named
# volumes keep them between runs; builds on a bind mount are slow under Docker Desktop. Any
# user can write to them, so `docker run --user` works on Linux.
ENV CARGO_TARGET_DIR=/cache/target
RUN mkdir -p /cache/target "$CARGO_HOME/registry" \
    && chmod 1777 /cache/target "$CARGO_HOME/registry"

# A non-root default user. On Linux, pass --user "$(id -u):$(id -g)" so files written into the
# repository belong to you.
RUN useradd --create-home --uid 1000 dev
USER dev
WORKDIR /work
