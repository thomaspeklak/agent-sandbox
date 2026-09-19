# Image-owned Rust toolchain artifact shared by the Glimpse build and the
# sandbox image. The work stage starts from the previous artifact when one
# exists, so a rustup-only update keeps the installed compiler. The exported
# stage contains only the current /usr/local/rustup and /usr/local/cargo trees.
ARG FOUNDATION_IMAGE
ARG RUST_SEED_IMAGE
FROM ${RUST_SEED_IMAGE} AS seed

FROM ${FOUNDATION_IMAGE} AS work
COPY --from=seed /usr/local/rustup/ /usr/local/rustup/
COPY --from=seed /usr/local/cargo/ /usr/local/cargo/
COPY rust-install.sh /tmp/ags-rust-install.sh
ARG RUST_TRIPLE
ARG RUSTUP_VERSION
ARG RUSTC_VERSION
RUN sh /tmp/ags-rust-install.sh && rm -f /tmp/ags-rust-install.sh

FROM scratch
COPY --from=work /usr/local/rustup/ /usr/local/rustup/
COPY --from=work /usr/local/cargo/ /usr/local/cargo/
