// ──────────────────────────────────────────────────────────────────────
// main.rs — archman entry point
//
// Handles CLI flags, terminal setup/teardown (raw mode + alternate
// screen), and the panic hook that restores the terminal.
// ──────────────────────────────────────────────────────────────────────

mod app;
pub mod cmd;
mod fuzzy;
mod pty;
mod screens;
mod settings;
mod sys;
pub mod tx;
mod widgets;

use std::io;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-v") => {
            println!("archman v{VERSION}");
            return;
        }
        Some("--help" | "-h") => {
            print_help();
            return;
        }
        Some(other) => {
            eprintln!("Unknown option: {other}");
            eprintln!("Run 'archman --help' for usage.");
            std::process::exit(1);
        }
        None => {}
    }

    if let Err(e) = run_tui() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_tui() -> io::Result<()> {
    // Restore the terminal even if we panic mid-draw.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::App::new().run(&mut terminal);

    restore_terminal()?;
    result
}

/// Return the terminal to cooked mode / normal screen. Safe to call twice.
pub fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn print_help() {
    let config_path = settings::config_dir();
    let settings_dir = config_path.display();
    println!(
        r#"archman — Interactive Arch Linux System Manager (ratatui edition)

Usage:
  archman           Launch the interactive TUI
  archman -v        Show version
  archman -h        Show this help

Configuration:
  {settings_dir}/settings.conf    Settings
  {settings_dir}/favorites.txt    Favorite packages
  {settings_dir}/archman.log      Activity log

Dependencies (optional):
  yay / paru        AUR helper
  brew              Homebrew package manager
  reflector         Mirror management
  pacman-contrib    Cache cleaning (paccache) and update checks (checkupdates)
"#
    );
}
