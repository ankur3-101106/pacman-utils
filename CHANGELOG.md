# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.2.0] - 2026-09-25

### Added
- **Repository Addition & Removal Management**:
  - Streamlined `🌍 Mirrors & Repos` dashboard action list and `MirrorsScreen` menu with clean **Add Repositories** and **Remove Repositories** submenus, avoiding redundant repetitive entries.
  - One-click enablement and clean removal for third-party repositories:
    - **Chaotic-AUR**: automated key import/deletion, `chaotic-keyring` & `chaotic-mirrorlist` install/purge, `/etc/pacman.conf` section configuration and cleanup, and database refresh.
    - **CachyOS**: official bootstrap installer/uninstaller (`cachyos-repo.sh --remove`), automated CPU instruction set detection (`x86-64-v3/v4/znver4`), keyring and mirrorlist cleanup, and database sync.
    - **BlackArch**: official `strap.sh` bootstrap, keyring/mirrorlist removal, `/etc/pacman.conf` cleanup, and sync.
  - Added new "BlackArch Security & Pentesting Essentials" curated group to Package Groups (`⭐ Extras ▸ Package Groups`).
- **Homebrew (brew) Installer & Uninstaller**:
  - One-click installer and uninstaller for Homebrew on Linux under `⭐ Extras`.
  - Installer automates prerequisite build dependencies (`base-devel`, `procps-ng`, `curl`, `git`, `file`), runs official install script, and sets up shell integration in `~/.bashrc` and `~/.zshrc`.
  - Uninstaller runs official Homebrew uninstall script with noninteractive bypass, cleans up shell environment integration lines, removes `/usr/local/bin/brew` symlinks, and deletes Linuxbrew directories.
  - Reported dynamically in `Dependency Check` and `Settings`.

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
