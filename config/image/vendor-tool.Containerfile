# One pinned vendor executable. AGS downloads and verifies the archive on the
# host; this recipe re-verifies it and extracts only the declared member, so an
# unchanged tool is never downloaded or extracted again.
ARG FOUNDATION_IMAGE
FROM ${FOUNDATION_IMAGE} AS work

COPY archive /tmp/ags-vendor/archive
ARG TOOL_ID
ARG TOOL_ARCHIVE
ARG TOOL_MEMBER
ARG TOOL_MEMBER_MATCH=exact
ARG TOOL_INSTALL_AS
ARG TOOL_SHA256
RUN set -eu; \
    matches() { printf '%s' "$1" | grep -Eq "$2"; }; \
    matches "$TOOL_ID" '^[a-z][a-z0-9]*(-[a-z0-9]+)*$'; \
    case "$TOOL_ARCHIVE" in zip|tar.gz|tar.xz) ;; *) echo "unsupported archive: $TOOL_ARCHIVE" >&2; exit 1 ;; esac; \
    case "$TOOL_MEMBER_MATCH" in exact|unique_basename) ;; *) echo "unsupported member match: $TOOL_MEMBER_MATCH" >&2; exit 1 ;; esac; \
    test -n "$TOOL_MEMBER"; \
    case "$TOOL_MEMBER" in /*|-*) echo "unsafe archive member: $TOOL_MEMBER" >&2; exit 1 ;; esac; \
    if matches "$TOOL_MEMBER" '(^|/)\.\.?(/|$)|[][*?]'; then echo "unsafe archive member: $TOOL_MEMBER" >&2; exit 1; fi; \
    matches "$TOOL_INSTALL_AS" '^[a-z][a-z0-9._-]*$'; \
    matches "$TOOL_SHA256" '^[A-Fa-f0-9]{64}$'; \
    work=/tmp/ags-vendor; archive="$work/archive"; binary="$work/binary"; \
    echo "Extracting $TOOL_ID"; \
    printf '%s  %s\n' "$TOOL_SHA256" "$archive" | sha256sum -c -; \
    archive_member="$TOOL_MEMBER"; \
    if [ "$TOOL_MEMBER_MATCH" = "unique_basename" ]; then \
      members="$work/members"; found="$work/matches"; \
      case "$TOOL_ARCHIVE" in \
        zip) unzip -Z1 "$archive" > "$members" ;; \
        tar.gz) tar -tzf "$archive" > "$members" ;; \
        tar.xz) tar -tJf "$archive" > "$members" ;; \
      esac; \
      : > "$found"; \
      while IFS= read -r candidate; do \
        if [ "${candidate##*/}" = "$TOOL_MEMBER" ]; then printf '%s\n' "$candidate" >> "$found"; fi; \
      done < "$members"; \
      test "$(wc -l < "$found")" -eq 1; \
      archive_member="$(IFS= read -r candidate < "$found"; printf '%s' "$candidate")"; \
    fi; \
    case "$TOOL_ARCHIVE" in \
      zip) unzip -p "$archive" "$archive_member" > "$binary" ;; \
      tar.gz) tar -xOzf "$archive" -- "$archive_member" > "$binary" ;; \
      tar.xz) tar -xOJf "$archive" -- "$archive_member" > "$binary" ;; \
    esac; \
    test -s "$binary"; \
    install -D -m 0755 "$binary" "/out/$TOOL_INSTALL_AS"; \
    rm -rf "$work"

FROM scratch
COPY --from=work /out/ /out/
