#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/search.sh — Package search for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

search_menu() {
    ui_clear
    ui_header "🔍  Search Packages"

    local list_cmd="pacman -Slq"
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        list_cmd="$aur_helper -Slq"
    fi

    ui_info "Fetching available packages..."
    local pkg
    pkg=$($list_cmd 2>/dev/null | ui_filter "Search packages..." || true)

    if [[ -z "$pkg" ]]; then
        ui_warn "No package selected."
        ui_pause
        return
    fi

    log_action "SEARCH: Selected '$pkg'"

    # Show info about the package
    if pacman -Si "$pkg" &>/dev/null; then
        echo -e "\n${CLR_BOLD}${CLR_GREEN}Official Repository Info:${CLR_RESET}\n"
        pacman -Si "$pkg"
    elif [[ -n "$aur_helper" ]] && $aur_helper -Si "$pkg" &>/dev/null; then
        echo -e "\n${CLR_BOLD}${CLR_MAGENTA}AUR Info:${CLR_RESET}\n"
        $aur_helper -Si "$pkg"
    else
        ui_error "Could not retrieve info for '$pkg'."
    fi

    # ── Offer to install ─────────────────────────────────────────────
    echo ""
    if ui_confirm "Install $pkg?"; then
        install_specific_package "$pkg"
    fi

    ui_pause
}
