#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/settings.sh — Settings management for Arch System Manager
# ──────────────────────────────────────────────────────────────────────

readonly CONFIG_DIR="${HOME}/.config/archman"
readonly CONFIG_FILE="${CONFIG_DIR}/settings.conf"
readonly LOG_FILE="${CONFIG_DIR}/archman.log"
readonly FAVORITES_FILE="${CONFIG_DIR}/favorites.txt"

# ── Default Settings ────────────────────────────────────────────────
declare -A SETTINGS=(
    [AUR_HELPER]="yay"
    [DRY_RUN]="false"
    [CONFIRM_ACTIONS]="true"
    [LOG_ENABLED]="true"
    [PACCACHE_KEEP]="3"
)

# ── Initialize Config ───────────────────────────────────────────────
settings_init() {
    mkdir -p "$CONFIG_DIR"

    # Create default config if missing
    if [[ ! -f "$CONFIG_FILE" ]]; then
        settings_save
    fi

    settings_load

    # Create log file if logging is enabled
    if [[ "${SETTINGS[LOG_ENABLED]}" == "true" ]]; then
        touch "$LOG_FILE"
    fi

    # Create favorites file if missing
    [[ -f "$FAVORITES_FILE" ]] || touch "$FAVORITES_FILE"

    # Export AUR_HELPER for use by other modules
    AUR_HELPER="${SETTINGS[AUR_HELPER]}"
}

# ── Save Settings ───────────────────────────────────────────────────
settings_save() {
    mkdir -p "$CONFIG_DIR"
    cat > "$CONFIG_FILE" <<EOF
# Arch System Manager Configuration
# Generated on $(date '+%Y-%m-%d %H:%M:%S')

# AUR helper to use (yay / paru)
AUR_HELPER=${SETTINGS[AUR_HELPER]}

# Dry-run mode — preview changes before executing (true / false)
DRY_RUN=${SETTINGS[DRY_RUN]}

# Confirm before destructive operations (true / false)
CONFIRM_ACTIONS=${SETTINGS[CONFIRM_ACTIONS]}

# Enable activity logging (true / false)
LOG_ENABLED=${SETTINGS[LOG_ENABLED]}

# Number of package versions to keep when cleaning cache
PACCACHE_KEEP=${SETTINGS[PACCACHE_KEEP]}
EOF
}

# ── Load Settings ───────────────────────────────────────────────────
settings_load() {
    if [[ -f "$CONFIG_FILE" ]]; then
        while IFS='=' read -r key value; do
            # Skip comments and empty lines
            [[ "$key" =~ ^[[:space:]]*# ]] && continue
            [[ -z "$key" ]] && continue
            # Trim whitespace
            key=$(echo "$key" | xargs)
            value=$(echo "$value" | xargs)
            if [[ -n "$key" && -n "$value" ]]; then
                SETTINGS[$key]="$value"
            fi
        done < "$CONFIG_FILE"
    fi

    # Export for modules
    AUR_HELPER="${SETTINGS[AUR_HELPER]}"
}

# ── Log Activity ────────────────────────────────────────────────────
log_action() {
    if [[ "${SETTINGS[LOG_ENABLED]}" == "true" ]]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE"
    fi
}

# ── Settings Menu ───────────────────────────────────────────────────
settings_menu() {
    while true; do
        ui_clear
        ui_header "⚙  Settings"

        echo ""
        ui_table \
            "AUR Helper"       "${SETTINGS[AUR_HELPER]}" \
            "Dry-Run Mode"     "${SETTINGS[DRY_RUN]}" \
            "Confirm Actions"  "${SETTINGS[CONFIRM_ACTIONS]}" \
            "Logging"          "${SETTINGS[LOG_ENABLED]}" \
            "Cache Keep"       "${SETTINGS[PACCACHE_KEEP]} versions" \
            "Config File"      "$CONFIG_FILE" \
            "Log File"         "$LOG_FILE"
        echo ""

        local choice
        choice=$(ui_choose \
            "🔧 Change AUR Helper" \
            "🧪 Toggle Dry-Run Mode" \
            "✅ Toggle Confirm Actions" \
            "📝 Toggle Logging" \
            "📦 Set Cache Keep Count" \
            "🔍 Check Dependencies" \
            "🔄 Reset to Defaults" \
            "🔙 Back to Main Menu"
        )

        case "$choice" in
            *"AUR Helper"*)
                settings_change_aur_helper
                ;;
            *"Dry-Run"*)
                settings_toggle "DRY_RUN"
                ;;
            *"Confirm"*)
                settings_toggle "CONFIRM_ACTIONS"
                ;;
            *"Logging"*)
                settings_toggle "LOG_ENABLED"
                ;;
            *"Cache Keep"*)
                settings_change_cache_keep
                ;;
            *"Dependencies"*)
                check_deps
                ui_pause
                ;;
            *"Reset"*)
                if ui_confirm "Reset all settings to defaults?"; then
                    SETTINGS=(
                        [AUR_HELPER]="yay"
                        [DRY_RUN]="false"
                        [CONFIRM_ACTIONS]="true"
                        [LOG_ENABLED]="true"
                        [PACCACHE_KEEP]="3"
                    )
                    settings_save
                    AUR_HELPER="${SETTINGS[AUR_HELPER]}"
                    ui_success "Settings reset to defaults."
                    sleep 1
                fi
                ;;
            *"Back"*|"")
                return
                ;;
        esac
    done
}

# ── Toggle a boolean setting ────────────────────────────────────────
settings_toggle() {
    local key="$1"
    if [[ "${SETTINGS[$key]}" == "true" ]]; then
        SETTINGS[$key]="false"
        ui_info "$key disabled"
    else
        SETTINGS[$key]="true"
        ui_info "$key enabled"
    fi
    settings_save
    sleep 0.5
}

# ── Change AUR Helper ───────────────────────────────────────────────
settings_change_aur_helper() {
    local choice
    choice=$(ui_choose "yay" "paru")
    if [[ -n "$choice" ]]; then
        if command -v "$choice" &>/dev/null; then
            SETTINGS[AUR_HELPER]="$choice"
            AUR_HELPER="$choice"
            settings_save
            ui_success "AUR helper set to $choice"
        else
            ui_error "$choice is not installed."
            if ui_confirm "Install $choice?"; then
                if [[ "$choice" == "yay" ]]; then
                    sudo pacman -S --needed git base-devel --noconfirm
                    local tmpdir
                    tmpdir=$(mktemp -d)
                    git clone https://aur.archlinux.org/yay.git "$tmpdir/yay"
                    (cd "$tmpdir/yay" && makepkg -si --noconfirm)
                    rm -rf "$tmpdir"
                else
                    sudo pacman -S --needed git base-devel --noconfirm
                    local tmpdir
                    tmpdir=$(mktemp -d)
                    git clone https://aur.archlinux.org/paru.git "$tmpdir/paru"
                    (cd "$tmpdir/paru" && makepkg -si --noconfirm)
                    rm -rf "$tmpdir"
                fi
                if command -v "$choice" &>/dev/null; then
                    SETTINGS[AUR_HELPER]="$choice"
                    AUR_HELPER="$choice"
                    settings_save
                    ui_success "Installed and set AUR helper to $choice"
                fi
            fi
        fi
    fi
    sleep 0.5
}

# ── Change Cache Keep Count ──────────────────────────────────────────
settings_change_cache_keep() {
    local count
    count=$(ui_input "Number of versions to keep (1-10)" "Cache Keep Count")
    if [[ "$count" =~ ^[0-9]+$ ]] && (( count >= 1 && count <= 10 )); then
        SETTINGS[PACCACHE_KEEP]="$count"
        settings_save
        ui_success "Will keep $count package versions when cleaning cache."
    else
        ui_error "Invalid number. Please enter 1-10."
    fi
    sleep 0.5
}
