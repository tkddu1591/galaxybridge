#!/bin/bash
# Offline release installer. Inspect this file before running it.
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export LC_ALL=C
umask 077

readonly destination=/Library/PrivilegedHelperTools/io.galaxybridge
readonly plist=/Library/LaunchDaemons/io.galaxybridge.plist
readonly link=/usr/local/bin/galaxybridge
readonly label=io.galaxybridge
automatic=0
product=
serial=
stage=
installed=0
plist_created=0
link_created=0
committed=0
helper_created=0

installer::error() { printf 'GalaxyBridge: %s\n' "$*" >&2; exit 1; }

installer::toolchain::check() {
    local tool executable
    local instruction='Install Apple Command Line Tools once: xcode-select --install (needed for installer verification, not runtime)'
    # Query selection first: do not invoke tool shims that could open Apple's
    # interactive installation prompt on a clean Mac.
    /usr/bin/xcode-select -p >/dev/null 2>&1 || installer::error "$instruction"
    for tool in lipo otool; do
        executable=$(/usr/bin/xcrun --find "$tool" 2>/dev/null) || installer::error "$instruction"
        [[ -f "$executable" && -x "$executable" ]] || installer::error "$instruction"
    done
}

# All installed executable ancestors must be root-owned, non-writable by
# non-root users, and free of ACL grants. Deny-only ACLs are harmless.
installer::path::check() {
    local path=$1 mode
    while :; do
        [[ ! -L "$path" && -d "$path" ]] || installer::error "Unsafe directory: $path"
        [[ $(/usr/bin/stat -f %u "$path") == 0 ]] || installer::error "Directory is not root-owned: $path"
        mode=$(/usr/bin/stat -f %Lp "$path")
        (( (8#$mode & 0022) == 0 )) || installer::error "Directory is writable by another user: $path"
        if /bin/ls -lde "$path" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
            installer::error "Directory has an ACL grant; review it first: $path"
        fi
        [[ "$path" == / ]] && break
        path=$(/usr/bin/dirname "$path")
    done
}

installer::bundle::hash() {
    local name=$1 digest
    digest=$(/usr/bin/awk -v name="$name" '$2 == name { print $1 }' "$bundle/SHA256SUMS")
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || installer::error "Missing or duplicate checksum: $name"
    printf '%s' "$digest"
}

installer::bundle::copy() {
    local name=$1 target=$2 mode=$3 expected actual
    [[ -f "$bundle/$name" && ! -L "$bundle/$name" ]] || installer::error "Missing or symlinked release file: $name"
    expected=$(installer::bundle::hash "$name")
    /usr/bin/install -o root -g wheel -m "$mode" "$bundle/$name" "$target"
    actual=$(/usr/bin/shasum -a 256 "$target")
    [[ "${actual%% *}" == "$expected" ]] || installer::error "Checksum mismatch: $name"
    # The destination is now immutable to the user supplying the bundle.
}

installer::binary::dependency::check() {
    local dependency=$1
    # A textual prefix is insufficient: /usr/lib/../../tmp is not a system path.
    case "$dependency" in
        */../*|*/./*|*/..|*/.|*//*|*[[:space:]]*) installer::error "Unclean dynamic dependency path: $dependency" ;;
    esac
    case "$dependency" in
        /usr/lib/*|/System/Library/*) ;;
        *) installer::error "Unexpected dynamic dependency: $dependency" ;;
    esac
}

installer::transaction::rollback() {
    local result=$?
    trap - EXIT HUP INT TERM
    if (( ! committed )); then
        if (( plist_created )); then
            /bin/launchctl bootout "system/$label" >/dev/null 2>&1 || true
            /bin/rm -f "$plist"
        fi
        if (( link_created )) && [[ -L "$link" ]] && [[ $(/usr/bin/readlink "$link") == "$destination/galaxybridge" ]]; then
            /bin/rm "$link"
        fi
        if (( installed )); then
            /bin/rm -f "$destination/galaxybridge" "$destination/uninstall.sh" "$destination/INSTALLATION"
            /bin/rmdir "$destination" || true
        fi
    fi
    if [[ -n "$stage" && -d "$stage" && ! -L "$stage" ]]; then
        /bin/rm -f "$stage/galaxybridge" "$stage/uninstall.sh" "$stage/INSTALLATION" "$stage/service.plist" "$stage/libraries.txt"
        /bin/rmdir "$stage" || true
    fi
    if (( helper_created && ! committed )); then
        /bin/rmdir /Library/PrivilegedHelperTools || true
    fi
    exit "$result"
}

while (( $# )); do
    case "$1" in
        --auto) automatic=1; shift ;;
        --product)
            (( $# >= 2 )) || installer::error '--product needs four hexadecimal digits'
            [[ "$2" =~ ^[0-9A-Fa-f]{4}$ ]] || installer::error 'Invalid USB product ID'
            product=$2; shift 2 ;;
        --serial)
            (( $# >= 2 )) || installer::error '--serial needs a value'
            # Restrict the persisted argument to XML-safe ASCII, 1-128 characters.
            [[ "$2" =~ ^[A-Za-z0-9._:-]{1,128}$ ]] || installer::error 'Serial must contain 1-128 letters, digits, dots, underscores, colons or hyphens'
            serial=$2; shift 2 ;;
        --help|-h)
            printf 'Usage: ./install.sh [--auto] [--product HEX] [--serial SERIAL]\n\nInstalls an offline, checksummed Apple Silicon release. --auto enables a system\nservice for reconnect/boot. Without it, start manually with sudo galaxybridge connect.\nDevice filters apply to --auto; supply them to connect for manual sessions.\nNo download, account, SIP change, USB debugging, or Homebrew is needed.\n'
            exit 0 ;;
        *) installer::error "Unknown option: $1" ;;
    esac
done

[[ $(/usr/bin/uname -s) == Darwin ]] || installer::error 'macOS is required'
[[ $(/usr/sbin/sysctl -n hw.optional.arm64) == 1 ]] || installer::error 'An Apple Silicon Mac is required'
os_version=$(/usr/bin/sw_vers -productVersion)
os_major=${os_version%%.*}
os_minor=${os_version#*.}; os_minor=${os_minor%%.*}
(( os_major > 13 || (os_major == 13 && os_minor >= 3) )) || installer::error 'macOS 13.3 or later is required'
bundle=$(cd -- "$(/usr/bin/dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
readonly bundle
[[ -f "$bundle/SHA256SUMS" && ! -L "$bundle/SHA256SUMS" ]] || installer::error 'Run install.sh from the extracted release bundle (SHA256SUMS is required)'
installer::toolchain::check

if (( EUID != 0 )); then
    # Bash 3.2 treats an empty array as unset under nounset.
    args=("$bundle/install.sh")
    (( automatic )) && args+=(--auto)
    [[ -z "$product" ]] || args+=(--product "$product")
    [[ -z "$serial" ]] || args+=(--serial "$serial")
    printf 'Installing GalaxyBridge%s. macOS will request administrator authentication once.\n' "$( (( automatic )) && printf ' with automatic reconnect' || true )"
    exec /usr/bin/sudo -- /bin/bash "${args[@]}"
fi

installer::path::check /Library
installer::path::check /Library/LaunchDaemons
[[ ! -e "$destination" && ! -L "$destination" ]] || installer::error 'An installation or conflicting path already exists. Run its uninstaller before installing a new version.'
[[ ! -e "$plist" && ! -L "$plist" ]] || installer::error "Refusing to overwrite: $plist"
if /bin/launchctl print "system/$label" >/dev/null 2>&1; then
    installer::error "Service $label already exists; stop and uninstall it first"
fi
if /usr/bin/pgrep -x tetherkit-cli >/dev/null; then
    installer::error 'TetherKit is running. Stop its service and driver first; this installer will not modify it.'
fi
if /usr/bin/pgrep -x galaxybridge >/dev/null; then
    installer::error 'GalaxyBridge is already running; stop it before installation'
fi

trap installer::transaction::rollback EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ ! -e /Library/PrivilegedHelperTools && ! -L /Library/PrivilegedHelperTools ]]; then
    /bin/mkdir -m 755 /Library/PrivilegedHelperTools
    helper_created=1
fi
installer::path::check /Library/PrivilegedHelperTools
stage=$(/usr/bin/mktemp -d /Library/PrivilegedHelperTools/.io.galaxybridge.XXXXXXXX)
installer::bundle::copy bin/galaxybridge "$stage/galaxybridge" 755
installer::bundle::copy uninstall.sh "$stage/uninstall.sh" 755
[[ $(/usr/bin/lipo -archs "$stage/galaxybridge") == arm64 ]] || installer::error 'Release binary must be native arm64'
/usr/bin/codesign --verify --strict "$stage/galaxybridge" || installer::error 'Binary code signature validation failed'
# Reject a binary that would load a user-writable library as root.
/usr/bin/otool -L "$stage/galaxybridge" > "$stage/libraries.txt"
while IFS= read -r dependency; do
    installer::binary::dependency::check "$dependency"
done < <(/usr/bin/sed '1d;s/^[[:space:]]*//;s/ (compatibility version.*//' "$stage/libraries.txt")
printf 'GalaxyBridge installation format 1\n' > "$stage/INSTALLATION"
/bin/chmod 644 "$stage/INSTALLATION"

if (( automatic )); then
    {
        cat <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>io.galaxybridge</string>
<key>ProgramArguments</key><array>
<string>/Library/PrivilegedHelperTools/io.galaxybridge/galaxybridge</string>
<string>daemon</string>
PLIST
        [[ -z "$product" ]] || printf '<string>--product</string><string>%s</string>\n' "$product"
        [[ -z "$serial" ]] || printf '<string>--serial</string><string>%s</string>\n' "$serial"
        cat <<'PLIST'
</array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>10</integer>
<key>ExitTimeOut</key><integer>15</integer>
<key>ProcessType</key><string>Background</string>
<key>Umask</key><integer>63</integer>
<key>WorkingDirectory</key><string>/</string>
<key>EnvironmentVariables</key><dict>
<key>PATH</key><string>/usr/bin:/bin:/usr/sbin:/sbin</string>
</dict>
</dict></plist>
PLIST
    } > "$stage/service.plist"
    /usr/bin/plutil -lint "$stage/service.plist" >/dev/null
fi

# mkdir is the atomic no-clobber reservation; mv into an existing directory
# would risk writing into someone else's installation.
/bin/mkdir -m 755 "$destination"
installed=1
/bin/mv "$stage/galaxybridge" "$stage/uninstall.sh" "$stage/INSTALLATION" "$destination/"

# A convenience link is optional; the service always uses the immutable path.
if [[ ! -e "$link" && ! -L "$link" && -d /usr/local/bin ]] && (installer::path::check /usr/local/bin) 2>/dev/null; then
    /bin/ln -s "$destination/galaxybridge" "$link"
    link_created=1
else
    printf 'No convenience link created; use %s/galaxybridge.\n' "$destination"
fi
if (( automatic )); then
    # Root-only parent and prior absence validation prevent an unprivileged race.
    /usr/bin/install -o root -g wheel -m 600 "$stage/service.plist" "$plist"
    plist_created=1
    /bin/launchctl bootstrap system "$plist"
    /bin/launchctl print "system/$label" >/dev/null
fi
committed=1
printf 'GalaxyBridge installed.\n'
if (( automatic )); then
    printf 'Automatic reconnect is enabled. Turn on USB tethering on the phone.\n'
else
    printf 'Start: sudo %s/galaxybridge connect\n' "$destination"
fi
printf 'Uninstall: sudo %s/uninstall.sh\n' "$destination"
