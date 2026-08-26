#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/install.sh — Package installation for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

install_menu() {
    ui_clear
    ui_header "📦  Install Package"

    local list_cmd="pacman -Slq"
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        list_cmd="$aur_helper -Slq"
    fi

    ui_info "Fetching available packages..."
    local pkg
    pkg=$($list_cmd 2>/dev/null | ui_filter "Select package to install..." || true)

    # Empty input
    if [[ -z "$pkg" ]]; then
        ui_warn "No package selected."
        ui_pause
        return
    fi

    log_action "INSTALL: Selected '$pkg'"

    # ── Check official repositories ──────────────────────────────────
    ui_info "Searching official repositories..."
    local repo_info
    if repo_info=$(pacman -Si "$pkg" 2>/dev/null); then
        echo ""
        ui_success "Found in official repositories."
        echo ""

        # Show brief info
        echo "$repo_info" | grep -E '^(Name|Version|Repository|Description|Download Size)' | while IFS=: read -r key val; do
            printf "  ${CLR_CYAN}%-16s${CLR_RESET}%s\n" "$key:" "$val"
        done
        echo ""

        # Dry-run check
        if [[ "${SETTINGS[DRY_RUN]}" == "true" ]]; then
            ui_info "Dry-run mode: showing what would be installed..."
            sudo pacman -S --print "$pkg" || ui_error "Could not resolve $pkg for installation."
            echo ""
            if ! ui_confirm "Proceed with actual install?"; then
                ui_info "Installation cancelled."
                ui_pause
                return
            fi
        fi

        if ui_confirm "Install $pkg?"; then
            echo ""
            log_action "INSTALL: Installing $pkg from official repos"
            if sudo pacman -S "$pkg"; then
                ui_success "$pkg installed successfully!"
                log_action "INSTALL: $pkg installed successfully"
            else
                ui_error "Installation failed."
                log_action "INSTALL: $pkg installation failed"
            fi
        else
            ui_info "Installation cancelled."
        fi
        ui_pause
        return
    fi

    # ── Check AUR ────────────────────────────────────────────────────
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        ui_info "Not in official repos. Searching AUR..."
        echo ""

        local aur_info
        if aur_info=$($aur_helper -Si "$pkg" 2>/dev/null); then
            ui_success "Found in AUR."
            echo ""

            echo "$aur_info" | grep -E '^(Name|Version|Description|Maintainer|Votes)' | while IFS=: read -r key val; do
                printf "  ${CLR_CYAN}%-16s${CLR_RESET}%s\n" "$key:" "$val"
            done
            echo ""

            if ui_confirm "Install $pkg from AUR?"; then
                echo ""
                log_action "INSTALL: Installing $pkg from AUR via $aur_helper"
                if $aur_helper -S "$pkg"; then
                    ui_success "$pkg installed successfully!"
                    log_action "INSTALL: $pkg installed successfully from AUR"
                else
                    ui_error "Installation failed."
                    log_action "INSTALL: $pkg AUR installation failed"
                fi
            else
                ui_info "Installation cancelled."
            fi
            ui_pause
            return
        fi
    fi

    # ── Not found anywhere ──────────────────────────────────────────
    echo ""
    ui_error "Package '$pkg' not found in official repos or AUR."
    echo ""

    if ui_confirm "Search for similar packages?"; then
        _install_search_similar "$pkg"
    fi

    ui_pause
}

# ── Search for similar packages and offer install ────────────────────
_install_search_similar() {
    local term="$1"
    local results=""

    ui_info "Searching for packages matching '$term'..."
    echo ""

    # Official repos
    local official
    official=$(pacman -Ss "$term" 2>/dev/null | grep -E '^[a-z]' | head -20)
    if [[ -n "$official" ]]; then
        echo -e "${CLR_BOLD}Official Repositories:${CLR_RESET}"
        echo "$official" | while read -r line; do
            echo "  $line"
        done
        results+="$official"$'\n'
        echo ""
    fi

    # AUR
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        local aur_results
        aur_results=$($aur_helper -Ss "$term" 2>/dev/null | grep -E '^aur/' | head -20)
        if [[ -n "$aur_results" ]]; then
            echo -e "${CLR_BOLD}AUR:${CLR_RESET}"
            echo "$aur_results" | while read -r line; do
                echo "  $line"
            done
            results+="$aur_results"
            echo ""
        fi
    fi

    if [[ -z "$results" ]]; then
        ui_error "No similar packages found."
        return
    fi

    echo ""
    if ui_confirm "Install one of these?"; then
        local selected_pkg
        selected_pkg=$(ui_input "Package name" "Enter exact package name to install")
        if [[ -n "$selected_pkg" ]]; then
            # Extract just the package name (remove repo/ prefix and version)
            selected_pkg=$(echo "$selected_pkg" | sed 's|.*/||' | awk '{print $1}')
            install_specific_package "$selected_pkg"
        fi
    fi
}

# ── Install a specific package (used by search, groups, etc.) ────────
install_specific_package() {
    local pkg="$1"

    if pacman -Si "$pkg" &>/dev/null; then
        log_action "INSTALL: Installing $pkg from official repos"
        if sudo pacman -S "$pkg"; then
            ui_success "$pkg installed successfully!"
            log_action "INSTALL: $pkg installed successfully"
        else
            ui_error "Installation of $pkg failed."
            log_action "INSTALL: $pkg installation failed"
        fi
    else
        local aur_helper
        if aur_helper=$(get_aur_helper); then
            log_action "INSTALL: Installing $pkg from AUR via $aur_helper"
            if $aur_helper -S "$pkg"; then
                ui_success "$pkg installed successfully!"
                log_action "INSTALL: $pkg installed successfully from AUR"
            else
                ui_error "Installation of $pkg failed."
                log_action "INSTALL: $pkg AUR installation failed"
            fi
        else
            ui_error "Cannot install $pkg — no AUR helper found."
        fi
    fi
}
