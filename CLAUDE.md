# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

**archman** — an interactive, menu-driven Bash TUI for managing an Arch Linux system (pacman/AUR installs, updates, cache, mirrors, etc.). It only runs meaningfully on an Arch-based system with `pacman`.

## Running & Checking

There is no build system, test suite, or linter config.

```bash
./archman            # Run from source (interactive menu)
./archman --version  # --help also available
bash -n archman lib/*.sh        # Syntax check
shellcheck archman lib/*.sh     # Static analysis, if installed
```

Most behavior is interactive (gum/fzf prompts, `sudo pacman`), so verify changes by walking through the affected menus in a real terminal. Note `install.sh` copies the script to `/usr/local/bin/archman` and libs to `/usr/local/lib/archman/` — an *installed* copy does not reflect repo edits until reinstalled; prefer testing from source.

## Architecture

Everything is plain Bash sourced into a single shell process — there is exactly **one global namespace** shared by all modules.

### Entry point (`archman`)

1. Resolves `LIB_DIR`: prefers `./lib` next to the script, falls back to `/usr/local/lib/archman`, then `/usr/lib/archman`. This is what lets the same file run from source or installed.
2. Sources all 14 `lib/*.sh` modules in fixed order (`ui.sh` first — others depend on its functions).
3. `init()`: runs `detect_capabilities` then `settings_init`, logs session start.
4. `main_menu()`: infinite loop rendering the main menu and dispatching.

### Menu dispatch convention

Menu selection uses **substring matching** against the displayed label:

```bash
case "$choice" in
    *"Install Package"*) install_menu ;;
```

Adding a menu item requires updating both the `ui_choose` label list and adding a `case` arm whose pattern matches the label. Patterns match substrings, so labels must stay unambiguous relative to each other (order matters — earlier arms win).

### Module convention (`lib/`)

Each module owns one feature and exposes a `<name>_menu()` entry point called from menus. Internal helpers are prefixed `_name` (e.g. `_install_search_similar`). Modules never call each other's menus except via well-known entry points like `install_specific_package`.

### Shared globals (the module API)

- `ui.sh`: `ui_*` wrapper functions, color constants (`CLR_*`), capability flags `HAS_GUM`/`HAS_FZF`/`HAS_REFLECTOR`/`HAS_PACCACHE` (set once by `detect_capabilities`), and `get_aur_helper()`.
- `settings.sh`: config paths (`CONFIG_DIR`, `CONFIG_FILE`, `LOG_FILE`, `FAVORITES_FILE` — all under `~/.config/archman/`), the `SETTINGS` associative array, and `log_action()`.
- `groups.sh`: `PKG_GROUPS` / `GROUP_DESC` associative arrays defining the curated package groups.

Rules the codebase follows:

- **Never call `gum`, `fzf`, or raw `echo` styling directly in feature code** — use the `ui_*` wrappers, which degrade gracefully when gum/fzf are absent (`$HAS_GUM` branching lives inside `ui.sh`).
- Persist user-visible actions via `log_action "CATEGORY: message"` (respects the `LOG_ENABLED` setting).
- Honor `SETTINGS[DRY_RUN]` before destructive/install operations.
- Use `get_aur_helper` (not bare `yay`) for AUR operations — it respects the configured helper and auto-detects.
- Only the main `archman` script sets `set -euo pipefail`; sourced modules inherit it. A failing command outside a conditional aborts the whole app, so exit-status checks must be written as `if cmd; then` or guarded immediately after.
