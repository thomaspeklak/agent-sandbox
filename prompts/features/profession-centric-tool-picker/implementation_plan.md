# Implementation Plan

1. Split the image package set into a fixed baseline and optional tool packages.
   Install the baseline unconditionally and keep `EXTRA_DNF_PACKAGES` for selected
   tools only.
2. Replace the current group-of-package-owners JSON with canonical tool definitions
   plus three profession groups containing ordered subcategories and tool ID
   references.
3. Reintroduce the per-tool `default` flag and synchronize default tool packages
   with generated config, the Containerfile build argument, and config-editor data.
4. Refactor selection state to store each tool once and derive profession rows from
   catalog references. Preserve unknown packages, partial bundles, overlays, and
   legacy cleanup while stripping fixed baseline packages on save.
5. Rebuild the TUI with three profession tabs, non-selectable subcategory dividers,
   shared selection state, scrolling, default indicators, and a restore-defaults
   action. Remove package names and package-manager-oriented bulk behavior.
6. Update CLI help, documentation, and quick setup wording without renaming the
   existing `--packages` option or example catalog file.
7. Add regression coverage for schema validation, shared references, defaults,
   baseline normalization, divider navigation, scrolling, save deduplication,
   and synchronization between catalog, config, and Containerfile.
8. Run formatting, workspace tests, Clippy, diff checks, and a final review. Record
   the container smoke-test gap if no local container runtime is available.
