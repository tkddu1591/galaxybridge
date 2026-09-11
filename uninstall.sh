#!/bin/bash
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export LC_ALL=C
unset BASH_ENV ENV CDPATH PERL5OPT PERL5LIB PERLLIB PERL5DB DEVELOPER_DIR SDKROOT TOOLCHAINS
umask 077
readonly destination=/Library/PrivilegedHelperTools/io.galaxybridge
readonly plist=/Library/LaunchDaemons/io.galaxybridge.plist
readonly link=/usr/local/bin/galaxybridge
readonly lock=/Library/PrivilegedHelperTools/.io.galaxybridge-install.lock
lock_acquired=0

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
    [[ $(/usr/bin/stat -f %u "$file") == 0 && $(/usr/bin/stat -f %l "$file") == 1 ]] || uninstaller::error "File is not exclusively root-owned: $file"
    mode=$(/usr/bin/stat -f %Lp "$file")
    (( (8#$mode & 0022) == 0 )) || uninstaller::error "File is writable by another user: $file"
    if /bin/ls -le "$file" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
        uninstaller::error "File has an ACL grant: $file"
    fi
}

uninstaller::lock::release() {
    local result=$?
    trap - EXIT HUP INT TERM
    if (( lock_acquired )); then /bin/rmdir "$lock" || true; fi
    exit "$result"
}

if (( $# )); then
    [[ $# == 1 && ( "$1" == --help || "$1" == -h ) ]] || uninstaller::error 'No arguments accepted'
    printf 'Usage: ./uninstall.sh\nStops the installed service and removes only validated GalaxyBridge files.\n'
    exit 0
fi
[[ $(/usr/bin/uname -s) == Darwin ]] || uninstaller::error 'macOS is required'
# Validate before sudo as well: an unsafe old installation must never become
# an opportunity to execute its replaced uninstall script as administrator.
uninstaller::path::check "$destination"
uninstaller::file::check "$destination/uninstall.sh"
if (( EUID != 0 )); then
    # Execute the installed root-owned copy, never another user's uninstall code.
    exec /usr/bin/sudo -- /usr/bin/env -i PATH="$PATH" LC_ALL=C /bin/bash "$destination/uninstall.sh"
fi
uninstaller::path::check "$destination"
uninstaller::path::check /Library/LaunchDaemons
for name in galaxybridge uninstall.sh identity.sh IDENTITY INSTALLATION; do
    uninstaller::file::check "$destination/$name"
done
for directory in USBWorker.app USBWorker.app/Contents USBWorker.app/Contents/MacOS USBWorker.app/Contents/_CodeSignature; do
    uninstaller::path::check "$destination/$directory"
done
for name in USBWorker.app/Contents/Info.plist USBWorker.app/Contents/MacOS/galaxybridge-usb USBWorker.app/Contents/_CodeSignature/CodeResources; do
    uninstaller::file::check "$destination/$name"
done
[[ $(/usr/bin/stat -f %Lp "$destination/IDENTITY") == 600 ]] || uninstaller::error 'IDENTITY receipt must be private (mode 600)'
[[ $(/bin/cat "$destination/INSTALLATION") == 'GalaxyBridge installation format 1' ]] || uninstaller::error 'Installation marker does not match'
shopt -s nullglob dotglob
for file in "$destination"/*; do
    case "${file##*/}" in galaxybridge|USBWorker.app|uninstall.sh|identity.sh|IDENTITY|INSTALLATION) ;; *) uninstaller::error "Unexpected file; leaving installation intact: $file" ;; esac
done
worker_entries=$(/usr/bin/find -P "$destination/USBWorker.app" -print)
while IFS= read -r file; do
    case "${file#"$destination/USBWorker.app"}" in
        ''|/Contents|/Contents/Info.plist|/Contents/MacOS|/Contents/MacOS/galaxybridge-usb|/Contents/_CodeSignature|/Contents/_CodeSignature/CodeResources) ;;
        *) uninstaller::error 'Unexpected worker app entry; leaving installation intact' ;;
    esac
done <<< "$worker_entries"
trap uninstaller::lock::release EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
/bin/mkdir -m 700 "$lock" || uninstaller::error "Another install/uninstall operation or stale lock exists: $lock"
lock_acquired=1
. "$destination/identity.sh"
identity::receipt::load "$destination/IDENTITY"
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
identity::installation::delete "$destination/IDENTITY"
if [[ -L "$link" ]] && [[ $(/usr/bin/readlink "$link") == "$destination/galaxybridge" ]]; then
    # Never unlink through a user-controlled parent.
    if (uninstaller::path::check /usr/local/bin) 2>/dev/null; then
        /bin/rm "$link"
    else
        printf 'Leaving convenience link in an untrusted directory: %s\n' "$link"
    fi
fi
[[ ! -e "$plist" ]] || /bin/rm "$plist"
/bin/rm "$destination/USBWorker.app/Contents/Info.plist" "$destination/USBWorker.app/Contents/MacOS/galaxybridge-usb" "$destination/USBWorker.app/Contents/_CodeSignature/CodeResources"
/bin/rmdir "$destination/USBWorker.app/Contents/MacOS" "$destination/USBWorker.app/Contents/_CodeSignature" "$destination/USBWorker.app/Contents" "$destination/USBWorker.app"
/bin/rm "$destination/galaxybridge" "$destination/uninstall.sh" "$destination/identity.sh" "$destination/IDENTITY" "$destination/INSTALLATION"
/bin/rmdir "$destination"
printf 'GalaxyBridge uninstalled. Phone USB preferences were not changed.\n'
