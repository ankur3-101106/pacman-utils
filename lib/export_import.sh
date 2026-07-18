#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/export_import.sh — Package list export/import
# ──────────────────────────────────────────────────────────────────────

export_import_menu() {
    while true; do
        ui_clear
        ui_header "📤  Export / Import Package Lists"

        local choice
        choice=$(ui_choose \
            "📤 Export explicit packages" \
            "📤 Export AUR packages" \
            "📤 Export ALL packages" \
            "📥 Import package list" \
            "🔙 Back to Main Menu"
        )

        case "$choice" in
            *"Export explicit"*)
                _export_explicit
                ;;
            *"Export AUR"*)
                _export_aur
                ;;
            *"Export ALL"*)
                _export_all
                ;;
            *"Import"*)
                _import_packages
                ;;
            *"Back"*|"")
                return
                ;;
        esac
    done
}

_export_explicit() {
    local default_path="${HOME}/pkglist-explicit.txt"
    local path
    path=$(ui_input "$default_path" "Export path (leave empty for default)")
    path="${path:-$default_path}"

    local count
    count=$(pacman -Qqen | wc -l)

    pacman -Qqen > "$path"
    if [[ $? -eq 0 ]]; then
        ui_success "Exported $count explicit packages to $path"
        log_action "EXPORT: $count explicit packages to $path"
    else
        ui_error "Export failed."
    fi
    ui_pause
}

_export_aur() {
    local default_path="${HOME}/pkglist-aur.txt"
    local path
    path=$(ui_input "$default_path" "Export path (leave empty for default)")
    path="${path:-$default_path}"

    local count
    count=$(pacman -Qqem | wc -l)

    if [[ "$count" -eq 0 ]]; then
        ui_warn "No AUR packages found."
        ui_pause
        return
    fi

    pacman -Qqem > "$path"
    if [[ $? -eq 0 ]]; then
        ui_success "Exported $count AUR packages to $path"
        log_action "EXPORT: $count AUR packages to $path"
    else
        ui_error "Export failed."
    fi
    ui_pause
}

_export_all() {
    local dir="${HOME}"
    local explicit_path="${dir}/pkglist-explicit.txt"
    local aur_path="${dir}/pkglist-aur.txt"

    local explicit_count aur_count

    pacman -Qqen > "$explicit_path"
    explicit_count=$(wc -l < "$explicit_path")

    pacman -Qqem > "$aur_path"
    aur_count=$(wc -l < "$aur_path")

    ui_success "Exported:"
    echo ""
    ui_table \
        "Official" "$explicit_count packages → $explicit_path" \
        "AUR"      "$aur_count packages → $aur_path"

    log_action "EXPORT: $explicit_count official + $aur_count AUR packages"
    ui_pause
}

_import_packages() {
    local path
    path=$(ui_input "${HOME}/pkglist-explicit.txt" "Path to package list file")

    if [[ -z "$path" ]]; then
        path="${HOME}/pkglist-explicit.txt"
    fi

    if [[ ! -f "$path" ]]; then
        ui_error "File not found: $path"
        ui_pause
        return
    fi

    local count
    count=$(wc -l < "$path")
    ui_info "Found $count packages in $path"
    echo ""

    # Show first few packages
    ui_dim "Preview (first 10):"
    head -10 "$path" | while read -r pkg; do
        echo -e "  ${CLR_DIM}•${CLR_RESET} $pkg"
    done
    if [[ "$count" -gt 10 ]]; then
        ui_dim "  ... and $((count - 10)) more"
    fi
    echo ""

    local install_method
    install_method=$(ui_choose \
        "📦 Install via pacman (official repos)" \
        "🌟 Install via AUR helper ($AUR_HELPER)" \
        "🔙 Cancel"
    )

    case "$install_method" in
        *"pacman"*)
            if ui_confirm "Install $count packages from $path?"; then
                log_action "IMPORT: Installing $count packages via pacman"
                sudo pacman -S --needed - < "$path"
                ui_success "Import complete!"
                log_action "IMPORT: Import complete"
            fi
            ;;
        *"AUR"*)
            local aur_helper
            if aur_helper=$(get_aur_helper); then
                if ui_confirm "Install $count packages via $aur_helper?"; then
                    log_action "IMPORT: Installing $count packages via $aur_helper"
                    $aur_helper -S --needed - < "$path"
                    ui_success "Import complete!"
                    log_action "IMPORT: Import complete"
                fi
            else
                ui_error "No AUR helper found."
            fi
            ;;
        *)
            ui_info "Import cancelled."
            ;;
    esac

    ui_pause
}
