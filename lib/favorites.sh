#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/favorites.sh — Favorite packages management
# ──────────────────────────────────────────────────────────────────────

favorites_menu() {
    while true; do
        ui_clear
        ui_header "⭐  Favorite Packages"

        # Show current favorites
        local fav_count=0
        if [[ -f "$FAVORITES_FILE" ]] && [[ -s "$FAVORITES_FILE" ]]; then
            fav_count=$(wc -l < "$FAVORITES_FILE")
            ui_info "$fav_count favorite(s):"
            echo ""
            local i=1
            while read -r pkg; do
                local installed=""
                if pacman -Q "$pkg" &>/dev/null; then
                    installed="${CLR_GREEN}[installed]${CLR_RESET}"
                else
                    installed="${CLR_DIM}[not installed]${CLR_RESET}"
                fi
                echo -e "  ${CLR_YELLOW}★${CLR_RESET} $pkg $installed"
                ((i++))
            done < "$FAVORITES_FILE"
        else
            ui_dim "  No favorites yet. Add packages to get started!"
        fi
        echo ""

        local choice
        choice=$(ui_choose \
            "➕ Add package to favorites" \
            "➖ Remove package from favorites" \
            "📦 Install all favorites" \
            "📦 Install missing favorites" \
            "🔙 Back to Main Menu"
        )

        case "$choice" in
            *"Add package"*)
                _fav_add
                ;;
            *"Remove package"*)
                _fav_remove
                ;;
            *"Install all"*)
                _fav_install_all
                ;;
            *"Install missing"*)
                _fav_install_missing
                ;;
            *"Back"*|"")
                return
                ;;
        esac
    done
}

_fav_add() {
    local pkg
    pkg=$(ui_input "Package name" "Add to favorites")

    if [[ -z "$pkg" ]]; then
        return
    fi

    # Check if already in favorites
    if grep -qx "$pkg" "$FAVORITES_FILE" 2>/dev/null; then
        ui_warn "$pkg is already in favorites."
        sleep 1
        return
    fi

    echo "$pkg" >> "$FAVORITES_FILE"
    ui_success "Added $pkg to favorites!"
    log_action "FAVORITES: Added $pkg"
    sleep 1
}

_fav_remove() {
    if [[ ! -s "$FAVORITES_FILE" ]]; then
        ui_warn "No favorites to remove."
        sleep 1
        return
    fi

    local selected
    selected=$(cat "$FAVORITES_FILE" | ui_filter "Select package to remove...")

    if [[ -n "$selected" ]]; then
        # Remove from file. grep exits 1 when the selection was the only
        # favorite (empty result is expected), hence the status guard.
        local tmp
        tmp=$(mktemp)
        grep -vx -- "$selected" "$FAVORITES_FILE" > "$tmp" || true
        mv "$tmp" "$FAVORITES_FILE"
        ui_success "Removed $selected from favorites."
        log_action "FAVORITES: Removed $selected"
        sleep 1
    fi
}

_fav_install_all() {
    if [[ ! -s "$FAVORITES_FILE" ]]; then
        ui_warn "No favorites to install."
        ui_pause
        return
    fi

    local count
    count=$(wc -l < "$FAVORITES_FILE")

    if ui_confirm "Install all $count favorite packages?"; then
        log_action "FAVORITES: Installing all $count favorites"

        # Separate official and AUR packages
        local official=() aur=()
        while read -r pkg; do
            if pacman -Si "$pkg" &>/dev/null; then
                official+=("$pkg")
            else
                aur+=("$pkg")
            fi
        done < "$FAVORITES_FILE"

        # Install official
        if [[ ${#official[@]} -gt 0 ]]; then
            ui_info "Installing ${#official[@]} official packages..."
            sudo pacman -S --needed "${official[@]}" || ui_error "Some official packages could not be installed."
        fi

        # Install AUR
        if [[ ${#aur[@]} -gt 0 ]]; then
            local aur_helper
            if aur_helper=$(get_aur_helper); then
                ui_info "Installing ${#aur[@]} AUR packages via $aur_helper..."
                $aur_helper -S --needed "${aur[@]}" || ui_error "Some AUR packages could not be installed."
            else
                ui_warn "Skipping ${#aur[@]} AUR packages — no AUR helper found."
            fi
        fi

        ui_success "Favorites installation complete!"
        log_action "FAVORITES: Installation complete"
    fi

    ui_pause
}

_fav_install_missing() {
    if [[ ! -s "$FAVORITES_FILE" ]]; then
        ui_warn "No favorites to install."
        ui_pause
        return
    fi

    local missing=()
    while read -r pkg; do
        if ! pacman -Q "$pkg" &>/dev/null; then
            missing+=("$pkg")
        fi
    done < "$FAVORITES_FILE"

    if [[ ${#missing[@]} -eq 0 ]]; then
        ui_success "All favorite packages are already installed!"
        ui_pause
        return
    fi

    ui_info "${#missing[@]} favorite(s) not installed:"
    echo ""
    for pkg in "${missing[@]}"; do
        echo -e "  ${CLR_DIM}•${CLR_RESET} $pkg"
    done
    echo ""

    if ui_confirm "Install ${#missing[@]} missing packages?"; then
        log_action "FAVORITES: Installing ${#missing[@]} missing favorites"

        local official=() aur=()
        for pkg in "${missing[@]}"; do
            if pacman -Si "$pkg" &>/dev/null; then
                official+=("$pkg")
            else
                aur+=("$pkg")
            fi
        done

        if [[ ${#official[@]} -gt 0 ]]; then
            sudo pacman -S --needed "${official[@]}" || ui_error "Some official packages could not be installed."
        fi

        if [[ ${#aur[@]} -gt 0 ]]; then
            local aur_helper
            if aur_helper=$(get_aur_helper); then
                $aur_helper -S --needed "${aur[@]}" || ui_error "Some AUR packages could not be installed."
            else
                ui_warn "Skipping AUR packages — no AUR helper."
            fi
        fi

        ui_success "Missing favorites installed!"
        log_action "FAVORITES: Missing favorites installed"
    fi

    ui_pause
}
