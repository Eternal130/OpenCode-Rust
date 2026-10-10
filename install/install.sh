#!/usr/bin/env bash
# OpenCode (Rust port) installer — mirrors the v2 installer's UX shape
# (install/install.sh in the reference: versioned download, ~/.opencode/bin
# destination, shell-config PATH hint, ASCII banner) with two port
# differences, both REGISTERED in tools/RELEASE.md:
#   * the tarball URL is PARAMETERIZED (no opencode.ai latest-version
#     probe: this stream must never adopt TS releases beyond 2.0.24);
#   * the download is verified against a sha256 when one is supplied
#     (--sha256, $SHA256, or a <url>.sha256 sidecar).
set -euo pipefail
APP=opencode

MUTED='\033[0;2m'
RED='\033[0;31m'
ORANGE='\033[38;5;214m'
NC='\033[0m' # No Color

usage() {
    cat <<EOF
OpenCode Installer (Rust port)

Usage: install.sh [options]

Options:
    -h, --help              Display this help message
    -v, --version <version> Install a specific version (default: $DEFAULT_VERSION)
    -u, --url <url>         Release tarball URL (default: \$RELEASE_URL or required)
        --sha256 <hash>     Verify the download against this sha256
    -b, --binary <path>     Install from a local binary or tarball instead of downloading
        --no-modify-path    Don't modify shell config files (.zshrc, .bashrc, etc.)

Examples:
    curl -fsSL <release-host>/install.sh | bash -s -- --version 2.0.24 --url <tarball-url> --sha256 <hash>
    ./install.sh --binary /path/to/opencode
    RELEASE_URL=https://example.com/opencode-2.0.24-linux-x64.tar.gz ./install.sh
EOF
}

DEFAULT_VERSION=2.0.24
requested_version="${VERSION:-$DEFAULT_VERSION}"
release_url="${RELEASE_URL:-}"
expected_sha="${SHA256:-}"
no_modify_path=false
binary_path=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        -v|--version)
            [[ -n "${2:-}" ]] || { echo -e "${RED}Error: --version requires a version argument${NC}" >&2; exit 1; }
            requested_version="$2"
            shift 2
            ;;
        -u|--url)
            [[ -n "${2:-}" ]] || { echo -e "${RED}Error: --url requires a URL argument${NC}" >&2; exit 1; }
            release_url="$2"
            shift 2
            ;;
        --sha256)
            [[ -n "${2:-}" ]] || { echo -e "${RED}Error: --sha256 requires a hash argument${NC}" >&2; exit 1; }
            expected_sha="$2"
            shift 2
            ;;
        -b|--binary)
            [[ -n "${2:-}" ]] || { echo -e "${RED}Error: --binary requires a path argument${NC}" >&2; exit 1; }
            binary_path="$2"
            shift 2
            ;;
        --no-modify-path)
            no_modify_path=true
            shift
            ;;
        *)
            echo -e "${ORANGE}Warning: Unknown option '$1'${NC}" >&2
            shift
            ;;
    esac
done

requested_version="${requested_version#v}"
INSTALL_DIR="$HOME/.opencode/bin"
mkdir -p "$INSTALL_DIR"

check_version() {
    if command -v "$APP" >/dev/null 2>&1; then
        installed_version=$("$APP" --version 2>/dev/null || echo "")
        installed_version="${installed_version##* }"
        installed_version="${installed_version#v}"
        echo -e "${MUTED}Installed version: ${NC}$installed_version."
    fi
}

verify_sha256() {
    local file="$1" expected="$2" actual
    actual=$(sha256sum "$file" | awk '{print $1}')
    if [ "$actual" != "$expected" ]; then
        echo -e "${RED}Error: checksum mismatch for $file${NC}" >&2
        echo -e "${MUTED}expected $expected${NC}got      $actual" >&2
        rm -f "$file"
        exit 1
    fi
}

extract_binary() {
    # Accepts a raw binary, a .tar.gz, or a .zip; prints the payload path.
    local file="$1" magic
    magic=$(head -c 4 "$file" | od -An -tx1 | tr -d ' \n')
    case "$magic" in
        7f454c46)
            printf '%s' "$file"
            ;;
        1f8b*)
            local dir
            dir=$(mktemp -d "${TMPDIR:-/tmp}/opencode_extract_XXXXXX")
            tar -xzf "$file" -C "$dir"
            local found
            found=$(find "$dir" -maxdepth 2 -type f -name opencode | head -n 1)
            [ -n "$found" ] || { echo -e "${RED}Error: no opencode binary inside $file${NC}" >&2; exit 1; }
            printf '%s' "$found"
            ;;
        504b0304)
            local dir
            dir=$(mktemp -d "${TMPDIR:-/tmp}/opencode_extract_XXXXXX")
            unzip -q -o "$file" -d "$dir"
            local found
            found=$(find "$dir" -type f -name opencode | head -n 1)
            [ -n "$found" ] || { echo -e "${RED}Error: no opencode binary inside $file${NC}" >&2; exit 1; }
            printf '%s' "$found"
            ;;
        *)
            echo -e "${RED}Error: $file is neither a binary nor a tar.gz/zip archive${NC}" >&2
            exit 1
            ;;
    esac
}

install_from_path() {
    local payload
    payload=$(extract_binary "$1")
    cp "$payload" "$INSTALL_DIR/$APP"
    chmod 755 "$INSTALL_DIR/$APP"
}

if [ -n "$binary_path" ]; then
    [ -f "$binary_path" ] || { echo -e "${RED}Error: Binary not found at ${binary_path}${NC}" >&2; exit 1; }
    echo -e "\n${MUTED}Installing ${NC}$APP ${MUTED}from: ${NC}$binary_path"
    install_from_path "$binary_path"
else
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || {
        echo -e "${RED}Error: curl (or wget) is required to download OpenCode.${NC}" >&2
        exit 1
    }

    if [ -z "$release_url" ]; then
        echo -e "${RED}Error: no download URL. Pass --url (or set \$RELEASE_URL) — the Rust port never guesses an upstream (the 2.0.24 pin must not adopt TS releases).${NC}" >&2
        exit 1
    fi
    # --version placeholders, for URL templates.
    arch=$(uname -m)
    [ "$arch" = "aarch64" ] && arch="arm64"
    [ "$arch" = "x86_64" ] && arch="x64"
    target="$(uname -s | tr '[:upper:]' '[:lower:]')-$arch"
    release_url="${release_url//\{version\}/$requested_version}"
    release_url="${release_url//\{target\}/$target}"

    check_version
    echo -e "\n${MUTED}Installing ${NC}$APP ${MUTED}version: ${NC}$requested_version"
    tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/opencode_install_XXXXXX")
    trap 'rm -rf "$tmp_dir"' EXIT
    archive="$tmp_dir/opencode-download"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 2 -o "$archive" "$release_url" || {
            echo -e "${RED}Error: download failed: $release_url${NC}" >&2
            exit 1
        }
    else
        wget -q --tries=2 -O "$archive" "$release_url" || {
            echo -e "${RED}Error: download failed: $release_url${NC}" >&2
            exit 1
        }
    fi

    # Checksum: --sha256 / $SHA256, else a <url>.sha256 sidecar when fetchable.
    if [ -z "$expected_sha" ] && command -v curl >/dev/null 2>&1; then
        expected_sha=$(curl -fsSL --retry 1 "$release_url.sha256" 2>/dev/null | awk '{print $1}' || true)
    fi
    if [ -n "$expected_sha" ]; then
        verify_sha256 "$archive" "$expected_sha"
        echo -e "${MUTED}Checksum verified.${NC}"
    else
        echo -e "${ORANGE}Warning: no sha256 available for $release_url — installing unverified.${NC}" >&2
    fi

    install_from_path "$archive"
fi

# ---------------------------------------------------------------------------
# PATH hint (v2 installer shape)
# ---------------------------------------------------------------------------
add_to_path() {
    local config_file=$1
    local command=$2

    if grep -Fxq "$command" "$config_file"; then
        echo -e "${MUTED}Command already exists in $config_file, skipping write.${NC}"
    elif [[ -w $config_file ]]; then
        echo -e "\n# opencode" >> "$config_file"
        echo "$command" >> "$config_file"
        echo -e "${MUTED}Successfully added ${NC}opencode ${MUTED}to \$PATH in ${NC}$config_file"
    else
        echo -e "${ORANGE}Warning: Manually add the directory to $config_file (or similar):${NC}" >&2
        echo -e "  $command"
    fi
}

XDG_CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}

current_shell=$(basename "${SHELL:-bash}")
case $current_shell in
    fish)
        config_files="$HOME/.config/fish/config.fish"
        ;;
    zsh)
        config_files="${ZDOTDIR:-$HOME}/.zshrc ${ZDOTDIR:-$HOME}/.zshenv $XDG_CONFIG_HOME/zsh/.zshrc $XDG_CONFIG_HOME/zsh/.zshenv"
        ;;
    bash)
        config_files="$HOME/.bashrc $HOME/.bash_profile $HOME/.profile $XDG_CONFIG_HOME/bash/.bashrc $XDG_CONFIG_HOME/bash/.bash_profile"
        ;;
    ash|sh)
        config_files="$HOME/.ashrc $HOME/.profile /etc/profile"
        ;;
    *)
        config_files="$HOME/.bashrc $HOME/.bash_profile $XDG_CONFIG_HOME/bash/.bashrc $XDG_CONFIG_HOME/bash/.bash_profile"
        ;;
esac

if [[ "$no_modify_path" != "true" ]]; then
    config_file=""
    for file in $config_files; do
        if [[ -f $file ]]; then
            config_file=$file
            break
        fi
    done

    if [[ -z "$config_file" ]]; then
        echo -e "${ORANGE}Warning: No config file found for $current_shell. You may need to manually add to PATH:${NC}" >&2
        echo -e "  export PATH=$INSTALL_DIR:\$PATH"
    elif [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        case $current_shell in
            fish)
                add_to_path "$config_file" "fish_add_path $INSTALL_DIR"
                ;;
            *)
                add_to_path "$config_file" "export PATH=$INSTALL_DIR:\$PATH"
                ;;
        esac
    fi
fi

if [ -n "${GITHUB_ACTIONS-}" ] && [ "${GITHUB_ACTIONS}" == "true" ]; then
    echo "$INSTALL_DIR" >> $GITHUB_PATH
    echo -e "${MUTED}Added $INSTALL_DIR to \$GITHUB_PATH${NC}"
fi

echo -e ""
echo -e "${MUTED}                   ${NC}             ▄     "
echo -e "${MUTED}█▀▀█ █▀▀█ █▀▀█ █▀▀▄ ${NC}█▀▀▀ █▀▀█ █▀▀█ █▀▀█"
echo -e "${MUTED}█░░█ █░░█ █▀▀▀ █░░█ ${NC}█░░░ █░░█ █░░█ █▀▀▀"
echo -e "${MUTED}▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀  ▀ ${NC}▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"
echo -e ""
echo -e ""
echo -e "${MUTED}OpenCode includes free models, to start:${NC}"
echo -e ""
echo -e "cd <project>  ${MUTED}# Open directory${NC}"
echo -e "opencode      ${MUTED}# Run command${NC}"
echo -e ""
echo -e "${MUTED}For more information visit ${NC}https://opencode.ai/v2/docs"
echo -e ""
echo -e ""
