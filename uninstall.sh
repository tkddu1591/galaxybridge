#!/bin/bash
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export LC_ALL=C
umask 077
readonly destination=/Library/PrivilegedHelperTools/io.galaxybridge
readonly plist=/Library/LaunchDaemons/io.galaxybridge.plist
readonly link=/usr/local/bin/galaxybridge

uninstaller::error() { printf 'GalaxyBridge: %s\n' "$*" >&2; exit 1; }
uninstaller::path::check() {
    local path=$1 mode
    while :; do
        [[ ! -L "$path" && -d "$path" ]] || uninstaller::error "Unsafe directory: $path"
        [[ $(/usr/bin/stat -f %u "$path") == 0 ]] || uninstaller::error "Directory is not root-owned: $path"
        mode=$(/usr/bin/stat -f %Lp "$path")
        (( (8#$mode & 0022) == 0 )) || uninstaller::error "Directory is writable by another user: $path"
        if /bin/ls -lde "$path" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
            uninstaller::error "Directory has an ACL grant: $path"
        fi
        [[ "$path" == / ]] && break
        path=$(/usr/bin/dirname "$path")
    done
}
uninstaller::file::check() {
    local file=$1 mode
    [[ -f "$file" && ! -L "$file" ]] || uninstaller::error "Missing or unsafe installation file: $file"
    [[ $(/usr/bin/stat -f %u "$file") == 0 ]] || uninstaller::error "File is not root-owned: $file"
    mode=$(/usr/bin/stat -f %Lp "$file")
    (( (8#$mode & 0022) == 0 )) || uninstaller::error "File is writable by another user: $file"
    if /bin/ls -le "$file" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
        uninstaller::error "File has an ACL grant: $file"
    fi
}

if (( $# )); then
    [[ $# == 1 && ( "$1" == --help || "$1" == -h ) ]] || uninstaller::error 'No arguments accepted'
    printf 'Usage: ./uninstall.sh\nStops the installed service and removes only validated GalaxyBridge files.\n'
    exit 0
fi
[[ $(/usr/bin/uname -s) == Darwin ]] || uninstaller::error 'macOS is required'
if (( EUID != 0 )); then
    # Execute the installed root-owned copy, never another user's uninstall code.
    exec /usr/bin/sudo -- /bin/bash "$destination/uninstall.sh"
fi
uninstaller::path::check "$destination"
uninstaller::path::check /Library/LaunchDaemons
for name in galaxybridge uninstall.sh INSTALLATION; do
    uninstaller::file::check "$destination/$name"
done
[[ $(/bin/cat "$destination/INSTALLATION") == 'GalaxyBridge installation format 1' ]] || uninstaller::error 'Installation marker does not match'
shopt -s nullglob dotglob
for file in "$destination"/*; do
    case "${file##*/}" in galaxybridge|uninstall.sh|INSTALLATION) ;; *) uninstaller::error "Unexpected file; leaving installation intact: $file" ;; esac
done
if [[ -e "$plist" || -L "$plist" ]]; then
    uninstaller::file::check "$plist"
    [[ $(/usr/bin/plutil -extract Label raw -o - "$plist") == io.galaxybridge ]] || uninstaller::error 'LaunchDaemon label does not match'
    [[ $(/usr/bin/plutil -extract ProgramArguments.0 raw -o - "$plist") == "$destination/galaxybridge" ]] || uninstaller::error 'LaunchDaemon executable does not match'
    if /bin/launchctl print system/io.galaxybridge >/dev/null 2>&1; then
        /bin/launchctl bootout system/io.galaxybridge
    fi
elif /bin/launchctl print system/io.galaxybridge >/dev/null 2>&1; then
    uninstaller::error 'A service with this label exists without our plist; refusing to remove it'
fi
# bootout delivers SIGTERM; allow route/interface cleanup to finish.
for (( attempt=0; attempt<20; attempt++ )); do
    if ! /usr/bin/pgrep -x galaxybridge >/dev/null; then break; fi
    /bin/sleep 1
done
if /usr/bin/pgrep -x galaxybridge >/dev/null; then
    uninstaller::error 'A GalaxyBridge process remains. Stop manual sessions (Ctrl+C), then run the uninstaller again.'
fi
if [[ -L "$link" ]] && [[ $(/usr/bin/readlink "$link") == "$destination/galaxybridge" ]]; then
    # Never unlink through a user-controlled parent.
    if (uninstaller::path::check /usr/local/bin) 2>/dev/null; then
        /bin/rm "$link"
    else
        printf 'Leaving convenience link in an untrusted directory: %s\n' "$link"
    fi
fi
[[ ! -e "$plist" ]] || /bin/rm "$plist"
/bin/rm "$destination/galaxybridge" "$destination/uninstall.sh" "$destination/INSTALLATION"
/bin/rmdir "$destination"
printf 'GalaxyBridge uninstalled. Phone USB preferences were not changed.\n'
