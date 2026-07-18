#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/packages.sh — Package utilities for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

packages_menu() {
    while true; do
        ui_clear
        ui_header "📋  Package Utilities"

        local choice
        choice=$(ui_choose \
            "📋 List installed packages" \
            "📦 List explicitly installed" \
            "🧹 Remove orphan packages" \
            "📂 List files in a package" \
            "🔎 Find which package owns a file" \
            "📖 Show package information" \
            "🕒 View installation history" \
            "📋 Transaction log viewer" \
            "🔄 Reinstall a package" \
            "🔙 Back to Main Menu"
        )

        case "$choice" in
            *"List installed"*)
                _pkg_list_installed
                ;;
            *"explicitly"*)
                _pkg_list_explicit
                ;;
            *"orphan"*)
                _pkg_remove_orphans
                ;;
            *"List files"*)
                _pkg_list_files
                ;;
            *"owns a file"*)
                _pkg_find_owner
                ;;
            *"information"*)
                _pkg_show_info
                ;;
            *"history"*)
                _pkg_installation_history
                ;;
            *"Transaction"*)
                _pkg_transaction_log
                ;;
            *"Reinstall"*)
                _pkg_reinstall
                ;;
            *"Back"*|"")
                return
                ;;
        esac
    done
}

# ── List Installed Packages ─────────────────────────────────────────
_pkg_list_installed() {
    ui_header "Installed Packages"
    local total
    total=$(pacman -Q | wc -l)
    ui_info "Total: $total packages"
    echo ""

    local selected
    selected=$(pacman -Q | awk '{print $1 " " $2}' | ui_filter "Search installed packages...")

    if [[ -n "$selected" ]]; then
        local pkg_name
        pkg_name=$(echo "$selected" | awk '{print $1}')
        _pkg_show_info_for "$pkg_name"
    fi
}

# ── List Explicitly Installed ────────────────────────────────────────
_pkg_list_explicit() {
    ui_header "Explicitly Installed Packages"
    local total
    total=$(pacman -Qe | wc -l)
    ui_info "Total: $total packages"
    echo ""

    local selected
    selected=$(pacman -Qe | awk '{print $1 " " $2}' | ui_filter "Search explicit packages...")

    if [[ -n "$selected" ]]; then
        local pkg_name
        pkg_name=$(echo "$selected" | awk '{print $1}')
        _pkg_show_info_for "$pkg_name"
    fi
}

# ── Remove Orphan Packages ──────────────────────────────────────────
_pkg_remove_orphans() {
    ui_header "Orphan Packages"

    local orphans
    orphans=$(pacman -Qtdq 2>/dev/null)

    if [[ -z "$orphans" ]]; then
        ui_success "No orphan packages found!"
        ui_pause
        return
    fi

    local count
    count=$(echo "$orphans" | wc -l)
    ui_warn "$count orphan package(s) found:"
    echo ""
    echo "$orphans" | while read -r pkg; do
        echo -e "  ${CLR_DIM}•${CLR_RESET} $pkg"
    done
    echo ""

    if ui_confirm "Remove all orphan packages?"; then
        log_action "PACKAGES: Removing $count orphan packages"
        # shellcheck disable=SC2086
        sudo pacman -Rns $orphans
        if [[ $? -eq 0 ]]; then
            ui_success "Orphan packages removed!"
            log_action "PACKAGES: Orphan removal successful"
        else
            ui_error "Some packages may not have been removed."
            log_action "PACKAGES: Orphan removal had issues"
        fi
    fi

    ui_pause
}

# ── List Files in a Package ─────────────────────────────────────────
_pkg_list_files() {
    ui_header "Package Files"

    local selected
    selected=$(pacman -Q | awk '{print $1}' | ui_filter "Select a package...")

    if [[ -z "$selected" ]]; then
        return
    fi

    local pkg_name
    pkg_name=$(echo "$selected" | awk '{print $1}')

    ui_info "Files in $pkg_name:"
    echo ""

    local files
    files=$(pacman -Ql "$pkg_name" 2>/dev/null | awk '{print $2}')

    if [[ -z "$files" ]]; then
        ui_error "No files found for $pkg_name."
    else
        local file_count
        file_count=$(echo "$files" | wc -l)
        ui_dim "$file_count files"
        echo ""
        echo "$files" | ui_pager
    fi

    ui_pause
}

# ── Find Which Package Owns a File ──────────────────────────────────
_pkg_find_owner() {
    ui_header "Find Package Owner"

    local filepath
    filepath=$(ui_input "/path/to/file" "Enter file path")

    if [[ -z "$filepath" ]]; then
        ui_warn "No path entered."
        ui_pause
        return
    fi

    local result
    result=$(pacman -Qo "$filepath" 2>&1)

    if [[ $? -eq 0 ]]; then
        ui_success "$result"
    else
        ui_error "No package owns '$filepath'"
        echo ""
        ui_info "The file may belong to an AUR package or was manually created."
    fi

    ui_pause
}

# ── Show Package Information ────────────────────────────────────────
_pkg_show_info() {
    ui_header "Package Information"

    local pkg_name
    pkg_name=$(ui_input "Package name" "Enter package name")

    if [[ -z "$pkg_name" ]]; then
        ui_warn "No package name entered."
        ui_pause
        return
    fi

    _pkg_show_info_for "$pkg_name"
}

_pkg_show_info_for() {
    local pkg_name="$1"
    echo ""

    # Try local info first (installed packages)
    local info
    info=$(pacman -Qi "$pkg_name" 2>/dev/null)
    if [[ -n "$info" ]]; then
        ui_success "Installed package:"
        echo ""
        echo "$info" | while IFS=: read -r key val; do
            if [[ -n "$key" ]]; then
                printf "  ${CLR_CYAN}%-20s${CLR_RESET}%s\n" "$key:" "$val"
            fi
        done
        ui_pause
        return
    fi

    # Try repo info
    info=$(pacman -Si "$pkg_name" 2>/dev/null)
    if [[ -n "$info" ]]; then
        ui_info "Available in repository:"
        echo ""
        echo "$info" | while IFS=: read -r key val; do
            if [[ -n "$key" ]]; then
                printf "  ${CLR_CYAN}%-20s${CLR_RESET}%s\n" "$key:" "$val"
            fi
        done
        ui_pause
        return
    fi

    # Try AUR
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        info=$($aur_helper -Si "$pkg_name" 2>/dev/null)
        if [[ -n "$info" ]]; then
            ui_info "Available in AUR:"
            echo ""
            echo "$info" | while IFS=: read -r key val; do
                if [[ -n "$key" ]]; then
                    printf "  ${CLR_MAGENTA}%-20s${CLR_RESET}%s\n" "$key:" "$val"
                fi
            done
            ui_pause
            return
        fi
    fi

    ui_error "Package '$pkg_name' not found."
    ui_pause
}

# ── Installation History ────────────────────────────────────────────
_pkg_installation_history() {
    ui_header "Installation History (Recent)"

    local history
    history=$(grep -E '\[ALPM\] (installed|upgraded|removed)' /var/log/pacman.log 2>/dev/null | tail -50)

    if [[ -z "$history" ]]; then
        ui_warn "No installation history found."
        ui_pause
        return
    fi

    echo "$history" | while IFS= read -r line; do
        if echo "$line" | grep -q 'installed'; then
            echo -e "  ${CLR_GREEN}+${CLR_RESET} $line"
        elif echo "$line" | grep -q 'upgraded'; then
            echo -e "  ${CLR_BLUE}↑${CLR_RESET} $line"
        elif echo "$line" | grep -q 'removed'; then
            echo -e "  ${CLR_RED}-${CLR_RESET} $line"
        else
            echo "  $line"
        fi
    done | ui_pager

    ui_pause
}

# ── Transaction Log Viewer ──────────────────────────────────────────
_pkg_transaction_log() {
    ui_header "Transaction Log"

    local log_file="/var/log/pacman.log"

    if [[ ! -f "$log_file" ]]; then
        ui_error "Pacman log not found at $log_file"
        ui_pause
        return
    fi

    local log_size
    log_size=$(du -h "$log_file" 2>/dev/null | awk '{print $1}')
    ui_info "Log file: $log_file ($log_size)"
    echo ""

    local choice
    choice=$(ui_choose \
        "📋 Full log (last 200 lines)" \
        "📦 Installations only" \
        "⬆  Upgrades only" \
        "🗑  Removals only" \
        "⚠  Warnings & errors" \
        "🔙 Back"
    )

    case "$choice" in
        *"Full log"*)
            tail -200 "$log_file" | ui_pager
            ;;
        *"Installations"*)
            grep '\[ALPM\] installed' "$log_file" | tail -100 | ui_pager
            ;;
        *"Upgrades"*)
            grep '\[ALPM\] upgraded' "$log_file" | tail -100 | ui_pager
            ;;
        *"Removals"*)
            grep '\[ALPM\] removed' "$log_file" | tail -100 | ui_pager
            ;;
        *"Warnings"*)
            grep -E '\[ALPM\] (warning|error)' "$log_file" | tail -100 | ui_pager
            ;;
        *"Back"*|"")
            return
            ;;
    esac

    ui_pause
}

# ── Reinstall Package ───────────────────────────────────────────────
_pkg_reinstall() {
    ui_header "Reinstall Package"

    local selected
    selected=$(pacman -Q | awk '{print $1}' | ui_filter "Select a package to reinstall...")

    if [[ -z "$selected" ]]; then
        return
    fi

    local pkg_name
    pkg_name=$(echo "$selected" | awk '{print $1}')

    ui_info "This will reinstall $pkg_name, overwriting any modified files."

    if ui_confirm "Reinstall $pkg_name?"; then
        log_action "PACKAGES: Reinstalling $pkg_name"
        sudo pacman -S --overwrite '*' "$pkg_name"
        if [[ $? -eq 0 ]]; then
            ui_success "$pkg_name reinstalled!"
            log_action "PACKAGES: $pkg_name reinstalled successfully"
        else
            ui_error "Reinstallation failed."
            log_action "PACKAGES: $pkg_name reinstall failed"
        fi
    fi

    ui_pause
}
