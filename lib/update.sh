#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/update.sh — System update for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

update_menu() {
    ui_clear
    ui_header "⬆  System Update"

    # Show current update status
    ui_info "Checking for updates..."
    echo ""

    local update_count=0
    if command -v checkupdates &>/dev/null; then
        update_count=$(checkupdates 2>/dev/null | wc -l || true)
    else
        ui_dim "checkupdates not available (install pacman-contrib to see pending updates)."
        echo ""
    fi
    if [[ "$update_count" -gt 0 ]]; then
        ui_warn "$update_count package update(s) available."
        echo ""
        checkupdates 2>/dev/null | head -20 | while read -r line; do
            echo -e "  ${CLR_DIM}$line${CLR_RESET}"
        done
        if [[ "$update_count" -gt 20 ]]; then
            ui_dim "  ... and $((update_count - 20)) more"
        fi
    else
        ui_success "System is up to date!"
    fi
    echo ""

    local choice
    choice=$(ui_choose \
        "🔄 Refresh databases only (pacman -Sy)" \
        "🔄 Force refresh databases (pacman -Syy)" \
        "⬆  Full system upgrade — pacman (pacman -Syu)" \
        "🌟 Full upgrade — AUR + official (yay/paru -Syu)" \
        "🔙 Back to Main Menu"
    )

    case "$choice" in
        *"Refresh databases only"*)
            log_action "UPDATE: Refreshing package databases"
            ui_info "Refreshing package databases..."
            if sudo pacman -Sy; then
                ui_success "Package databases refreshed!"
                log_action "UPDATE: Database refresh successful"
            else
                ui_error "Database refresh failed."
                log_action "UPDATE: Database refresh failed"
            fi
            ;;
        *"Force refresh databases"*)
            log_action "UPDATE: Force refreshing package databases"
            ui_info "Force refreshing package databases..."
            if sudo pacman -Syy; then
                ui_success "Package databases force refreshed!"
                log_action "UPDATE: Force database refresh successful"
            else
                ui_error "Force database refresh failed."
                log_action "UPDATE: Force database refresh failed"
            fi
            ;;
        *"pacman"*)
            log_action "UPDATE: Full system upgrade via pacman"
            if [[ "${SETTINGS[CONFIRM_ACTIONS]}" == "true" ]]; then
                if ! ui_confirm "Perform full system upgrade?"; then
                    ui_info "Update cancelled."
                    ui_pause
                    return
                fi
            fi
            if sudo pacman -Syu; then
                ui_success "System upgrade complete!"
                log_action "UPDATE: System upgrade successful"
            else
                ui_error "System upgrade failed."
                log_action "UPDATE: System upgrade failed"
            fi
            ;;
        *"AUR"*)
            local aur_helper
            if aur_helper=$(get_aur_helper); then
                log_action "UPDATE: Full upgrade via $aur_helper"
                if [[ "${SETTINGS[CONFIRM_ACTIONS]}" == "true" ]]; then
                    if ! ui_confirm "Perform full upgrade (official + AUR) via $aur_helper?"; then
                        ui_info "Update cancelled."
                        ui_pause
                        return
                    fi
                fi
                if $aur_helper -Syu; then
                    ui_success "Full system upgrade complete!"
                    log_action "UPDATE: Full upgrade via $aur_helper successful"
                else
                    ui_error "Upgrade failed."
                    log_action "UPDATE: Full upgrade via $aur_helper failed"
                fi
            else
                ui_error "No AUR helper found. Install yay or paru first."
                ui_info "You can install one via Settings > Change AUR Helper."
            fi
            ;;
        *"Back"*|"")
            return
            ;;
    esac

    ui_pause
}
