#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/search.sh — Package search for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

search_menu() {
    ui_clear
    ui_header "🔍  Search Packages"

    local term
    term=$(ui_input "Search term..." "Enter search query")

    if [[ -z "$term" ]]; then
        ui_warn "No search term entered."
        ui_pause
        return
    fi

    log_action "SEARCH: Searching for '$term'"

    local found_any=false

    # ── Official repositories ────────────────────────────────────────
    ui_info "Searching official repositories..."
    echo ""

    local official_results
    official_results=$(pacman -Ss "$term" 2>/dev/null)

    if [[ -n "$official_results" ]]; then
        found_any=true
        echo -e "${CLR_BOLD}${CLR_GREEN}Official Repositories:${CLR_RESET}"
        echo ""

        # Parse and display results nicely
        echo "$official_results" | while IFS= read -r line; do
            if [[ "$line" =~ ^[a-z] ]]; then
                # Package header line (repo/name version)
                local repo_pkg version
                repo_pkg=$(echo "$line" | awk '{print $1}')
                version=$(echo "$line" | awk '{print $2}')
                local installed_marker=""
                if echo "$line" | grep -q '\[installed\]'; then
                    installed_marker=" ${CLR_GREEN}[installed]${CLR_RESET}"
                fi
                echo -e "  ${CLR_CYAN}${repo_pkg}${CLR_RESET} ${CLR_DIM}${version}${CLR_RESET}${installed_marker}"
            else
                # Description line
                echo -e "    ${CLR_DIM}${line}${CLR_RESET}"
            fi
        done | head -40
        echo ""
    fi

    # ── AUR ──────────────────────────────────────────────────────────
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        ui_info "Searching AUR..."
        echo ""

        local aur_results
        aur_results=$($aur_helper -Ss "$term" 2>/dev/null | grep -E '^aur/')

        if [[ -n "$aur_results" ]]; then
            found_any=true
            echo -e "${CLR_BOLD}${CLR_MAGENTA}AUR:${CLR_RESET}"
            echo ""

            echo "$aur_results" | head -20 | while IFS= read -r line; do
                local pkg_name version votes
                pkg_name=$(echo "$line" | awk '{print $1}')
                version=$(echo "$line" | awk '{print $2}')
                echo -e "  ${CLR_MAGENTA}${pkg_name}${CLR_RESET} ${CLR_DIM}${version}${CLR_RESET}"
            done
            echo ""
        fi
    fi

    if ! $found_any; then
        ui_error "No packages found matching '$term'."
        ui_pause
        return
    fi

    # ── Offer to install ─────────────────────────────────────────────
    echo ""
    if ui_confirm "Install a package from the results?"; then
        local selected
        selected=$(ui_input "Package name" "Enter exact package name")
        if [[ -n "$selected" ]]; then
            # Strip repo prefix if present
            selected=$(echo "$selected" | sed 's|.*/||' | awk '{print $1}')
            install_specific_package "$selected"
        fi
    fi

    ui_pause
}
