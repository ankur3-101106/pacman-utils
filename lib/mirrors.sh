#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/mirrors.sh — Mirror management for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

readonly MIRRORLIST="/etc/pacman.d/mirrorlist"
readonly MIRRORLIST_BACKUP="/etc/pacman.d/mirrorlist.bak"

mirrors_menu() {
    while true; do
        ui_clear
        ui_header "🌍  Mirror Management"

        # Show current mirror stats
        local mirror_count
        mirror_count=$(grep -c '^Server' "$MIRRORLIST" 2>/dev/null)
        ui_table \
            "Mirrorlist"   "$MIRRORLIST" \
            "Active Mirrors" "${mirror_count:-0}" \
            "Reflector"    "$($HAS_REFLECTOR && echo 'installed' || echo 'not installed')"
        echo ""

        local choice
        choice=$(ui_choose \
            "🔄 Auto-update mirrors (reflector)" \
            "🏎  Rank fastest mirrors" \
            "📋 Show current mirrors" \
            "💾 Backup current mirrorlist" \
            "♻  Restore mirrorlist from backup" \
            "🔙 Back to Main Menu"
        )

        case "$choice" in
            *"Auto-update"*)
                _mirrors_reflector_auto
                ;;
            *"Rank fastest"*)
                _mirrors_rank_fastest
                ;;
            *"Show current"*)
                _mirrors_show_current
                ;;
            *"Backup"*)
                _mirrors_backup
                ;;
            *"Restore"*)
                _mirrors_restore
                ;;
            *"Back"*|"")
                return
                ;;
        esac
    done
}

_mirrors_ensure_reflector() {
    if ! $HAS_REFLECTOR; then
        ui_warn "reflector is not installed."
        if ui_confirm "Install reflector?"; then
            sudo pacman -S reflector --noconfirm
            detect_capabilities
            if ! $HAS_REFLECTOR; then
                ui_error "Failed to install reflector."
                return 1
            fi
        else
            return 1
        fi
    fi
    return 0
}

_mirrors_reflector_auto() {
    _mirrors_ensure_reflector || { ui_pause; return; }

    ui_info "Updating mirrorlist with reflector..."
    ui_info "Using: --country auto --latest 20 --sort rate"
    echo ""

    # Backup first
    sudo cp "$MIRRORLIST" "$MIRRORLIST_BACKUP" 2>/dev/null

    log_action "MIRRORS: Auto-updating mirrors with reflector"

    if $HAS_GUM; then
        gum spin --spinner dot --title "Fetching and ranking mirrors..." -- \
            sudo reflector \
                --country auto \
                --latest 20 \
                --sort rate \
                --save "$MIRRORLIST"
    else
        sudo reflector \
            --country auto \
            --latest 20 \
            --sort rate \
            --save "$MIRRORLIST"
    fi

    if [[ $? -eq 0 ]]; then
        local new_count
        new_count=$(grep -c '^Server' "$MIRRORLIST" 2>/dev/null)
        ui_success "Mirrorlist updated! ($new_count mirrors)"
        log_action "MIRRORS: Updated to $new_count mirrors"
    else
        ui_error "reflector failed. Restoring backup..."
        if [[ -f "$MIRRORLIST_BACKUP" ]]; then
            sudo cp "$MIRRORLIST_BACKUP" "$MIRRORLIST"
            ui_info "Backup restored."
        fi
        log_action "MIRRORS: reflector failed, backup restored"
    fi

    ui_pause
}

_mirrors_rank_fastest() {
    _mirrors_ensure_reflector || { ui_pause; return; }

    ui_info "Ranking the 10 fastest mirrors..."
    echo ""

    sudo cp "$MIRRORLIST" "$MIRRORLIST_BACKUP" 2>/dev/null

    log_action "MIRRORS: Ranking fastest mirrors"

    if $HAS_GUM; then
        gum spin --spinner dot --title "Testing mirror speeds..." -- \
            sudo reflector \
                --latest 10 \
                --sort rate \
                --save "$MIRRORLIST"
    else
        sudo reflector \
            --latest 10 \
            --sort rate \
            --save "$MIRRORLIST"
    fi

    if [[ $? -eq 0 ]]; then
        ui_success "Mirrorlist updated with fastest mirrors!"
        log_action "MIRRORS: Ranked fastest mirrors"
    else
        ui_error "Failed to rank mirrors."
        log_action "MIRRORS: Failed to rank mirrors"
    fi

    ui_pause
}

_mirrors_show_current() {
    ui_header "Current Mirrors"

    local servers
    servers=$(grep '^Server' "$MIRRORLIST" 2>/dev/null | sed 's/Server = //')

    if [[ -z "$servers" ]]; then
        ui_warn "No active mirrors found in $MIRRORLIST"
    else
        local i=1
        echo "$servers" | while read -r mirror; do
            printf "  ${CLR_CYAN}%2d.${CLR_RESET} %s\n" "$i" "$mirror"
            ((i++))
        done
    fi

    ui_pause
}

_mirrors_backup() {
    log_action "MIRRORS: Backing up mirrorlist"
    sudo cp "$MIRRORLIST" "$MIRRORLIST_BACKUP"
    if [[ $? -eq 0 ]]; then
        ui_success "Mirrorlist backed up to $MIRRORLIST_BACKUP"
        log_action "MIRRORS: Backup successful"
    else
        ui_error "Backup failed."
        log_action "MIRRORS: Backup failed"
    fi
    ui_pause
}

_mirrors_restore() {
    if [[ -f "$MIRRORLIST_BACKUP" ]]; then
        ui_info "Backup found: $MIRRORLIST_BACKUP"
        local backup_date
        backup_date=$(stat -c '%y' "$MIRRORLIST_BACKUP" 2>/dev/null | cut -d. -f1)
        ui_info "Created: $backup_date"

        if ui_confirm "Restore mirrorlist from backup?"; then
            log_action "MIRRORS: Restoring mirrorlist from backup"
            sudo cp "$MIRRORLIST_BACKUP" "$MIRRORLIST"
            ui_success "Mirrorlist restored!"
            log_action "MIRRORS: Mirrorlist restored"
        fi
    else
        ui_error "No backup found at $MIRRORLIST_BACKUP"
    fi
    ui_pause
}
