# Glimpse sandbox shim, compiled with the shared Rust artifact against the
# checked-in standalone lockfile. Only the executable leaves this recipe.
ARG FOUNDATION_IMAGE
ARG RUST_IMAGE
FROM ${RUST_IMAGE} AS rust

FROM ${FOUNDATION_IMAGE} AS build
COPY --from=rust /usr/local/rustup/ /usr/local/rustup/
COPY --from=rust /usr/local/cargo/ /usr/local/cargo/
COPY glimpse-shim/ /src/glimpse-shim/
RUN cd /src/glimpse-shim && \
    RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/tmp/cargo-home \
      /usr/local/cargo/bin/cargo +stable build --release --locked && \
    install -D -m 0755 target/release/glimpse-shim /out/glimpse-shim

FROM scratch
COPY --from=build /out/glimpse-shim /out/glimpse-shim
