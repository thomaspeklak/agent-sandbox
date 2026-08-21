# Walkthrough

## Image Baseline

- Split 26 fixed runtime, standard utility, and header packages from optional tools.
- Kept `extra_dnf_packages` and `EXTRA_DNF_PACKAGES` limited to purposeful,
  user-selectable tools.
- Preserved explicit empty optional lists as a fixed-baseline-only image.
- Added synchronization tests for the fixed baseline, optional defaults, generated
  config, example config, config editor, and Containerfile.

## Tool Catalog

- Replaced package-shaped groups with 22 canonical tool definitions.
- Added required stable IDs, purpose-focused descriptions, `default` flags, and
  internal DNF ownership.
- Added exactly three ordered profession views: General, Software Development,
  and Operations and DevOps.
- Added ordered subcategories that reference canonical tool IDs, allowing one tool
  to appear in several profession areas without duplicated state or package output.
- Kept all selectable languages under Languages in development and operations.
- Kept npm and Python pip under Package managers in both professional views.
- Added Wayland clipboard to General, Software Development, and Operations and DevOps.
- Made Kitty terminfo an internal dependency of the tmux tool.

## Picker Experience

- Rebuilt the horizontal selector around the three profession tabs.
- Added visible, non-selectable subcategory divider rows.
- Added stateful scrolling that keeps the selected tool visible.
- Removed DNF package names from the primary UI.
- Added recommended-default markers and `d` to restore catalog defaults.
- Removed profession-wide bulk selection so tabs remain views rather than presets.
- Added details showing every profession and area in which a shared tool appears.

## Configuration Compatibility

- Existing explicit package selections remain authoritative.
- Omitted package lists use the supplied catalog's defaults in the picker.
- Explicit empty lists remain distinct from omitted lists.
- Picker saves remove fixed baseline names from legacy extra-package lists.
- Unknown non-baseline packages, untouched partial bundles, overlay ownership, file
  backups, and legacy managed-tool cleanup remain supported.

## Documentation And Tests

- Updated README, quick setup, command documentation, config documentation, CLI
  help, errors, and shell completions to describe profession-guided tool selection.
- Added model, migration, schema, placement, navigation, scrolling, and render-level
  regression coverage.
- Verified rendered tabs, divider rows, default markers, and absence of internal RPM
  names in the primary UI.

## Verification

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all`
- `git diff --check`
- Independent final correctness and UX reviews reported no remaining findings.
- Container smoke testing remains unavailable because no supported container/image
  runtime is installed on this host.
