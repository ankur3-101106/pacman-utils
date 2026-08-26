# Contributing to archman

Thanks for your interest in improving archman! 🎉 Whether it's a bug report, a new action in the dashboard, or a rendering fix — all contributions are welcome.

## Getting started

You need a Rust toolchain ([rustup](https://rustup.rs) or `sudo pacman -S rust`) and, ideally, an Arch machine to test on — archman shells out to `pacman` and friends.

```bash
git clone https://github.com/ankur3-101106/pacman-utils.git
cd pacman-utils
cargo build --release        # release binary at target/release/archman
cargo check                  # fast type-check while iterating
cargo test                   # unit tests
./target/release/archman     # try it (requires a real terminal)
```

## How the codebase is organized

The whole user-facing surface is declared in **`src/screens/registry.rs`**: categories hold flat lists of actions, and each action either opens a screen or runs a command directly (optionally behind a confirmation). One module per feature lives in `src/screens/`.

**Adding a feature is deliberately small:**

1. Create `src/screens/<feature>.rs` implementing the `Screen` trait (see existing modules for the flat-state pattern), or write a `RunSpec` builder for a one-shot command.
2. Register the module in `src/screens/mod.rs`.
3. Add an `ActionDef` row to the right category in `src/screens/registry.rs`.

## Conventions

- **Styling flows through `src/widgets.rs`** — screens never touch crossterm directly, and the helpers honor `NO_COLOR` automatically. Please don't hard-code colors or glyphs in feature code.
- **Slow/network queries are background jobs** (`sys::Job::spawn`) so the UI never freezes; drain results in the screen's `poll()`.
- **External commands run in the embedded pane** via `app.queue_ext(ExtCmd::new(...))` — never spawn blocking processes on the UI thread.
- **Log user-visible actions** with `app.log("VERB: message")`, matching the existing wording style (`INSTALL:`, `REMOVE:`, `CACHE:`, …).
- There is no enforced rustfmt/clippy config — match the style of the surrounding code.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `chore:` — matching the existing history (e.g. `chore: release v2.0.0`).

## Pull request checklist

Before opening a PR:

- [ ] `cargo check` passes with no warnings
- [ ] `cargo test` passes
- [ ] the app still renders legibly with `NO_COLOR=1`
- [ ] new user-facing actions are registered in `src/screens/registry.rs` with a clear description line
- [ ] README (features table / project structure) is updated if the change is user-visible

Open the PR against `main` with a short description of what changed and why. Screenshots are appreciated for UI changes.

## Reporting bugs

Open a [GitHub issue](https://github.com/ankur3-101106/pacman-utils/issues) with your `archman --version`, terminal environment, and steps to reproduce. For security issues, please follow [SECURITY.md](SECURITY.md) instead — don't open a public issue.

By participating in this project you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

archman is licensed under **GPL-2.0**. By contributing, you agree that your contributions will be licensed under the same license.
