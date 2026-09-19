# pnpm artifact, installed from the exact registry tarball whose version and
# integrity AGS resolved. Rust lives in a separate artifact, so a pnpm update
# never reinstalls Rust or rebuilds Glimpse.
ARG FOUNDATION_IMAGE
FROM ${FOUNDATION_IMAGE} AS work

ARG PNPM_VERSION
ARG PNPM_SHA512
RUN set -eu; \
    case "$PNPM_VERSION" in \
      *[!0-9.]*|'') echo "invalid pnpm version: $PNPM_VERSION" >&2; exit 1 ;; \
    esac; \
    work="$(mktemp -d)"; \
    curl --proto '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 300 \
      --retry 2 --retry-delay 1 \
      "https://registry.npmjs.org/pnpm/-/pnpm-${PNPM_VERSION}.tgz" -o "$work/pnpm.tgz"; \
    printf '%s  %s\n' "$PNPM_SHA512" "$work/pnpm.tgz" | sha512sum -c -; \
    npm install --global --prefix /usr/local --ignore-scripts --no-audit --no-fund "$work/pnpm.tgz"; \
    installed="$(node -p "require('/usr/local/lib/node_modules/pnpm/package.json').version")"; \
    test "$installed" = "$PNPM_VERSION"; \
    rm -rf "$work"

FROM scratch
COPY --from=work /usr/local/lib/node_modules/pnpm/ /usr/local/lib/node_modules/pnpm/
