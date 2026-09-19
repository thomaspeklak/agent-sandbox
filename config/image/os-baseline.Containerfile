# Stable OS baseline of the sandbox image. AGS builds it once per recorded
# Fedora base and package selection. Later RPM updates are applied as small
# checkpoint layers by os-refresh.Containerfile instead of rebuilding this.
ARG BASE_IMAGE
FROM ${BASE_IMAGE}

# Mise is published in upstream's COPR, not Fedora's default repositories.
# Minimal Fedora images need the DNF5 plugins for the `copr` subcommand.
# Update checks must fail when a repository is unavailable instead of silently
# skipping it and reporting the image as current, so no repository may opt out.
RUN dnf -y install dnf5-plugins && \
    dnf -y copr enable jdxcode/mise && \
    sed -i 's/^skip_if_unavailable=.*/skip_if_unavailable=False/' /etc/yum.repos.d/*.repo && \
    dnf clean all

# Stable AGS runtime and standard utility baseline. These packages are not user
# choices: the image and its documented workflows may rely on them being present.
RUN BASE_DNF_PACKAGES="bash ca-certificates coreutils curl dbus-devel diffutils file findutils gcc grep jq less mise nodejs24-bin patch procps-ng python3 sed sqlite-devel tar tree unzip util-linux wget which xz zip" && \
    dnf -y install $BASE_DNF_PACKAGES && \
    dnf clean all

# Purposeful user-selectable tools. AGS always passes its sorted, de-duplicated
# selection, so equivalent configurations share one cached baseline.
ARG EXTRA_DNF_PACKAGES="git gh openssh-clients fd-find ripgrep rsync tmux kitty-terminfo socat make pkgconf-pkg-config sccache"
RUN if [ -n "$EXTRA_DNF_PACKAGES" ]; then \
      dnf -y install $EXTRA_DNF_PACKAGES; \
    fi && \
    dnf clean all

RUN useradd -m -u 1000 -s /bin/bash dev && \
    mkdir -p /workspace /home/dev/.local/bin /home/dev/.local/share /home/dev/.cache /home/dev/.config/pnpm /usr/local/pnpm /opt/claude-home /opt/opencode-home /opt/ags && \
    printf 'ignore-scripts=true\n' > /etc/npmrc && \
    printf 'ignore-scripts=true\n' > /home/dev/.npmrc && \
    printf 'ignoreScripts: true\nstoreDir: /usr/local/pnpm/.store\nglobalBinDir: /usr/local/pnpm/bin\n' > /home/dev/.config/pnpm/config.yaml && \
    printf '#!/bin/sh\nif command -v sccache >/dev/null 2>&1; then exec sccache "$@"; fi\nexec "$@"\n' > /opt/ags/rustc-wrapper && \
    chmod 0755 /opt/ags/rustc-wrapper && \
    chown -R dev:dev /workspace /home/dev /usr/local/pnpm /opt/claude-home /opt/opencode-home
