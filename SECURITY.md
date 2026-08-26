# Security Policy

## Supported versions

archman is a system tool that runs privileged commands on your machine — please keep it up to date.

| Version | Supported |
|---------|-----------|
| 2.0.x   | ✅ |
| 1.x (bash edition) | ❌ removed in v2 — please upgrade |

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately using GitHub's [private vulnerability reporting](https://github.com/ankur3-101106/pacman-utils/security/advisories/new) on this repository (Security tab → Report a vulnerability). If private reporting is unavailable, contact the maintainer directly: [@ankur3-101106](https://github.com/ankur3-101106).

When reporting, please include:

- your `archman --version` output
- the terminal environment (emulator, `TERM`, window size if relevant)
- steps or a proof of concept to reproduce the issue
- your assessment of the impact

You will get an initial response as soon as the report is triaged, and credit in the release notes if you wish.

## Scope

archman is a TUI that constructs and executes privileged commands (`sudo pacman`, `paccache`, `reflector`, AUR helpers) on a pseudo-terminal embedded in its own interface. Areas of particular interest:

- **Command / argument construction** — package names, settings values and imported package lists flow into shell command arguments (`src/sys.rs`, `src/screens/registry.rs`). Injection through crafted package names or a malicious `~/.config/archman/settings.conf` is in scope.
- **PTY handling and child process lifecycle** — spawning, keystroke forwarding, and cleanup of child processes (`src/pty.rs`, `src/screens/runpane.rs`).
- **Config and list parsing** — `src/settings.rs`, package-list import (`src/screens/exportimport.rs`).
- **Terminal handling** — raw mode, alternate screen, and panic-hook restoration (`src/main.rs`).

### Not a vulnerability

archman intentionally mutates your system — removing packages, wiping caches, restarting mirrors — behind explicit confirmations. "It removed packages I told it to remove" is expected behavior, not a security issue. Reports that a destructive action ran *without* its configured confirmation, however, very much are.
