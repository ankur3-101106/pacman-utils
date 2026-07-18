#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/remove.sh — Package removal for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

remove_menu() {
    ui_clear
    ui_header "🗑  Remove Package"

    # Get list of explicitly installed packages
    local packages
    packages=$(pacman -Qe | awk '{print $1 " (" $2 ")"}')

    if [[ -z "$packages" ]]; then
        ui_error "No explicitly installed packages found."
        ui_pause
        return
    fi

    ui_info "Select package(s) to remove (fuzzy search):"
    echo ""

    local selected
    selected=$(echo "$packages" | ui_filter "Search installed packages...")

    if [[ -z "$selected" ]]; then
        ui_info "No package selected."
        ui_pause
        return
    fi

    # Extract package name (remove version info)
    local pkg_name
    pkg_name=$(echo "$selected" | awk '{print $1}')

    echo ""
    ui_info "Selected: $pkg_name"
    echo ""

    # Show package info
    pacman -Qi "$pkg_name" 2>/dev/null | grep -E '^(Name|Version|Description|Installed Size|Depends On)' | while IFS=: read -r key val; do
        printf "  ${CLR_CYAN}%-18s${CLR_RESET}%s\n" "$key:" "$val"
    done
    echo ""

    # Choose removal strategy
    ui_info "Select removal method:"
    echo ""

    local method
    method=$(ui_choose \
        "📦 Package only (pacman -R)" \
        "🔗 Package + unused dependencies (pacman -Rs)" \
        "🧹 Complete removal + configs (pacman -Rns)" \
        "🔙 Cancel"
    )

    case "$method" in
        *"Package only"*)
            if ui_confirm "Remove $pkg_name?"; then
                log_action "REMOVE: Removing $pkg_name (package only)"
                sudo pacman -R "$pkg_name"
                _remove_result "$pkg_name"
            fi
            ;;
        *"unused dependencies"*)
            if ui_confirm "Remove $pkg_name and unused dependencies?"; then
                log_action "REMOVE: Removing $pkg_name with unused deps"
                sudo pacman -Rs "$pkg_name"
                _remove_result "$pkg_name"
            fi
            ;;
        *"Complete removal"*)
            if ui_confirm "Completely remove $pkg_name (including configs)?"; then
                log_action "REMOVE: Completely removing $pkg_name"
                sudo pacman -Rns "$pkg_name"
                _remove_result "$pkg_name"
            fi
            ;;
        *"Cancel"*|"")
            ui_info "Removal cancelled."
            ;;
    esac

    ui_pause
}

_remove_result() {
    local pkg_name="$1"
    if ! pacman -Q "$pkg_name" &>/dev/null; then
        ui_success "$pkg_name removed successfully!"
        log_action "REMOVE: $pkg_name removed successfully"
    else
        ui_error "Removal may have failed. $pkg_name is still installed."
        log_action "REMOVE: $pkg_name removal may have failed"
    fi
}
