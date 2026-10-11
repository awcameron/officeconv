# A Linux image for building and testing officeconv: the release toolchain, nightly for
# fuzzing, cargo-fuzz, cargo-deny, and Python 3 and git for tools/compare. It's for
# contributors only, not for running officeconv. CONTRIBUTING.md says how to use it.

# Exactly the rust-version in Cargo.toml, which releases build with. CI fails if the two differ.
# Debian rather than Alpine: CI and the main Linux releases use glibc, so tests here build what
# CI builds, and the compiler doesn't run with musl's slower allocator. Nearly all of the
# image's size is the toolchains, so Alpine's smaller base would save little.
FROM rust:1.92.0-slim-bookworm@sha256:f1f73538ebe623fd3673a35aff3df358ae1084c64c55646516e5b17b321b6c9b

ARG CARGO_FUZZ_VERSION=0.13.2
ARG CARGO_DENY_VERSION=0.20.2
# SHA-256 of cargo-deny's prebuilt archives, as its release publishes them. Change them with the
# version above. CI's deny job installs the same version, reading it and the amd64 hash from here.
ARG CARGO_DENY_SHA256_AMD64=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
ARG CARGO_DENY_SHA256_ARM64=995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d

# g++ builds libFuzzer for cargo-fuzz; curl downloads cargo-deny.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl g++ git python3 \
    && rm -rf /var/lib/apt/lists/*

# rust-src is for rust-analyzer in an editor attached to the container. Nightly is only for
# fuzzing, so it's whichever one is current when the image is built.
RUN rustup component add rustfmt clippy rust-src \
    && rustup toolchain install nightly --profile minimal

# cargo-deny's own prebuilt binary, checked against the hashes above: compiling it takes minutes.
# cargo-fuzz is compiled, which takes under a minute: its prebuilt binary is a musl build, and
# cargo-fuzz builds fuzz targets for the target it was itself built for, which nightly here
# doesn't have. Change CARGO_FUZZ_VERSION with the one in .github/workflows/fuzz.yml.
RUN set -eu; \
    arch=$(dpkg --print-architecture); \
    case $arch in \
        amd64) target=x86_64-unknown-linux-musl; deny_sha=$CARGO_DENY_SHA256_AMD64 ;; \
        arm64) target=aarch64-unknown-linux-musl; deny_sha=$CARGO_DENY_SHA256_ARM64 ;; \
        *) echo "no prebuilt cargo-deny for $arch" >&2; exit 1 ;; \
    esac; \
    cd /tmp; \
    deny=cargo-deny-$CARGO_DENY_VERSION-$target; \
    curl -fsSLO "https://github.com/EmbarkStudios/cargo-deny/releases/download/$CARGO_DENY_VERSION/$deny.tar.gz"; \
    echo "$deny_sha  $deny.tar.gz" | sha256sum -c -; \
    tar xzf "$deny.tar.gz" -C "$CARGO_HOME/bin" --strip-components 1 "$deny/cargo-deny"; \
    rm "$deny.tar.gz"; \
    cargo install --locked "cargo-fuzz@$CARGO_FUZZ_VERSION"; \
    rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git"

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
