#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# lib/ui.sh — Shared UI library for Arch System Manager
#
# Provides styled output, prompts, and gum wrappers with plain-text
# fallbacks so the script works (less pretty) even without gum.
# ──────────────────────────────────────────────────────────────────────

# ── Color Constants ──────────────────────────────────────────────────
readonly CLR_RESET='\033[0m'
readonly CLR_BOLD='\033[1m'
readonly CLR_DIM='\033[2m'
readonly CLR_RED='\033[0;31m'
readonly CLR_GREEN='\033[0;32m'
readonly CLR_YELLOW='\033[0;33m'
readonly CLR_BLUE='\033[0;34m'
readonly CLR_MAGENTA='\033[0;35m'
readonly CLR_CYAN='\033[0;36m'
readonly CLR_WHITE='\033[1;37m'
readonly CLR_BG_BLUE='\033[44m'

# ── Capability Detection ────────────────────────────────────────────
HAS_GUM=false
HAS_FZF=false
HAS_REFLECTOR=false
HAS_PACCACHE=false

detect_capabilities() {
    command -v gum      &>/dev/null && HAS_GUM=true
    command -v fzf      &>/dev/null && HAS_FZF=true
    command -v reflector &>/dev/null && HAS_REFLECTOR=true
    command -v paccache  &>/dev/null && HAS_PACCACHE=true
}

# ── ASCII Art Banner ────────────────────────────────────────────────
ui_banner() {
    local banner
    banner=$(cat <<'EOF'

     █████╗ ██████╗  ██████╗██╗  ██╗
    ██╔══██╗██╔══██╗██╔════╝██║  ██║
    ███████║██████╔╝██║     ███████║
    ██╔══██║██╔══██╗██║     ██╔══██║
    ██║  ██║██║  ██║╚██████╗██║  ██║
    ╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝

       Interactive Arch Manager

EOF
)

    if $HAS_GUM; then
        gum style \
            --border double \
            --border-foreground 33 \
            --foreground 33 \
            --bold \
            --align center \
            --padding "0 4" \
            --margin "1 2" \
            "$banner"
    else
        echo -e "${CLR_CYAN}╔══════════════════════════════════════════════╗${CLR_RESET}"
        while IFS= read -r line; do
            printf "${CLR_CYAN}║${CLR_BLUE} %-44s ${CLR_CYAN}║${CLR_RESET}\n" "$line"
        done <<< "$banner"
        echo -e "${CLR_CYAN}╚══════════════════════════════════════════════╝${CLR_RESET}"
    fi
}

# ── Section Headers ─────────────────────────────────────────────────
ui_header() {
    local title="$1"
    echo ""
    if $HAS_GUM; then
        gum style \
            --foreground 33 \
            --bold \
            --border normal \
            --border-foreground 240 \
            --padding "0 2" \
            --margin "0 1" \
            "$title"
    else
        echo -e "${CLR_BOLD}${CLR_CYAN}── $title ──${CLR_RESET}"
    fi
    echo ""
}

# ── Status Messages ─────────────────────────────────────────────────
ui_success() {
    if $HAS_GUM; then
        gum style --foreground 2 "✔ $1"
    else
        echo -e "${CLR_GREEN}✔ $1${CLR_RESET}"
    fi
}

ui_warn() {
    if $HAS_GUM; then
        gum style --foreground 3 "⚠ $1"
    else
        echo -e "${CLR_YELLOW}⚠ $1${CLR_RESET}"
    fi
}

ui_error() {
    if $HAS_GUM; then
        gum style --foreground 1 "✘ $1"
    else
        echo -e "${CLR_RED}✘ $1${CLR_RESET}"
    fi
}

ui_info() {
    if $HAS_GUM; then
        gum style --foreground 4 "ℹ $1"
    else
        echo -e "${CLR_BLUE}ℹ $1${CLR_RESET}"
    fi
}

ui_dim() {
    if $HAS_GUM; then
        gum style --foreground 240 "$1"
    else
        echo -e "${CLR_DIM}$1${CLR_RESET}"
    fi
}

# ── Interactive Prompts ─────────────────────────────────────────────

# ui_confirm "Are you sure?" → returns 0 (yes) or 1 (no)
ui_confirm() {
    local prompt="${1:-Continue?}"
    if $HAS_GUM; then
        gum confirm "$prompt"
        return $?
    else
        local reply
        echo -en "${CLR_YELLOW}$prompt ${CLR_DIM}[Y/n]${CLR_RESET} "
        read -r reply
        [[ -z "$reply" || "$reply" =~ ^[Yy] ]]
        return $?
    fi
}

# ui_choose "Option A" "Option B" "Option C" → prints selected option
# Never propagates failure: an aborted chooser (ESC/q) yields an empty result,
# letting callers treat it via their *"Back"*|"" arms instead of crashing.
ui_choose() {
    local selection=""
    if $HAS_GUM; then
        selection=$(gum choose --height=20 --cursor.foreground 33 --selected.foreground 33 "$@") || true
    else
        local i=1
        local options=("$@")
        for opt in "${options[@]}"; do
            echo -e "  ${CLR_CYAN}$i)${CLR_RESET} $opt" >&2
            ((i++))
        done
        echo "" >&2
        local reply
        echo -en "${CLR_BOLD}Select: ${CLR_RESET}" >&2
        read -r reply
        if [[ "$reply" =~ ^[0-9]+$ ]] && (( reply >= 1 && reply <= ${#options[@]} )); then
            selection="${options[$((reply - 1))]}"
        fi
    fi
    printf '%s\n' "$selection"
}

# ui_choose_multi "Option A" "Option B" → prints selected options (one per line)
ui_choose_multi() {
    if $HAS_GUM; then
        gum choose --no-limit --cursor.foreground 33 --selected.foreground 33 "$@" || true
    else
        local i=1
        local options=("$@")
        for opt in "${options[@]}"; do
            echo -e "  ${CLR_CYAN}$i)${CLR_RESET} $opt" >&2
            ((i++))
        done
        echo "" >&2
        echo -en "${CLR_BOLD}Select (comma-separated, e.g. 1,3,5): ${CLR_RESET}" >&2
        local selection
        read -r selection
        IFS=',' read -ra indices <<< "$selection"
        for idx in "${indices[@]}"; do
            idx=$(echo "$idx" | tr -d ' ')
            if [[ "$idx" =~ ^[0-9]+$ ]] && (( idx >= 1 && idx <= ${#options[@]} )); then
                echo "${options[$((idx - 1))]}"
            fi
        done
    fi
}

# ui_input "placeholder text" → prints user input
# An aborted prompt (ESC) returns empty output.
ui_input() {
    local placeholder="${1:-Enter text...}"
    local header="${2:-}"
    local value=""
    if $HAS_GUM; then
        value=$(gum input --placeholder "$placeholder" --cursor.foreground 33 ${header:+--header "$header"}) || true
    else
        [[ -n "$header" ]] && echo -e "${CLR_BOLD}$header${CLR_RESET}" >&2
        echo -en "${CLR_DIM}($placeholder)${CLR_RESET} > " >&2
        read -r value
    fi
    printf '%s\n' "$value"
}

# ui_filter — pipe a list into this for fuzzy filtering
# Usage: echo -e "item1\nitem2" | ui_filter "Search..."
# Never propagates failure: an aborted filter (ESC) yields an empty result.
ui_filter() {
    local placeholder="${1:-Filter...}"
    local selection=""
    if $HAS_FZF; then
        selection=$(fzf --prompt="$placeholder > " --height=20 --reverse) || true
    elif $HAS_GUM; then
        selection=$(gum filter --placeholder "$placeholder" --indicator.foreground 33) || true
    else
        # Plain fallback: show list on stderr, ask user to type
        local items=()
        while IFS= read -r line; do
            items+=("$line")
        done
        local i=1
        for item in "${items[@]}"; do
            echo -e "  ${CLR_CYAN}$i)${CLR_RESET} $item" >&2
            ((i++))
        done
        echo "" >&2
        echo -en "${CLR_BOLD}$placeholder > ${CLR_RESET}" >&2
        local reply
        read -r reply
        if [[ "$reply" =~ ^[0-9]+$ ]] && (( reply >= 1 && reply <= ${#items[@]} )); then
            selection="${items[$((reply - 1))]}"
        elif [[ -n "$reply" ]]; then
            selection=$(printf '%s\n' "${items[@]}" | grep -i -- "$reply" | head -1) || true
        fi
    fi
    printf '%s\n' "$selection"
}

# ui_spin "Loading..." command arg1 arg2 ...
ui_spin() {
    local title="$1"
    shift
    if $HAS_GUM; then
        gum spin --spinner dot --title "$title" -- "$@"
    else
        echo -en "${CLR_DIM}$title${CLR_RESET} "
        "$@" &
        local pid=$!
        local spinchars='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
        local i=0
        while kill -0 "$pid" 2>/dev/null; do
            printf "\r${CLR_DIM}$title ${spinchars:$i:1}${CLR_RESET} "
            i=$(( (i + 1) % ${#spinchars} ))
            sleep 0.1
        done
        wait "$pid"
        local ret=$?
        printf "\r%-$((${#title} + 4))s\r" " "
        return $ret
    fi
}

# ui_pager — display content in a scrollable pager
# Usage: echo "content" | ui_pager
ui_pager() {
    if $HAS_GUM; then
        gum pager
    else
        less -R
    fi
}

# ui_pause — wait for keypress
ui_pause() {
    echo ""
    local val=""
    if $HAS_GUM; then
        val=$(gum input --placeholder "Press Enter for main menu, 'q' to quit..." --cursor.foreground 240) || true
    else
        echo -en "${CLR_DIM}Press Enter for main menu, 'q' to quit...${CLR_RESET}"
        read -r val
    fi
    if [[ "${val,,}" == "q" ]]; then
        ui_clear
        if $HAS_GUM; then
            gum style --foreground 33 --bold --padding "1 2" "Thanks for using archman! 👋"
        else
            echo -e "${CLR_CYAN}${CLR_BOLD}Thanks for using archman! 👋${CLR_RESET}"
        fi
        exit 0
    fi
}

# ui_table — display key-value pairs as a formatted table
# Usage: ui_table "Key1" "Value1" "Key2" "Value2" ...
ui_table() {
    local args=("$@")
    local max_key_len=0
    local i

    # Find max key length for alignment
    for (( i=0; i<${#args[@]}; i+=2 )); do
        local key="${args[$i]}"
        (( ${#key} > max_key_len )) && max_key_len=${#key}
    done

    for (( i=0; i<${#args[@]}; i+=2 )); do
        local key="${args[$i]}"
        local val="${args[$((i+1))]}"
        if $HAS_GUM; then
            printf "  $(gum style --foreground 33 --bold "%-${max_key_len}s") │ %s\n" "$key" "$val"
        else
            printf "  ${CLR_CYAN}${CLR_BOLD}%-${max_key_len}s${CLR_RESET} │ %s\n" "$key" "$val"
        fi
    done
}

# ── Dependency Checker ──────────────────────────────────────────────
check_deps() {
    ui_header "Dependency Check"

    local all_good=true

    if $HAS_GUM; then
        ui_success "gum — interactive TUI"
    else
        ui_warn "gum — not installed (using plain-text fallbacks)"
        all_good=false
    fi

    if $HAS_FZF; then
        ui_success "fzf — fuzzy finder"
    else
        ui_info "fzf — not installed (optional)"
    fi

    if $HAS_REFLECTOR; then
        ui_success "reflector — mirror management"
    else
        ui_warn "reflector — not installed (needed for mirror management)"
    fi

    if $HAS_PACCACHE; then
        ui_success "paccache — cache cleaning (pacman-contrib)"
    else
        ui_warn "paccache — not installed (needed for cache management)"
    fi

    # Check for AUR helper
    if command -v yay &>/dev/null; then
        ui_success "yay — AUR helper"
    elif command -v paru &>/dev/null; then
        ui_success "paru — AUR helper"
    else
        ui_warn "No AUR helper found (yay/paru)"
    fi

    echo ""
    if ! $all_good; then
        ui_info "Install missing dependencies for the best experience."
        if ui_confirm "Install gum now?"; then
            sudo pacman -S gum --noconfirm && detect_capabilities
        fi
    fi
}

# ── AUR Helper Detection ────────────────────────────────────────────
# Returns the AUR helper command, respecting settings
get_aur_helper() {
    # Check settings first
    if [[ -n "${AUR_HELPER:-}" ]]; then
        if command -v "$AUR_HELPER" &>/dev/null; then
            echo "$AUR_HELPER"
            return 0
        fi
    fi
    # Auto-detect
    if command -v yay &>/dev/null; then
        echo "yay"
        return 0
    elif command -v paru &>/dev/null; then
        echo "paru"
        return 0
    fi
    return 1
}

# ── Clear Screen Helper ─────────────────────────────────────────────
ui_clear() {
    clear
}
