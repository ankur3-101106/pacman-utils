# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

archman — an interactive TUI system manager for Arch Linux (pacman + AUR). A native **Rust** application built on **ratatui** + **crossterm**, styled after LinUtil: a two-pane browser (category sidebar → flat action list), description pane, ASCII logo header, per-category accent colors, and a `?` cheatsheet overlay. All styling flows through the helpers in `widgets.rs`, which also honor `NO_COLOR`. The original bash implementation was removed in v2; its sources remain reachable in git history.

The user-facing surface is declared entirely in `screens/registry.rs`: categories (`CatDef`) hold flat lists of actions (`ActionDef`), and each action either opens a screen (`Launch::Screen(fn(&App) -> Box<dyn Screen>)`) or runs immediately behind an optional confirmation (`Launch::Run(RunSpec)` with tag/confirm/danger/build/ok_msg/fail_msg/done_log). Adding a feature = one row there plus its module; nothing else needs touching.

## Commands

```bash
cargo build --release        # release binary at target/release/archman
cargo check                  # fast type-check (use before building)
cargo test                   # unit tests (settings epoch/toggle, fuzzy matcher)
./target/release/archman     # run the TUI
./install.sh                 # build + install to /usr/local/bin
```

- CLI surface: `--version/-v`, `--help/-h`; anything else is the interactive TUI.
- The app mutates the system through `sudo pacman` etc. and requires a real terminal — never run it expecting non-interactive completion.
- There is no headless test harness for the TUI itself. To smoke-test rendering/keys, drive it under a pty with an explicit winsize (e.g. Python `pty.fork()` + `TIOCSWINSZ`); a bare `script -qec` in CI-like shells reports a 0×0 window and renders nothing.

## Architecture

Single binary, one module per feature:

- `main.rs` — CLI flags, raw-mode/alternate-screen setup, panic hook that restores the terminal, `restore_terminal()`/`enter_tui()` used around external commands.
- `app.rs` — the core. Owns:
  - the **screen stack** (`Vec<Box<dyn Screen>>`, main menu always at index 0),
  - **modals** (`Confirm`/`Input`; empty input = cancel, mirroring v1's bash semantics),
  - toasts, and the queue of **external commands** (`ExtCmd`).
- `widgets.rs` — reusable pieces: header logo, help overlay, `Menu`, `FuzzyList`, `TextViewer`, `kv_table`, spinner. All styling goes through these helpers; screens never touch crossterm directly.
- `sys.rs` — every external interaction: pacman/AUR queries (`si`, `qi`, `-Slq`, orphans, explicit/foreign lists…), capability detection (`Caps::detect`, `has_bin`), mirror/cache/log parsing, and `Job<T>` (background thread + channel so slow/network queries don't freeze drawing).
- `screens/*` — one module per feature, each exposing small single-purpose screens implementing the `Screen` trait (`handle_key`/`poll`/`draw`/`busy`/`on_confirm`/`on_input`/`on_ext_done`). Shared helpers live in `screens/mod.rs` (`info_lines`, `args`). Query-style screens (OwnerQueryScreen, InfoQueryScreen, ImportScreen) open their input modal from `poll()` on the first tick via an `asked` flag — constructors only get `&App`.

### Conventions that span files

- **Flat state, not data enums.** Screens keep a `mode: Mode` discriminant (Copy enum) plus sibling fields (`pick: FuzzyList`, `detail: Detail`, …). Never write the matched field inside its own match arm (`self.state = …` while matching on `&mut self.state`) — it won't borrow-check; assign sibling fields instead.
- **The dispatch pitfall.** `App::dispatch` pops the top screen, runs the closure, then re-inserts it *at its original depth* so screens pushed during handling end up above it, and skipped entirely if the screen called `app.pop()`. If you change this dance, the symptom of getting it wrong is "navigation pushes a screen but the UI keeps drawing the old one".
- **External commands run in an embedded pane.** Screens call `app.queue_ext(ExtCmd::new(tag, program, &args))`; the main loop spawns the child on a pseudo-terminal (`pty.rs`) and pushes a `RunPane` screen that streams its output through a mini terminal emulator (ANSI-stripping, \r-overwrite aware). Keystrokes are forwarded to the child, so sudo passwords and pacman `[Y/n]` prompts work in-pane; `pgup/pgdn` scroll locally; closing the pane routes `(tag, exit_status)` back via `Screen::on_ext_done` (`App::complete_ext`). One pane runs at a time (`ext_active` gates the queue); `RunPane::drop` kills an abandoned child. The dashboard is a permanent shell: `App::draw` always paints `screens[0]` first, then renders whichever screen is top — and the embedded runner — into `screens[0].content_area()` (the action pane), linutil-style. When a command spawns, `App` unwinds the stack (`screens.truncate(1)`) so the run happens on the dashboard. Commands queued away from home attach feedback via `ExtCmd::result(ok_msg, fail_msg, done_log)` — `App` toasts/logs those itself since the originating screen is gone; dashboard `RunSpec` results still route through `HomeScreen::on_ext_done`. Border: cyan running, green success, red failure.
- **Slow queries are jobs.** Anything that can take seconds (AUR `-Si`/`-Ss`, package-list fetches, `checkupdates`) is spawned with `Job::spawn(label, closure)` in a screen's constructor/action; `poll()` drains the result each tick, and `busy()` surfaces the spinner overlay while pending.
- **Config compatibility.** `settings.rs` reads/writes the exact v1 format (`~/.config/archman/settings.conf`, `favorites.txt`, `archman.log`). Settings values live in a typed map accessed via `app.settings()/settings_mut()`; screens that need them at draw time keep a `Snapshot` copy refreshed after changes (draw has no App access).
- **AUR helper resolution** always goes through `app.aur_helper()` / `sys::get_aur_helper` (configured helper if installed → yay → paru → None). Bulk installs split official vs AUR with one batched `pacman -Si` call (`sys::filter_official`), then queue `sudo pacman -S --needed …` followed by `<helper> -S --needed …`.
- **Logging**: user-visible actions call `app.log("VERB: message")` with the same wording style as v1 (`INSTALL:`, `REMOVE:`, `CACHE:`…).

### Adding an action

1. Add `src/screens/<feature>.rs` with a `Screen` impl following the flat-state convention (or a `RunSpec` builder for one-shot commands).
2. Register the module in `src/screens/mod.rs`.
3. Add an `ActionDef` row to the right category in `screens/registry.rs`.

Registry macro gotcha: in `action!`, the `run = …` arm must stay **before** the plain arm — `run = run_spec!(…)` otherwise parses as an assignment expression and silently matches the wrong arm.

### Global keys (App::on_key)

Ctrl+C quits anywhere; while the cheatsheet overlay is open any key closes it; modals next; everything else dispatches to the top screen. Home-specific: digits 1–7 jump categories, tab/shift+tab switch, `/` searches all actions, `?` toggles help, `q` quits.
