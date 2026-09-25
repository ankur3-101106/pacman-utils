# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.2.1] - 2026-09-25

### Added
- **Streamlined Repository Submenus (`Add Repositories` & `Remove Repositories`)**:
  - Replaced repetitive top-level repository actions in Tab 4 (`🌍 Mirrors & Repos`) and `MirrorsScreen` with structured submenus.
  - Interactive status badges displaying real-time configuration status (`enabled` / `not enabled`) for each repository.
- **Repository Removal Actions**:
  - **Chaotic-AUR Removal**: strips `[chaotic-aur]` pacman configuration, purges `chaotic-keyring` & `chaotic-mirrorlist`, deletes GPG keys, and syncs databases.
  - **CachyOS Removal**: executes official uninstaller (`cachyos-repo.sh --remove`), cleans pacman configuration and mirrorlists, removes keyrings, and refreshes package databases.
  - **BlackArch Removal**: removes `[blackarch]` pacman directives, purges `blackarch-keyring` & `blackarch-mirrorlist`, and resyncs databases.
- **Homebrew Uninstallation**:
  - Added **Uninstall Homebrew (brew)** under `⭐ Extras`.
  - Executes official uninstaller script non-interactively, strips shellenv integration from `~/.bashrc` and `~/.zshrc`, removes `/usr/local/bin/brew` symlinks, and cleans Linuxbrew directories.

## [2.2.0] - 2026-09-25

### Added
- **Chaotic-AUR Repository Enable Option**:
  - Keyring import/local signing for Chaotic-AUR GPG keys (`FBA220DFC880C036` and `3056513887B78AEB`).
  - Automated installation of `chaotic-keyring` and `chaotic-mirrorlist`.
  - Appends `[chaotic-aur]` configuration to `/etc/pacman.conf` with automatic backup (`/etc/pacman.conf.bak`) and database sync (`pacman -Sy`).
- **CachyOS Optimized Repositories Enable Option**:
  - Official CachyOS repository setup integrating automated hardware instruction set detection (`x86-64-v3`, `x86-64-v4`, `znver4`).
  - Installs GPG keyring and mirrorlists, configures `/etc/pacman.conf`, and synchronizes package databases.
- **Homebrew (brew) Installation Option**:
  - One-click installer for Homebrew on Linux under `⭐ Extras` and reported in `Dependency Check`.
  - Automates installation of base build dependencies (`base-devel`, `procps-ng`, `curl`, `git`, `file`).
  - Sets up shell environment integration (`eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"`) in `~/.bashrc` and `~/.zshrc`.
- **BlackArch Pentesting Repository & Tool Options**:
  - One-click enablement for the BlackArch penetration testing repository using official `strap.sh`.
  - Added new "BlackArch Security & Pentesting Essentials" curated group to Package Groups (`⭐ Extras ▸ Package Groups`).

## [2.1.0] - 2026-09-09

### Added
- **Centralized Command & Transaction Engine (`src/cmd.rs`)**:
  - Introduced `CommandSpec` to decouple command parameters, execution mode, and logging concerns from UI screens.
  - Implemented `CommandEngine` with explicit `CommandMode::Capture` and `CommandMode::Interactive`.
  - Structured execution status via `CommandStatus` (`Success`, `Exited`, `Signaled`).
- **Independent Confirmation Controls**:
  - Independent `CONFIRM_ACTIONS` (Archman UI confirmation prompt) and `NATIVE_CONFIRM` (underlying package manager `--noconfirm` injection) settings.
  - Backward-compatible default `NATIVE_CONFIRM=true`.
  - Four fully tested confirmation combinations covering interactive prompts and non-interactive workflows.
- **PTY Hardening (`src/pty.rs`)**:
  - Controlling terminal (`TIOCSCTTY`) and session setup (`setsid`).
  - Single-owner process lifecycle with dedicated reaper worker thread, preventing double-waitpid races.
  - Signal-safe termination (`signal`, `kill`) preventing PID recycling races.
  - Non-blocking dynamic terminal resizing with dimension clamping.
  - Mutex poisoning protection via `.unwrap_or_else(|p| p.into_inner())`.
- **Preflight Checks & Transaction Preview (`src/tx.rs`, `src/screens/preview.rs`)**:
  - Automated preflight inspection: database lock check (live PID vs stale lock), binary resolution in `PATH`, non-interactive privilege verification, network route inspection, root disk space thresholds (`100 MB` block, `1 GB` warning), and target name validation.
  - Dedicated `PreviewScreen` presenting preflight status (`PASS`, `WARN`, `BLOCK`) and non-destructive `pacman --print` output prior to execution.
  - Honest preview limitations noted for AUR helpers and pacman `-Rns` `--nosave`.
- **Reliability & Release Infrastructure**:
  - GitHub Actions CI pipeline (`.github/workflows/ci.yml`) testing formatting, strict clippy lints, unit/integration test suites, and release builds.
  - Single version source in `Cargo.toml` derived via `env!("CARGO_PKG_VERSION")`.
  - Truthful `PreflightEnv::system()` host representation paired with deterministic `PreflightEnv::mock()` injection for hermetic test execution.
  - Comprehensive edge-case regression test suite with 101 tests passing.

### Changed
- Refactored all screens (`install`, `remove`, `update`, `packages`, `lockfile`, `mirrors`, `settingsscr`) to route external commands through the centralized engine.
- Replaced panic paths in libc calls and configuration parsing with resilient, zero-panic fallbacks.

### Known Limitations
- **Reflector Interactive Cancellation (Ctrl-C)**: While pacman transactions, AUR helpers, and shell commands respond immediately to `Ctrl-C` (terminal interrupt), `reflector` currently does not cleanly terminate upon `Ctrl-C` in the embedded runner and should be allowed to run to completion or interrupted externally. This is scheduled for refinement in a future patch.

---

## [2.0.0] - 2026-08-26

### Added
- Complete rewrite in Rust using `ratatui` and `crossterm`.
- Embedded pseudo-terminal command runner.
- Fuzzy search across package actions and repositories.
- Live system dashboard with dependency checking and mirror management.
