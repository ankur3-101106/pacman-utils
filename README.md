# archman — Interactive Arch Linux System Manager

<p align="center">
  <b>A fast, keyboard-driven terminal dashboard for managing your Arch Linux system.</b>
</p>

<p align="center">
  <a href="https://www.archlinux.org/"><img src="https://img.shields.io/badge/Arch_Linux-1793D1?style=for-the-badge&logo=arch-linux&logoColor=white" alt="Arch Linux" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" /></a>
  <img src="https://img.shields.io/badge/Version-2.0.0-green?style=for-the-badge" alt="version" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL_2.0-blue?style=for-the-badge" alt="GPL-2.0" /></a>
</p>

---

**archman** is a native **Rust** TUI (built on [ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm)) inspired by [LinUtil](https://github.com/ChrisTitusTech/linutil). Everything lives in one dashboard: a category sidebar with live system info, a flat action list with descriptions, a global search, and an embedded command runner — pacman, AUR helpers and reflector execute *inside the pane*, with your keystrokes (sudo passwords, `[Y/n]` prompts) forwarded straight to them.

```
 ╔════════════════╦ SEARCH ══════════════════════╗
 ║  ▄▀█ █▀█ █▀▀   ║ Type to search (/)           ║
 ║  █▀█ █▀▄ █░░   ╠═ ARCHMAN ─────────────────────╣
 ║  archman v2.0.0║ ▸ Install Package             ║
 ╠════════════════╡   Search Packages             ║
 ║ ▸ 1 📦 Packages │   Remove Package …           ║
 ║   2 ⬆ System    │                              ║
 ╠════════════════╡                              ║
 ║ SYSTEM         │                              ║
 ║  CPU: … RAM: … ║                              ║
 ╠════════════════╧══════════════════════════════╣
 ║ Command list                                  ║
 ║ [q] Exit   [tab] Category   [/] Search  …     ║
 ╚═══════════════════════════════════════════════╝
```

## ✨ Features

| | Feature | Description |
|--|---------|-------------|
| 📦 | **Packages** | Install (official + AUR, dry-run mode), search, remove (3 strategies), browse, file ownership, package info, history, transaction log, reinstall, orphans, export/import lists |
| ⬆ | **System** | Update check (`checkupdates`), database refresh, full upgrades via pacman or your AUR helper |
| 🧹 | **Maintenance** | Cache cleaning (`paccache -rkN`, `-Sc`, `-Scc`), safe `db.lck` removal |
| 🌍 | **Mirrors** | reflector auto-update & ranking, mirrorlist backup/restore |
| 📊 | **Information** | System dashboard, dependency check with one-key installs |
| ⭐ | **Extras** | Favorite packages, 12 curated package groups, list import |
| ⚙ | **Settings** | AUR helper (incl. building yay/paru from AUR), dry-run, confirmations, logging, cache retention |

## ⌨ Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `↑` / `↓` (`k` / `j`) | Move selection |
| `/` | Search all actions |
| `tab` / `shift+tab` | Next / previous category |
| `1` … `7` | Jump straight to a category |
| `g` / `G` | First / last action |
| `enter` | Run or open the selected action |
| `esc` | Go back · clear search filter |
| `q` | Quit |
| `?` | Shortcut cheatsheet overlay |
| `ctrl+c` | Quit / interrupt a running command |

> ♿ **Accessibility** — every category has a color *and* a number; selection uses a cursor glyph plus bold-italic text (never color alone); statuses pair icons (`✔ ⚠ ✘`) with color; secondary text uses high-contrast grays; and `NO_COLOR` strips all styling while every cue stays legible.

## 🖥 Embedded command runner

Commands never take over your terminal. They run on a pseudo-terminal **inside the action pane**:

- output streams live, with progress bars rendered sanely
- sudo password prompts and pacman `[Y/n]` work right in the pane
- the border is **cyan** while running, **green** on success, **red** on failure
- `ctrl+c` interrupts · `pgup` / `pgdn` browse scrollback
- launching from any sub-screen returns you to the dashboard while it runs

## 📦 Installation

```bash
git clone https://github.com/ankur3-101106/pacman-utils.git
cd pacman-utils
./install.sh          # builds with cargo, installs to /usr/local/bin
```

Manual build:

```bash
cargo build --release
sudo cp target/release/archman /usr/local/bin/
```

## 🔧 Dependencies

| Dependency | Required | Purpose |
|------------|----------|---------|
| `pacman` | ✅ | Core package manager |
| Rust toolchain | ✅ to build | [rustup](https://rustup.rs) or `sudo pacman -S rust` |
| `yay` / `paru` | optional | AUR helper |
| `reflector` | optional | Mirror management |
| `pacman-contrib` | optional | `paccache` + `checkupdates` |

## ⚙ Configuration

Everything lives in `~/.config/archman/` (created on first launch, same format since v1):

| File | Purpose |
|------|---------|
| `settings.conf` | AUR helper, dry-run, confirmations, logging, cache retention |
| `favorites.txt` | Your favorite packages |
| `archman.log` | Activity log |

## 📖 Usage

```bash
archman              # launch the dashboard
archman --version
archman --help
```

## 🏗 Project Structure

```
src/
├── main.rs          # entry point: CLI flags, terminal lifecycle
├── app.rs           # screen stack, modals, event loop, embedded runner
├── widgets.rs       # logo, menus, fuzzy lists, pagers, theme, cheatsheet
├── settings.rs      # ~/.config/archman (settings, favorites, log)
├── sys.rs           # pacman / AUR / reflector wrappers, background jobs
├── pty.rs           # pseudo-terminal execution for embedded commands
├── fuzzy.rs         # subsequence fuzzy matcher
└── screens/
    ├── registry.rs  # declarative category → action table (the whole UI)
    ├── home.rs      # two-pane dashboard (sidebar + action pane)
    ├── runpane.rs   # embedded command runner
    └── …            # one module per feature (install, mirrors, …)
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

Run `cargo test` before submitting — the suite covers settings, fuzzy matching, the PTY layer and the runner pane.

## 📄 License

Licensed under the **GNU General Public License v2.0** — see [LICENSE](LICENSE).
