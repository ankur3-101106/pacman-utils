#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/info.sh — System information for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

info_menu() {
    ui_clear
    ui_header "📊  System Information"

    # Gather system info
    local hostname kernel arch pacman_ver mirror_count
    local total_pkgs explicit_pkgs orphan_pkgs cache_size disk_usage
    local uptime_info

    hostname=$(hostnamectl --static 2>/dev/null || hostname 2>/dev/null || echo "unknown")
    kernel=$(uname -r 2>/dev/null || echo "unknown")
    arch=$(uname -m 2>/dev/null || echo "unknown")
    pacman_ver=$(pacman --version 2>/dev/null | head -1 | grep -oP 'v[\d.]+' || echo "unknown")
    mirror_count=$(grep -c '^Server' /etc/pacman.d/mirrorlist 2>/dev/null || true)
    mirror_count="${mirror_count:-0}"
    total_pkgs=$(pacman -Q 2>/dev/null | wc -l || true)
    explicit_pkgs=$(pacman -Qe 2>/dev/null | wc -l || true)
    orphan_pkgs=$(pacman -Qtdq 2>/dev/null | wc -l || true)
    cache_size=$(du -sh /var/cache/pacman/pkg/ 2>/dev/null | awk '{print $1}' || echo "unknown")
    disk_usage=$(df -h / 2>/dev/null | awk 'NR==2 {printf "%s / %s (%s used)", $3, $2, $5}')
    uptime_info=$(uptime -p 2>/dev/null || echo "unknown")

    # AUR stats
    local aur_pkgs aur_helper_info
    aur_pkgs=$(pacman -Qm 2>/dev/null | wc -l || true)
    local aur_helper
    if aur_helper=$(get_aur_helper); then
        aur_helper_info="$aur_helper ($($aur_helper --version 2>/dev/null | head -1))"
    else
        aur_helper_info="not installed"
    fi

    # Display
    echo ""
    if $HAS_GUM; then
        gum style --foreground 240 "  ── System ──"
    else
        echo -e "${CLR_DIM}  ── System ──${CLR_RESET}"
    fi
    echo ""
    ui_table \
        "Hostname"      "$hostname" \
        "Kernel"        "$kernel" \
        "Architecture"  "$arch" \
        "Uptime"        "$uptime_info"
    echo ""

    if $HAS_GUM; then
        gum style --foreground 240 "  ── Pacman ──"
    else
        echo -e "${CLR_DIM}  ── Pacman ──${CLR_RESET}"
    fi
    echo ""
    ui_table \
        "Pacman Version"    "$pacman_ver" \
        "Mirror Count"      "$mirror_count" \
        "AUR Helper"        "$aur_helper_info"
    echo ""

    if $HAS_GUM; then
        gum style --foreground 240 "  ── Packages ──"
    else
        echo -e "${CLR_DIM}  ── Packages ──${CLR_RESET}"
    fi
    echo ""
    ui_table \
        "Total Packages"      "$total_pkgs" \
        "Explicit Packages"   "$explicit_pkgs" \
        "AUR Packages"        "$aur_pkgs" \
        "Orphan Packages"     "$orphan_pkgs"
    echo ""

    if $HAS_GUM; then
        gum style --foreground 240 "  ── Storage ──"
    else
        echo -e "${CLR_DIM}  ── Storage ──${CLR_RESET}"
    fi
    echo ""
    ui_table \
        "Cache Size"   "$cache_size" \
        "Disk Usage"   "$disk_usage"
    echo ""

    # Highlight orphans if any
    if [[ "$orphan_pkgs" -gt 0 ]]; then
        ui_warn "$orphan_pkgs orphan packages found. Consider cleaning them."
    fi

    ui_pause
}
