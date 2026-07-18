# archman — Interactive Arch Linux System Manager

<p align="center">
  <b>A full-featured, menu-driven terminal utility for managing your Arch Linux system.</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Arch_Linux-1793D1?style=for-the-badge&logo=arch-linux&logoColor=white" />
  <img src="https://img.shields.io/badge/Shell-Bash-4EAA25?style=for-the-badge&logo=gnu-bash&logoColor=white" />
  <img src="https://img.shields.io/badge/License-GPL_2.0-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/Version-1.0.0-green?style=for-the-badge" />
</p>

---

## ✨ Features

| Feature | Description |
|---------|-------------|
| 📦 **Install Package** | Auto-detects official repos vs AUR, dry-run mode |
| 🗑 **Remove Package** | Fuzzy-search installed packages, 3 removal strategies |
| 🔍 **Search Packages** | Searches official repos + AUR, colored results |
| 📋 **Package Utilities** | List, orphans, file ownership, info, history, reinstall |
| ⬆ **System Update** | Database refresh, pacman upgrade, AUR upgrade |
| 🧹 **Cache Cleaning** | paccache integration, configurable keep count |
| 🔒 **Lock File** | Detect & remove `db.lck` with safety checks |
| 🌍 **Mirror Management** | reflector integration, backup & restore |
| 📊 **System Information** | Comprehensive system overview dashboard |
| ⭐ **Favorites** | Save, sync, and bulk-install your must-have packages |
| 📦 **Package Groups** | 12 curated groups (GNOME, KDE, Hyprland, Dev, Gaming...) |
| 📤 **Export / Import** | Backup and restore package lists across systems |
| ⚙ **Settings** | AUR helper, dry-run, logging, and more |
| 📋 **Transaction Logs** | View pacman history with filters |

## 🎨 Beautiful TUI

archman is built around [**gum**](https://github.com/charmbracelet/gum) for a polished, modern terminal experience:

- 🎯 Arrow-key menu navigation
- 🔍 Fuzzy filtering for package lists
- ✅ Styled confirmation dialogs
- ⏳ Animated spinners for long operations
- 📄 Scrollable pager for logs

> **Works without gum too!** Plain-text fallbacks ensure the script runs on any terminal.

## 📦 Installation

### Quick Install

```bash
git clone https://github.com/ankur3/pacman-utils.git
cd pacman-utils
chmod +x install.sh
./install.sh
```

### Manual Install

```bash
sudo cp archman /usr/local/bin/archman
sudo mkdir -p /usr/local/lib/archman
sudo cp lib/*.sh /usr/local/lib/archman/
sudo chmod 755 /usr/local/bin/archman
```

### Run From Source

```bash
chmod +x archman
./archman
```

## 🔧 Dependencies

| Dependency | Required | Purpose |
|------------|----------|---------|
| `pacman` | ✅ Yes | Core package manager |
| `bash` ≥ 4.0 | ✅ Yes | Shell |
| [`gum`](https://github.com/charmbracelet/gum) | ⭐ Recommended | Beautiful TUI components |
| [`fzf`](https://github.com/junegunn/fzf) | Optional | Fuzzy finder (fallback) |
| `yay` / `paru` | Optional | AUR helper |
| `reflector` | Optional | Mirror management |
| `pacman-contrib` | Optional | Cache cleaning (`paccache`) |

Install recommended dependencies:

```bash
sudo pacman -S gum fzf reflector pacman-contrib
```

## ⚙ Configuration

Configuration is stored at `~/.config/archman/`:

| File | Purpose |
|------|---------|
| `settings.conf` | All settings (AUR helper, dry-run, etc.) |
| `favorites.txt` | Your favorite packages list |
| `archman.log` | Activity log |

### Settings

| Setting | Default | Options |
|---------|---------|---------|
| `AUR_HELPER` | `yay` | `yay`, `paru` |
| `DRY_RUN` | `false` | `true`, `false` |
| `CONFIRM_ACTIONS` | `true` | `true`, `false` |
| `LOG_ENABLED` | `true` | `true`, `false` |
| `PACCACHE_KEEP` | `3` | `1`-`10` |

## 📦 Package Groups

Pre-configured package groups for quick setup:

| Group | Packages |
|-------|----------|
| 🖥 GNOME | `gnome gnome-extra gdm gnome-tweaks` |
| 🖥 KDE Plasma | `plasma kde-applications sddm` |
| 🪟 Hyprland | `hyprland waybar wofi kitty swaybg swaylock mako grim slurp` |
| 🪟 Sway | `sway swaylock swayidle waybar wofi foot mako grim slurp` |
| 🪟 i3 | `i3-wm i3status i3lock dmenu alacritty picom feh dunst` |
| 💻 Development | `base-devel git nodejs npm python go rustup docker` |
| 🎮 Gaming | `steam lutris wine-staging gamemode lib32-mesa mangohud` |
| 🎬 Multimedia | `vlc obs-studio gimp inkscape audacity ffmpeg mpv` |
| 🌐 Networking | `networkmanager openssh curl wget nmap wireshark-qt` |
| 🔤 Fonts | `ttf-dejavu ttf-liberation noto-fonts ttf-fira-code ttf-jetbrains-mono` |
| 💻 Terminal | `zsh fish starship tmux neovim htop btop bat eza fd ripgrep fzf gum` |
| 🔒 Security | `ufw gufw clamav firejail keepassxc gnupg` |

## 📖 Usage

```bash
# Launch interactive menu
archman

# Show version
archman --version

# Show help
archman --help
```

## 🏗 Project Structure

```
pacman-utils/
├── archman              # Main entry point
├── install.sh           # System-wide installer
├── lib/
│   ├── ui.sh            # UI library (gum wrappers, fallbacks)
│   ├── settings.sh      # Settings management
│   ├── install.sh       # Package installation
│   ├── remove.sh        # Package removal
│   ├── search.sh        # Package search
│   ├── update.sh        # System updates
│   ├── cache.sh         # Cache cleaning
│   ├── lockfile.sh      # Lock file management
│   ├── mirrors.sh       # Mirror management
│   ├── info.sh          # System information
│   ├── packages.sh      # Package utilities
│   ├── export_import.sh # Export/import lists
│   ├── favorites.sh     # Favorites management
│   └── groups.sh        # Package group installer
├── LICENSE              # GPL-2.0
└── README.md            # This file
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📄 License

This project is licensed under the **GNU General Public License v2.0** — see the [LICENSE](LICENSE) file for details.
