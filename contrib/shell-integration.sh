# Shell integration for omafile: `of` opens the file manager and cd's to the
# directory you quit in. Source this from ~/.bashrc or ~/.zshrc:
#
#   source ~/.local/share/omafile/shell-integration.sh

of() {
    local tmp dir
    tmp="$(mktemp -t omafile-cwd.XXXXXX)" || return 1
    omafile --cwd-file "$tmp" "$@"
    dir="$(cat "$tmp" 2>/dev/null)"
    rm -f "$tmp"
    if [ -n "$dir" ] && [ "$dir" != "$PWD" ] && [ -d "$dir" ]; then
        cd "$dir" || return 1
    fi
}
