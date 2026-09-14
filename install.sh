#!/bin/bash
# Offline release installer. Inspect this file before running it.
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export LC_ALL=C
unset BASH_ENV ENV CDPATH PERL5OPT PERL5LIB PERLLIB PERL5DB DEVELOPER_DIR SDKROOT TOOLCHAINS
umask 077

readonly destination=/Library/PrivilegedHelperTools/io.galaxybridge
readonly plist=/Library/LaunchDaemons/io.galaxybridge.plist
readonly link=/usr/local/bin/galaxybridge
readonly label=io.galaxybridge
readonly lock=/Library/PrivilegedHelperTools/.io.galaxybridge-install.lock
automatic=0
vendor=
product=
serial=
stage=
installed=0
plist_created=0
link_created=0
committed=0
helper_created=0
lock_acquired=0
identity_ready=0
identity_receipt=
tool_lipo=
tool_otool=

installer::error() { printf 'GalaxyBridge: %s\n' "$*" >&2; exit 1; }

installer::source::uid::get() {
    local effective=$1 sudo_uid=$2 owner
    if [[ "$effective" == 0 ]]; then owner=${sudo_uid:-0}; else owner=$effective; fi
    [[ "$owner" =~ ^(0|[1-9][0-9]{0,9})$ ]] || installer::error 'Invalid invoking UID for source validation'
    (( owner <= 4294967294 )) || installer::error 'Invoking UID is out of range'
    printf '%s' "$owner"
}

installer::source::error() {
    installer::error "Unsafe release source: $1. Copy the extracted bundle into a private folder owned by your account, then retry."
}

installer::source::directory::check() {
    local path=$1 allowed_uid=$2 owner mode acl child=
    while :; do
        [[ -d "$path" && ! -L "$path" ]] || installer::source::error "missing or symlinked directory $path"
        owner=$(/usr/bin/stat -f %u "$path") || installer::source::error "cannot inspect owner of $path"
        [[ "$owner" == 0 || "$owner" == "$allowed_uid" ]] || installer::source::error "directory owned by another user: $path"
        # %Lp omits the sticky bit on macOS; %p retains full octal st_mode.
        mode=$(/usr/bin/stat -f %p "$path") || installer::source::error "cannot inspect permissions of $path"
        [[ "$mode" =~ ^[0-7]{5,6}$ ]] || installer::source::error "invalid permissions on $path"
        acl=$(/bin/ls -lde "$path") || installer::source::error "cannot inspect ACLs of $path"
        if /usr/bin/sed '1d' <<< "$acl" | /usr/bin/grep -q ' allow '; then
            installer::source::error "directory has an ACL grant: $path"
        fi
        if (( (8#$mode & 0022) != 0 )); then
            # A root-owned sticky parent cannot let another user replace the
            # protected child just verified below it (e.g. private mktemp dir
            # under /private/tmp). Never permit the bundle directly in shared tmp.
            [[ "$owner" == 0 && -n "$child" ]] && (( (8#$mode & 01000) != 0 )) || installer::source::error "directory writable by another user: $path"
        fi
        [[ "$path" == / ]] && break
        child=$path
        path=$(/usr/bin/dirname "$path")
    done
}

installer::source::file::check() {
    local file=$1 allowed_uid=$2 owner mode acl
    [[ -f "$file" && ! -L "$file" ]] || installer::source::error "missing or symlinked file $file"
    owner=$(/usr/bin/stat -f %u "$file") || installer::source::error "cannot inspect owner of $file"
    [[ "$owner" == 0 || "$owner" == "$allowed_uid" ]] || installer::source::error "file owned by another user: $file"
    [[ $(/usr/bin/stat -f %l "$file") == 1 ]] || installer::source::error "hard-linked file $file"
    mode=$(/usr/bin/stat -f %Lp "$file") || installer::source::error "cannot inspect permissions of $file"
    [[ "$mode" =~ ^[0-7]{3,4}$ ]] && (( (8#$mode & 0022) == 0 )) || installer::source::error "file writable by another user: $file"
    acl=$(/bin/ls -le "$file") || installer::source::error "cannot inspect ACLs of $file"
    if /usr/bin/sed '1d' <<< "$acl" | /usr/bin/grep -q ' allow '; then
        installer::source::error "file has an ACL grant: $file"
    fi
    installer::source::directory::check "$(/usr/bin/dirname "$file")" "$allowed_uid"
}

installer::source::bundle::check() {
    local source=$1 allowed_uid=$2 file
    installer::source::directory::check "$source" "$allowed_uid"
    for file in install.sh SHA256SUMS bin/galaxybridge uninstall.sh libexec/identity.sh \
        libexec/USBWorker.app/Contents/Info.plist \
        libexec/USBWorker.app/Contents/MacOS/galaxybridge-usb \
        libexec/USBWorker.app/Contents/_CodeSignature/CodeResources; do
        installer::source::file::check "$source/$file" "$allowed_uid"
    done
}

installer::toolchain::check() {
    local tool executable developer dispatcher
    local instruction='Install Apple Command Line Tools once: xcode-select --install (needed for installer verification, not runtime)'
    # Query selection first: do not invoke tool shims that could open Apple's
    # interactive installation prompt on a clean Mac.
    developer=$(/usr/bin/env -i PATH="$PATH" LC_ALL=C /usr/bin/xcode-select -p 2>/dev/null) || installer::error "$instruction"
    installer::path::check "$developer"
    # xcrun may execute the selected developer directory's own dispatcher.
    # Validate it BEFORE asking xcrun to locate any other executable.
    dispatcher="$developer/usr/bin/xcrun"
    if [[ -e "$dispatcher" || -L "$dispatcher" ]]; then
        installer::file::path::get "$dispatcher" >/dev/null
    fi
    for tool in lipo otool; do
        executable=$(/usr/bin/env -i PATH="$PATH" LC_ALL=C DEVELOPER_DIR="$developer" /usr/bin/xcrun --find "$tool" 2>/dev/null) || installer::error "$instruction"
        [[ -f "$executable" && -x "$executable" ]] || installer::error "$instruction"
        executable=$(installer::file::path::get "$executable")
        case "$tool" in lipo) tool_lipo=$executable ;; otool) tool_otool=$executable ;; esac
    done
}

installer::file::check() {
    local file=$1 mode
    [[ -f "$file" && ! -L "$file" ]] || installer::error "Unsafe regular file: $file"
    [[ $(/usr/bin/stat -f %u "$file") == 0 && $(/usr/bin/stat -f %l "$file") == 1 ]] || installer::error "File is not exclusively root-owned: $file"
    mode=$(/usr/bin/stat -f %Lp "$file")
    (( (8#$mode & 0022) == 0 )) || installer::error "File is writable by another user: $file"
    if /bin/ls -le "$file" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
        installer::error "File has an ACL grant: $file"
    fi
}

installer::file::path::get() {
    local file=$1 target parent attempt
    [[ "$file" == /* && "$file" != *$'\n'* ]] || installer::error 'Toolchain returned an invalid absolute path'
    for (( attempt=0; attempt<16; attempt++ )); do
        parent=$(/usr/bin/dirname "$file")
        installer::path::check "$parent"
        if [[ ! -L "$file" ]]; then
            installer::file::check "$file"
            [[ -x "$file" ]] || installer::error "Tool is not executable: $file"
            parent=$(cd -- "$parent" && pwd -P)
            printf '%s/%s' "$parent" "${file##*/}"
            return
        fi
        [[ $(/usr/bin/stat -f %u "$file") == 0 ]] || installer::error "Tool symlink is not root-owned: $file"
        target=$(/usr/bin/readlink "$file")
        case "$target" in /*) file=$target ;; *) file="$parent/$target" ;; esac
    done
    installer::error 'Too many toolchain symlinks'
}

installer::file::metadata::clear() {
    # macOS install(1) preserves source ACLs and BSD flags even with -m/-o.
    # Quarantine and other extended attributes are deliberately left intact.
    /usr/bin/chflags 0 "$1"
    /bin/chmod -N "$1"
    /bin/chmod "$2" "$1"
}

installer::file::copy() {
    /usr/bin/install -f '' -o root -g wheel -m "$3" "$1" "$2"
    installer::file::metadata::clear "$2" "$3"
    installer::file::check "$2"
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
    installer::file::copy "$bundle/$name" "$target" "$mode"
    actual=$(/usr/bin/env -i PATH="$PATH" LC_ALL=C /usr/bin/shasum -a 256 "$target")
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

installer::worker::entitlements::check() {
    local file=$1 sandbox usb remainder
    sandbox=$(/usr/bin/plutil -extract 'com\.apple\.security\.app-sandbox' raw -expect bool "$file")
    usb=$(/usr/bin/plutil -extract 'com\.apple\.security\.device\.usb' raw -expect bool "$file")
    [[ "$sandbox" == true && "$usb" == true ]] || installer::error 'Worker App Sandbox and USB entitlements must both be boolean true'
    /usr/bin/plutil -remove 'com\.apple\.security\.app-sandbox' "$file"
    /usr/bin/plutil -remove 'com\.apple\.security\.device\.usb' "$file"
    remainder=$(/usr/bin/plutil -convert json -o - "$file")
    [[ "$remainder" == '{}' ]] || installer::error 'Additional worker entitlements are forbidden'
}

installer::file::quarantine::copy() {
    local attributes quarantine
    attributes=$(/usr/bin/xattr "$1")
    if /usr/bin/grep -Fxq com.apple.quarantine <<< "$attributes"; then
        quarantine=$(/usr/bin/xattr -px com.apple.quarantine "$1")
        /usr/bin/xattr -wx com.apple.quarantine "$quarantine" "$2"
    fi
}

installer::worker::copy() {
    local app="$stage/USBWorker.app" source_app="$bundle/libexec/USBWorker.app" directory relative
    [[ -d "$source_app" && ! -L "$source_app" ]] || installer::error 'Missing or symlinked USBWorker.app'
    /bin/mkdir -m 755 "$app" "$app/Contents" "$app/Contents/MacOS" "$app/Contents/_CodeSignature"
    for relative in Contents/Info.plist Contents/MacOS/galaxybridge-usb Contents/_CodeSignature/CodeResources; do
        if [[ "$relative" == Contents/MacOS/galaxybridge-usb ]]; then
            installer::bundle::copy "libexec/USBWorker.app/$relative" "$app/$relative" 755
        else
            installer::bundle::copy "libexec/USBWorker.app/$relative" "$app/$relative" 644
        fi
    done
    # Directory quarantine is distinct from executable quarantine; carry both.
    for directory in '' /Contents /Contents/MacOS /Contents/_CodeSignature; do
        installer::file::quarantine::copy "$source_app$directory" "$app$directory"
    done
    /usr/bin/codesign --verify --strict "$app" || installer::error 'Worker app signature verification failed'
    [[ $(/usr/bin/plutil -extract CFBundleIdentifier raw -expect string "$app/Contents/Info.plist") == io.galaxybridge.usb-worker ]] || installer::error 'Unexpected worker bundle identifier'
    [[ $(/usr/bin/plutil -extract CFBundleExecutable raw -expect string "$app/Contents/Info.plist") == galaxybridge-usb ]] || installer::error 'Unexpected worker executable name'
    [[ $(/usr/bin/plutil -extract CFBundlePackageType raw -expect string "$app/Contents/Info.plist") == APPL ]] || installer::error 'Worker must be an application bundle'
    /usr/bin/codesign --display --entitlements :- "$app" > "$stage/worker-entitlements.plist" 2>/dev/null
    installer::worker::entitlements::check "$stage/worker-entitlements.plist"
    [[ $("$tool_lipo" -archs "$app/Contents/MacOS/galaxybridge-usb") == arm64 ]] || installer::error 'Worker must be native arm64'
    "$tool_otool" -L "$app/Contents/MacOS/galaxybridge-usb" > "$stage/worker-libraries.txt"
    while IFS= read -r dependency; do
        installer::binary::dependency::check "$dependency"
    done < <(/usr/bin/sed '1d;s/^[[:space:]]*//;s/ (compatibility version.*//' "$stage/worker-libraries.txt")
}

installer::worker::files::delete() {
    local app="$1/USBWorker.app"
    [[ -d "$app" && ! -L "$app" ]] || return 0
    /bin/rm -f "$app/Contents/MacOS/galaxybridge-usb" "$app/Contents/Info.plist" "$app/Contents/_CodeSignature/CodeResources"
    /bin/rmdir "$app/Contents/MacOS" "$app/Contents/_CodeSignature" "$app/Contents" "$app" || true
}

installer::transaction::rollback() {
    local result=$? preserve=0
    trap - EXIT HUP INT TERM
    if (( ! committed )); then
        if (( plist_created )); then
            /bin/launchctl bootout "system/$label" >/dev/null 2>&1 || true
            /bin/rm -f "$plist"
        fi
        if (( identity_ready )) && [[ -f "$identity_receipt" ]]; then
            if ! identity::installation::delete "$identity_receipt" 1; then
                preserve=1
                printf 'GalaxyBridge: account cleanup could not prove ownership or quiescence; retaining recovery files at %s\n' "${identity_receipt%/*}" >&2
            fi
        fi
        if (( link_created )) && [[ -L "$link" ]] && [[ $(/usr/bin/readlink "$link") == "$destination/galaxybridge" ]]; then
            /bin/rm "$link"
        fi
        if (( installed && ! preserve )); then
            installer::worker::files::delete "$destination"
            /bin/rm -f "$destination/galaxybridge" "$destination/uninstall.sh" "$destination/identity.sh" "$destination/IDENTITY" "$destination/INSTALLATION"
            /bin/rmdir "$destination" || true
        fi
    fi
    if (( ! preserve )) && [[ -n "$stage" && -d "$stage" && ! -L "$stage" ]]; then
        installer::worker::files::delete "$stage"
        /bin/rm -f "$stage/galaxybridge" "$stage/uninstall.sh" "$stage/identity.sh" "$stage/IDENTITY" "$stage/INSTALLATION" "$stage/service.plist" "$stage/libraries.txt" "$stage/worker-libraries.txt" "$stage/worker-entitlements.plist" "$stage/supervisor-entitlements.plist"
        /bin/rmdir "$stage" || true
    fi
    if (( lock_acquired )); then /bin/rmdir "$lock" || true; fi
    if (( helper_created && ! committed )); then
        /bin/rmdir /Library/PrivilegedHelperTools || true
    fi
    exit "$result"
}

while (( $# )); do
    case "$1" in
        --auto) automatic=1; shift ;;
        --vendor)
            (( $# >= 2 )) || installer::error '--vendor needs four hexadecimal digits'
            [[ "$2" =~ ^[0-9A-Fa-f]{4}$ ]] || installer::error 'Invalid USB vendor ID'
            vendor=$2; shift 2 ;;
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
            printf 'Usage: ./install.sh [--auto] [--vendor HEX] [--product HEX] [--serial SERIAL]\n\nInstalls an offline, checksummed Apple Silicon release and a disabled local worker\nidentity. --auto enables a system service for reconnect/boot. Without it, start\nmanually with sudo galaxybridge connect. Device filters apply to --auto; supply\nthem to connect for manual sessions. USB identifiers are not authentication.\nNo download, online account, SIP change, USB debugging, or Homebrew is needed.\n'
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
source_uid=$(installer::source::uid::get "$EUID" "${SUDO_UID:-}")
readonly source_uid
installer::source::bundle::check "$bundle" "$source_uid"
installer::toolchain::check

if (( EUID != 0 )); then
    # Bash 3.2 treats an empty array as unset under nounset.
    args=("$bundle/install.sh")
    (( automatic )) && args+=(--auto)
    [[ -z "$vendor" ]] || args+=(--vendor "$vendor")
    [[ -z "$product" ]] || args+=(--product "$product")
    [[ -z "$serial" ]] || args+=(--serial "$serial")
    printf 'Installing GalaxyBridge%s. macOS will request administrator authentication once.\n' "$( (( automatic )) && printf ' with automatic reconnect' || true )"
    exec /usr/bin/sudo -- /usr/bin/env -i PATH="$PATH" LC_ALL=C SUDO_UID="$source_uid" /bin/bash "${args[@]}"
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
/bin/mkdir -m 700 "$lock" || installer::error "Another install/uninstall operation or stale lock exists: $lock"
lock_acquired=1
# Repeat collision checks while holding the operation lock.
[[ ! -e "$destination" && ! -L "$destination" && ! -e "$plist" && ! -L "$plist" ]] || installer::error 'Installation paths changed while waiting for the operation lock'
stage=$(/usr/bin/mktemp -d /Library/PrivilegedHelperTools/.io.galaxybridge.XXXXXXXX)
installer::bundle::copy bin/galaxybridge "$stage/galaxybridge" 755
installer::bundle::copy uninstall.sh "$stage/uninstall.sh" 755
installer::bundle::copy libexec/identity.sh "$stage/identity.sh" 644
[[ $("$tool_lipo" -archs "$stage/galaxybridge") == arm64 ]] || installer::error 'Release binary must be native arm64'
/usr/bin/codesign --verify --strict "$stage/galaxybridge" || installer::error 'Binary code signature validation failed'
/usr/bin/codesign --display --entitlements :- "$stage/galaxybridge" > "$stage/supervisor-entitlements.plist" 2>/dev/null
if [[ -s "$stage/supervisor-entitlements.plist" ]]; then
    [[ $(/usr/bin/plutil -convert json -o - "$stage/supervisor-entitlements.plist") == '{}' ]] || installer::error 'Supervisor must not carry application entitlements'
fi
# Reject a binary that would load a user-writable library as root.
"$tool_otool" -L "$stage/galaxybridge" > "$stage/libraries.txt"
while IFS= read -r dependency; do
    installer::binary::dependency::check "$dependency"
done < <(/usr/bin/sed '1d;s/^[[:space:]]*//;s/ (compatibility version.*//' "$stage/libraries.txt")
installer::worker::copy
printf 'GalaxyBridge installation format 1\n' > "$stage/INSTALLATION"
/bin/chmod 644 "$stage/INSTALLATION"
# This module is sourced only after its hash, owner, mode and ACLs are checked.
. "$stage/identity.sh"
identity_ready=1
identity_receipt="$stage/IDENTITY"
identity::home::vacancy::check
identity::receipt::create "$identity_receipt"
identity::account::create "$identity_receipt"
identity::home::create "$identity_receipt"

if (( automatic )); then
    {
        /bin/cat <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>io.galaxybridge</string>
<key>ProgramArguments</key><array>
<string>/Library/PrivilegedHelperTools/io.galaxybridge/galaxybridge</string>
<string>daemon</string>
PLIST
        [[ -z "$vendor" ]] || printf '<string>--vendor</string><string>%s</string>\n' "$vendor"
        [[ -z "$product" ]] || printf '<string>--product</string><string>%s</string>\n' "$product"
        [[ -z "$serial" ]] || printf '<string>--serial</string><string>%s</string>\n' "$serial"
        /bin/cat <<'PLIST'
</array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>10</integer>
<key>ExitTimeOut</key><integer>15</integer>
<key>ProcessType</key><string>Background</string>
<key>Umask</key><integer>63</integer>
<key>WorkingDirectory</key><string>/</string>
<key>SoftResourceLimits</key><dict><key>Core</key><integer>0</integer></dict>
<key>HardResourceLimits</key><dict><key>Core</key><integer>0</integer></dict>
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
/bin/mv "$stage/galaxybridge" "$stage/USBWorker.app" "$stage/uninstall.sh" "$stage/identity.sh" "$stage/IDENTITY" "$stage/INSTALLATION" "$destination/"
identity_receipt="$destination/IDENTITY"

# A convenience link is optional; the service always uses the immutable path.
if [[ ! -e "$link" && ! -L "$link" && -d /usr/local/bin ]] && (installer::path::check /usr/local/bin) 2>/dev/null; then
    /bin/ln -s "$destination/galaxybridge" "$link"
    link_created=1
else
    printf 'No convenience link created; use %s/galaxybridge.\n' "$destination"
fi
if (( automatic )); then
    # Root-only parent and prior absence validation prevent an unprivileged race.
    installer::file::copy "$stage/service.plist" "$plist" 600
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
