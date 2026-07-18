#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/lockfile.sh — Pacman lock file management
# ──────────────────────────────────────────────────────────────────────

readonly LOCK_FILE="/var/lib/pacman/db.lck"

lockfile_menu() {
    ui_clear
    ui_header "🔒  Pacman Lock File"

    echo ""
    if [[ -f "$LOCK_FILE" ]]; then
        ui_warn "Lock file exists!"
        echo ""
        ui_table \
            "File"     "$LOCK_FILE" \
            "Created"  "$(stat -c '%y' "$LOCK_FILE" 2>/dev/null | cut -d. -f1)" \
            "Size"     "$(stat -c '%s' "$LOCK_FILE" 2>/dev/null) bytes"
        echo ""

        ui_info "This file prevents multiple pacman instances from running."
        ui_info "Only remove it if you're sure no other pacman process is active."
        echo ""

        # Check if pacman is actually running
        if pgrep -x pacman &>/dev/null; then
            ui_error "WARNING: pacman appears to be running!"
            ui_error "PID(s): $(pgrep -x pacman | tr '\n' ' ')"
            echo ""
            ui_error "Removing the lock file while pacman is running can corrupt your database."
            echo ""
        fi

        if ui_confirm "Remove lock file?"; then
            log_action "LOCKFILE: Removing $LOCK_FILE"
            sudo rm -f "$LOCK_FILE"
            if [[ ! -f "$LOCK_FILE" ]]; then
                ui_success "Lock file removed successfully."
                log_action "LOCKFILE: Lock file removed"
            else
                ui_error "Failed to remove lock file."
                log_action "LOCKFILE: Failed to remove lock file"
            fi
        else
            ui_info "Lock file kept."
        fi
    else
        ui_success "No lock file found — pacman is free to run."
    fi

    ui_pause
}
