#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/cache.sh — Package cache management for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

cache_menu() {
    ui_clear
    ui_header "🧹  Package Cache Management"

    # Show current cache info
    local cache_size cache_count
    cache_size=$(du -sh /var/cache/pacman/pkg/ 2>/dev/null | awk '{print $1}')
    cache_count=$(find /var/cache/pacman/pkg/ -name '*.pkg.tar.*' 2>/dev/null | wc -l)

    ui_table \
        "Cache Location" "/var/cache/pacman/pkg/" \
        "Cache Size"     "${cache_size:-unknown}" \
        "Cached Files"   "${cache_count:-unknown}"
    echo ""

    local choice
    choice=$(ui_choose \
        "📦 Keep last ${SETTINGS[PACCACHE_KEEP]} versions (paccache -rk${SETTINGS[PACCACHE_KEEP]})" \
        "🗑  Remove uninstalled packages (pacman -Sc)" \
        "💣 Remove ALL cache (pacman -Scc)" \
        "🔙 Back to Main Menu"
    )

    case "$choice" in
        *"Keep last"*)
            if ! $HAS_PACCACHE; then
                ui_error "paccache not found. Install pacman-contrib first."
                if ui_confirm "Install pacman-contrib?"; then
                    sudo pacman -S pacman-contrib --noconfirm
                    detect_capabilities
                fi
                ui_pause
                return
            fi
            local keep="${SETTINGS[PACCACHE_KEEP]}"
            log_action "CACHE: Keeping last $keep versions"
            if ui_confirm "Remove all but the last $keep versions of each package?"; then
                sudo paccache -rk"$keep"
                _show_cache_after
            fi
            ;;
        *"uninstalled"*)
            log_action "CACHE: Removing uninstalled package cache"
            if ui_confirm "Remove all cached packages that are not currently installed?"; then
                sudo pacman -Sc
                _show_cache_after
            fi
            ;;
        *"ALL cache"*)
            ui_warn "This will remove ALL cached packages!"
            log_action "CACHE: Removing ALL cache"
            if ui_confirm "Are you absolutely sure? This cannot be undone."; then
                sudo pacman -Scc
                _show_cache_after
            fi
            ;;
        *"Back"*|"")
            return
            ;;
    esac

    ui_pause
}

_show_cache_after() {
    echo ""
    local new_size
    new_size=$(du -sh /var/cache/pacman/pkg/ 2>/dev/null | awk '{print $1}')
    ui_success "Cache cleaned! New size: ${new_size:-unknown}"
    log_action "CACHE: Cache cleaned. New size: $new_size"
}
