#!/bin/sh
# Installs the dagon command from a GitHub release, for Linux and macOS.
#
#     curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.sh | sh
#
# A version:  ... | sh -s -- 1.2.3   or  DAGON_VERSION=1.2.3  (default: the latest)
# The folder: DAGON_INSTALL_DIR      (default: ~/.local/bin)
#
# It stops on any download or checksum error and leaves the folder as it was.
# It needs no administrator rights.
#
# Everything is inside main, called on the last line, so a download cut short
# runs nothing.

REPO="c-ardinal/DAGON-Boot"
# A list that never ends stops here instead of running forever.
MAX_PAGES=100

fail() {
    echo "dagon install: $1" >&2
    exit 1
}

fetch() {
    curl --proto '=https' --tlsv1.2 -fsSL "$@"
}

target_of() {
    system="$(uname -s)"
    machine="$(uname -m)"
    case "$system/$machine" in
        Linux/x86_64 | Linux/amd64) echo "x86_64-unknown-linux-gnu" ;;
        Darwin/arm64 | Darwin/aarch64) echo "aarch64-apple-darwin" ;;
        Darwin/x86_64) echo "x86_64-apple-darwin" ;;
        *) fail "no release for $system $machine; see https://github.com/$REPO#readme to build it yourself" ;;
    esac
}

latest_version() {
    # Every component has its own releases: only cli-v<version> counts, and a
    # pre-release does not. The list comes newest first, a page at a time; the
    # page after the last one holds no release.
    page=1
    while [ "$page" -le "$MAX_PAGES" ]; do
        list="$(fetch "https://api.github.com/repos/$REPO/releases?per_page=100&page=$page")" \
            || fail "cannot read the list of releases; name a version with DAGON_VERSION"
        found="$(printf '%s\n' "$list" | tr ',' '\n' | sed -n 's/.*"tag_name": *"cli-v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)".*/\1/p' | head -n 1)"
        if [ -n "$found" ]; then
            printf '%s\n' "$found"
            return 0
        fi
        case "$list" in
            *'"tag_name"'*) page=$((page + 1)) ;;
            *) break ;;
        esac
    done
    fail "no release of the dagon command found"
}

sha256_of() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "needs sha256sum or shasum to check the download"
    fi
}

main() {
    set -eu
    command -v curl > /dev/null 2>&1 || fail "needs curl"
    command -v tar > /dev/null 2>&1 || fail "needs tar"

    target="$(target_of)"
    version="${1:-${DAGON_VERSION:-}}"
    if [ -z "$version" ]; then
        version="$(latest_version)"
    fi
    case "$version" in
        *[!0-9A-Za-z.-]* | "" | [!0-9]*) fail "not a version: $version" ;;
    esac

    dir="${DAGON_INSTALL_DIR:-${HOME:?set HOME or DAGON_INSTALL_DIR}/.local/bin}"
    archive="dagon-cli-$version-$target.tar.gz"
    url="https://github.com/$REPO/releases/download/cli-v$version/$archive"

    tmp="$(mktemp -d)" || fail "cannot make a temporary folder"
    staged=""
    trap 'rm -rf "$tmp"; [ -z "$staged" ] || rm -f "$staged"' EXIT

    fetch -o "$tmp/$archive" "$url" || fail "cannot download $url"
    fetch -o "$tmp/$archive.sha256" "$url.sha256" || fail "cannot download $url.sha256"

    want="$(cut -d ' ' -f 1 "$tmp/$archive.sha256")"
    got="$(sha256_of "$tmp/$archive")"
    case "$want" in
        *[!0-9a-f]* | "") fail "$archive.sha256 holds no checksum" ;;
    esac
    [ "${#want}" -eq 64 ] || fail "$archive.sha256 holds no checksum"
    [ "$want" = "$got" ] || fail "$archive does not match its checksum; nothing was installed"

    mkdir "$tmp/unpacked"
    tar -xzf "$tmp/$archive" -C "$tmp/unpacked" dagon || fail "$archive holds no dagon"
    [ -f "$tmp/unpacked/dagon" ] || fail "$archive holds no dagon"

    mkdir -p "$dir" || fail "cannot make $dir"
    # A file of this run's own in the destination, so installs that overlap
    # never share one and the rename below is atomic.
    staged="$(mktemp "$dir/.dagon.XXXXXX")" || fail "cannot write to $dir"
    cp "$tmp/unpacked/dagon" "$staged" || fail "cannot write to $dir"
    chmod 755 "$staged" || fail "cannot write to $dir"
    mv -f "$staged" "$dir/dagon" || fail "cannot write to $dir"
    staged=""

    echo "dagon $version ($target) installed in $dir"
    case ":${PATH:-}:" in
        *":$dir:"*) ;;
        *)
            echo "$dir is not on your PATH. Add this line to your shell's profile:"
            echo "    export PATH=\"$dir:\$PATH\""
            ;;
    esac
}

main "$@"
