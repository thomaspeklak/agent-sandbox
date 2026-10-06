# Applies newly available RPM updates on top of the previous OS checkpoint.
# AGS runs this recipe with --no-cache only after `dnf check-upgrade` reports
# updates, so each checkpoint continues from the last one instead of replaying
# every upgrade since the baseline.
ARG CHECKPOINT_IMAGE
FROM ${CHECKPOINT_IMAGE}

RUN dnf -y upgrade --refresh --setopt=skip_if_unavailable=False && \
    dnf clean all
