#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/groups.sh — Pre-defined package group installer
# ──────────────────────────────────────────────────────────────────────

# ── Group Definitions ────────────────────────────────────────────────
# Each group is defined as GROUP_<name>="pkg1 pkg2 pkg3"
# and GROUP_DESC_<name>="description"

declare -A PKG_GROUPS
declare -A GROUP_DESC

PKG_GROUPS[gnome]="gnome gnome-extra gdm gnome-tweaks"
GROUP_DESC[gnome]="GNOME Desktop Environment"

PKG_GROUPS[kde]="plasma kde-applications sddm"
GROUP_DESC[kde]="KDE Plasma Desktop"

PKG_GROUPS[hyprland]="hyprland waybar wofi kitty swaybg swaylock mako grim slurp"
GROUP_DESC[hyprland]="Hyprland Wayland Compositor"

PKG_GROUPS[sway]="sway swaylock swayidle waybar wofi foot mako grim slurp"
GROUP_DESC[sway]="Sway Wayland Compositor"

PKG_GROUPS[i3]="i3-wm i3status i3lock dmenu alacritty picom feh dunst"
GROUP_DESC[i3]="i3 Window Manager"

PKG_GROUPS[dev]="base-devel git nodejs npm python python-pip go rustup docker docker-compose"
GROUP_DESC[dev]="Development Essentials"

PKG_GROUPS[gaming]="steam lutris wine-staging gamemode lib32-mesa lib32-vulkan-icd-loader mangohud"
GROUP_DESC[gaming]="Gaming (Steam, Lutris, Wine)"

PKG_GROUPS[multimedia]="vlc obs-studio gimp inkscape audacity ffmpeg mpv imagemagick"
GROUP_DESC[multimedia]="Multimedia (Video, Audio, Graphics)"

PKG_GROUPS[networking]="networkmanager nm-connection-editor openssh curl wget nmap wireshark-qt"
GROUP_DESC[networking]="Networking Tools"

PKG_GROUPS[fonts]="ttf-dejavu ttf-liberation noto-fonts noto-fonts-cjk noto-fonts-emoji ttf-fira-code ttf-jetbrains-mono"
GROUP_DESC[fonts]="Essential Fonts"

PKG_GROUPS[terminal]="zsh fish starship tmux neovim htop btop bat eza fd ripgrep fzf gum"
GROUP_DESC[terminal]="Terminal Power Tools"

PKG_GROUPS[security]="ufw gufw clamav firejail keepassxc gnupg"
GROUP_DESC[security]="Security Tools"

# ── Groups Menu ──────────────────────────────────────────────────────
groups_menu() {
    while true; do
        ui_clear
        ui_header "📦  Package Groups"

        # Build menu items
        local items=()
        for group_key in gnome kde hyprland sway i3 dev gaming multimedia networking fonts terminal security; do
            items+=("${GROUP_DESC[$group_key]}")
        done
        items+=("🔙 Back to Main Menu")

        local choice
        choice=$(ui_choose "${items[@]}")

        case "$choice" in
            *"Back"*|"")
                return
                ;;
            *)
                # Find matching group
                local found_key=""
                for group_key in "${!GROUP_DESC[@]}"; do
                    if [[ "${GROUP_DESC[$group_key]}" == "$choice" ]]; then
                        found_key="$group_key"
                        break
                    fi
                done

                if [[ -n "$found_key" ]]; then
                    _group_install "$found_key"
                fi
                ;;
        esac
    done
}

_group_install() {
    local group_key="$1"
    local packages="${PKG_GROUPS[$group_key]}"
    local desc="${GROUP_DESC[$group_key]}"

    ui_header "$desc"

    # Show packages
    local pkg_array
    read -ra pkg_array <<< "$packages"
    local total=${#pkg_array[@]}

    ui_info "$total packages in this group:"
    echo ""

    for pkg in "${pkg_array[@]}"; do
        local status
        if pacman -Q "$pkg" &>/dev/null; then
            status="${CLR_GREEN}[installed]${CLR_RESET}"
        elif pacman -Si "$pkg" &>/dev/null; then
            status="${CLR_DIM}[available]${CLR_RESET}"
        else
            status="${CLR_YELLOW}[AUR/missing]${CLR_RESET}"
        fi
        echo -e "  ${CLR_CYAN}•${CLR_RESET} $pkg $status"
    done
    echo ""

    local method
    method=$(ui_choose \
        "📦 Install all packages" \
        "✅ Install only missing packages" \
        "🔍 Select specific packages" \
        "🔙 Cancel"
    )

    case "$method" in
        *"Install all"*)
            if ui_confirm "Install all $total packages?"; then
                log_action "PKG_GROUPS: Installing all packages for $desc"
                _group_do_install "${pkg_array[@]}"
            fi
            ;;
        *"only missing"*)
            local missing=()
            for pkg in "${pkg_array[@]}"; do
                if ! pacman -Q "$pkg" &>/dev/null; then
                    missing+=("$pkg")
                fi
            done
            if [[ ${#missing[@]} -eq 0 ]]; then
                ui_success "All packages are already installed!"
            elif ui_confirm "Install ${#missing[@]} missing packages?"; then
                log_action "PKG_GROUPS: Installing ${#missing[@]} missing for $desc"
                _group_do_install "${missing[@]}"
            fi
            ;;
        *"Select specific"*)
            local selected
            selected=$(printf '%s\n' "${pkg_array[@]}" | ui_filter "Select packages...")
            if [[ -n "$selected" ]]; then
                local selected_pkg
                selected_pkg=$(echo "$selected" | awk '{print $1}')
                if ui_confirm "Install $selected_pkg?"; then
                    install_specific_package "$selected_pkg"
                fi
            fi
            ;;
        *)
            ui_info "Cancelled."
            ;;
    esac

    ui_pause
}

_group_do_install() {
    local pkgs=("$@")

    # Separate official and AUR
    local official=() aur=()
    for pkg in "${pkgs[@]}"; do
        if pacman -Si "$pkg" &>/dev/null; then
            official+=("$pkg")
        else
            aur+=("$pkg")
        fi
    done

    if [[ ${#official[@]} -gt 0 ]]; then
        ui_info "Installing ${#official[@]} official packages..."
        sudo pacman -S --needed "${official[@]}"
    fi

    if [[ ${#aur[@]} -gt 0 ]]; then
        local aur_helper
        if aur_helper=$(get_aur_helper); then
            ui_info "Installing ${#aur[@]} AUR packages via $aur_helper..."
            $aur_helper -S --needed "${aur[@]}"
        else
            ui_warn "Cannot install AUR packages — no AUR helper found:"
            printf '  %s\n' "${aur[@]}"
        fi
    fi

    ui_success "Group installation complete!"
}
