# Bugs

- [x] Propagate failures from every verified-download installer loop iteration.
- [x] Reject archive members that can be interpreted as command options or glob patterns.
- [x] Persist download locks with portable, content-addressed paths without mutating the active lock before config commit.
- [x] Prevent preserved unknown downloads from colliding with commands owned by selected catalog downloads.
- [x] Preserve effective base packages when an overlay owns only the download lock.
