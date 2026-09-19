# Build-only foundation shared by the artifact recipes. It is never a parent of
# the sandbox image, so ordinary OS package updates do not invalidate the Rust,
# pnpm, vendor-tool, or Glimpse artifacts built from it. `ags update-image
# --rebase` refreshes it.
ARG BASE_IMAGE
FROM ${BASE_IMAGE}

RUN dnf -y install \
      ca-certificates \
      curl \
      gcc \
      nodejs24-bin \
      nodejs24-npm-bin \
      pkgconf-pkg-config \
      tar \
      unzip \
      xz && \
    dnf clean all && \
    printf 'ignore-scripts=true\n' > /etc/npmrc && \
    mkdir -p /usr/local/rustup /usr/local/cargo
