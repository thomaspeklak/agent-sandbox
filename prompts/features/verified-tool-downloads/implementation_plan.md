# Implementation Plan

1. Extend the canonical tool catalog with mutually exclusive DNF and verified
   archive providers, strict URL/checksum/path validation, and provider-aware
   selection state.
2. Add validated sandbox configuration for selected external-tool lock records,
   preserving omitted/default and explicit-empty selection behavior alongside
   `extra_dnf_packages`.
3. Pass selected lock records through launch plans and both image-build paths as
   encoded structured build input.
4. Add a generic Containerfile installer that selects the build architecture,
   downloads over HTTPS, verifies SHA-256, safely extracts the declared archive
   member, and installs only the requested executable.
5. Add the requested tools, Fedora package mappings, optional defaults, and
   approved profession/subcategory placements to the catalog.
6. Update configuration examples, CLI documentation, and architecture guidance
   for verified downloaded tools and reproducible lock behavior.
7. Add regression coverage for catalog validation, mixed-provider selection and
   persistence, build arguments, Containerfile synchronization, and requested
   placements, then run formatting, tests, and Clippy.
