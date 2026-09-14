#!/bin/bash
# Shared installer/uninstaller account lifecycle. Source only after validating
# the immutable root-owned copy. This file never runs account actions on load.
readonly identity_name=_galaxybridge
readonly identity_user=/Users/_galaxybridge
readonly identity_group=/Groups/_galaxybridge
readonly identity_data=/private/var/db/io.galaxybridge
readonly identity_home=/private/var/db/io.galaxybridge/worker
identity_home_created=0
identity_home_directory_created=0

identity::error() { printf 'GalaxyBridge identity: %s\n' "$*" >&2; return 1; }

identity::command::run() {
    # Directory lookups can block on unavailable directory services. exec keeps
    # the alarm attached to the actual command, with no background child left.
    /usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C \
        /usr/bin/perl -e 'alarm 8; exec @ARGV or die "exec failed: $!\n"' -- "$@"
}

identity::record::names::get() {
    identity::command::run /usr/bin/dscl /Local/Default -list "$1"
}

identity::record::exists() {
    local kind=$1 names
    names=$(identity::record::names::get "$kind") || return 2
    /usr/bin/grep -Fxq "$identity_name" <<< "$names"
}

identity::record::attribute::get() {
    local record=$1 attribute=$2 value
    value=$(identity::command::run /usr/bin/dscl /Local/Default -read "$record" "$attribute" 2>/dev/null) || return 1
    [[ "$value" == "$attribute: "* && "$value" != *$'\n'* ]] || return 1
    printf '%s' "${value#*: }"
}

identity::record::attribute::check() {
    local value
    value=$(identity::record::attribute::get "$1" "$2") || { identity::error "Cannot read required attribute $1 $2"; return 1; }
    [[ "$value" == "$3" ]] || { identity::error "Ownership or account settings changed: $1 $2"; return 1; }
}

identity::receipt::load() {
    local file=$1 first uid_line gid_line user_line group_line extra count forbidden
    [[ -f "$file" && ! -L "$file" ]] || { identity::error 'Missing or symlinked IDENTITY receipt'; return 1; }
    count=$(/usr/bin/wc -c < "$file") || return 1
    (( count <= 1024 )) || { identity::error 'Oversized IDENTITY receipt'; return 1; }
    forbidden=$(/usr/bin/tr -d 'A-Za-z0-9_=\n-' < "$file" | /usr/bin/wc -c) || return 1
    (( forbidden == 0 )) || { identity::error 'Non-ASCII or invalid bytes in IDENTITY receipt'; return 1; }
    {
        IFS= read -r first && IFS= read -r uid_line && IFS= read -r gid_line &&
        IFS= read -r user_line && IFS= read -r group_line && ! IFS= read -r extra && [[ -z "$extra" ]]
    } < "$file" || { identity::error 'IDENTITY receipt must have exactly five newline-terminated lines'; return 1; }
    [[ "$first" == version=1 && "$uid_line" =~ ^uid=6[0-4][0-9][0-9][0-9]$ && "$gid_line" =~ ^gid=6[0-4][0-9][0-9][0-9]$ ]] || { identity::error 'Invalid IDENTITY version or numeric IDs'; return 1; }
    [[ "$user_line" =~ ^user_guid=[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$ && "$group_line" =~ ^group_guid=[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$ ]] || { identity::error 'Invalid IDENTITY UUIDs'; return 1; }
    identity_uid=${uid_line#uid=}
    identity_gid=${gid_line#gid=}
    identity_user_guid=${user_line#user_guid=}
    identity_group_guid=${group_line#group_guid=}
    [[ "$identity_uid" == "$identity_gid" && "$identity_user_guid" != "$identity_group_guid" ]] || { identity::error 'Invalid IDENTITY ID relationship'; return 1; }
}

identity::names::check() {
    local names lookup kind
    for kind in /Users /Groups; do
        names=$(identity::record::names::get "$kind") || { identity::error "Cannot enumerate local $kind"; return 1; }
        if /usr/bin/grep -Fxq "$identity_name" <<< "$names"; then
            identity::error "Pre-existing $kind/$identity_name; refusing to reuse or change it"
            return 1
        fi
    done
    for kind in user group; do
        lookup=$(identity::command::run /usr/bin/dscacheutil -q "$kind" -a name "$identity_name") || { identity::error 'Cannot check directory account name collisions'; return 1; }
        [[ -z "$lookup" ]] || { identity::error "Directory $kind name already exists: $identity_name"; return 1; }
    done
}

identity::number::check() {
    local candidate=$1 users groups lookup
    users=$(identity::command::run /usr/bin/dscl /Local/Default -list /Users UniqueID) || return 2
    groups=$(identity::command::run /usr/bin/dscl /Local/Default -list /Groups PrimaryGroupID) || return 2
    if /usr/bin/awk -v id="$candidate" '$NF == id { found=1 } END { exit !found }' <<< "$users"; then return 1; fi
    if /usr/bin/awk -v id="$candidate" '$NF == id { found=1 } END { exit !found }' <<< "$groups"; then return 1; fi
    lookup=$(identity::command::run /usr/bin/dscacheutil -q user -a uid "$candidate") || return 2
    [[ -z "$lookup" ]] || return 1
    lookup=$(identity::command::run /usr/bin/dscacheutil -q group -a gid "$candidate") || return 2
    [[ -z "$lookup" ]] || return 1
}

identity::receipt::create() {
    local file=$1 candidate status user_guid group_guid
    identity::names::check || return 1
    for (( candidate=60000; candidate<65000; candidate++ )); do
        if identity::number::check "$candidate"; then break; else status=$?; fi
        (( status == 1 )) || { identity::error 'Directory service unavailable while checking numeric IDs'; return 1; }
    done
    (( candidate < 65000 )) || { identity::error 'No unused worker UID/GID in range 60000-64999'; return 1; }
    user_guid=$(/usr/bin/uuidgen) || return 1
    group_guid=$(/usr/bin/uuidgen) || return 1
    # The containing staging directory is root-private. Noclobber also rejects
    # a pre-existing receipt, including a symlink.
    (
        umask 077
        set -o noclobber
        printf 'version=1\nuid=%s\ngid=%s\nuser_guid=%s\ngroup_guid=%s\n' "$candidate" "$candidate" "$user_guid" "$group_guid" > "$file"
    ) || return 1
    identity::receipt::load "$file"
}

identity::account::create() {
    identity::receipt::load "$1" || return 1
    identity::names::check || return 1
    identity::number::check "$identity_uid" || { identity::error 'Chosen worker UID/GID is no longer free'; return 1; }
    # Each record receives its ownership nonce in the very first mutation.
    # Numeric user identity is deliberately assigned last, after login is disabled.
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_group" GeneratedUID "$identity_group_guid" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_group" Password '*' || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_group" PrimaryGroupID "$identity_gid" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_group" GroupMembership "$identity_name" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_group" GroupMembers "$identity_user_guid" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" GeneratedUID "$identity_user_guid" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" Password '*' || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" AuthenticationAuthority ';DisabledUser;' || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" UserShell /usr/bin/false || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" NFSHomeDirectory "$identity_home" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" IsHidden 1 || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" RealName 'GalaxyBridge USB worker' || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" PrimaryGroupID "$identity_gid" || return 1
    identity::command::run /usr/bin/dscl /Local/Default -create "$identity_user" UniqueID "$identity_uid" || return 1
    identity::account::check "$1"
}

identity::account::user::check() {
    identity::record::attribute::check "$identity_user" GeneratedUID "$identity_user_guid" &&
    identity::record::attribute::check "$identity_user" UniqueID "$identity_uid" &&
    identity::record::attribute::check "$identity_user" PrimaryGroupID "$identity_gid" &&
    identity::record::attribute::check "$identity_user" Password '*' &&
    identity::record::attribute::check "$identity_user" AuthenticationAuthority ';DisabledUser;' &&
    identity::record::attribute::check "$identity_user" UserShell /usr/bin/false &&
    identity::record::attribute::check "$identity_user" NFSHomeDirectory "$identity_home" &&
    identity::record::attribute::check "$identity_user" IsHidden 1
}

identity::account::group::check() {
    identity::record::attribute::check "$identity_group" GeneratedUID "$identity_group_guid" &&
    identity::record::attribute::check "$identity_group" PrimaryGroupID "$identity_gid" &&
    identity::record::attribute::check "$identity_group" Password '*' &&
    identity::record::attribute::check "$identity_group" GroupMembership "$identity_name" &&
    identity::record::attribute::check "$identity_group" GroupMembers "$identity_user_guid"
}

identity::account::check() {
    local uid gid records
    identity::receipt::load "$1" || return 1
    identity::account::user::check && identity::account::group::check || return 1
    uid=$(identity::command::run /usr/bin/id -u "$identity_name") || return 1
    gid=$(identity::command::run /usr/bin/id -g "$identity_name") || return 1
    [[ "$uid" == "$identity_uid" && "$gid" == "$identity_gid" ]] || { identity::error 'Directory/NSS account IDs do not match the installation receipt'; return 1; }
    records=$(identity::command::run /usr/bin/dscl /Local/Default -list /Users UniqueID) || return 1
    /usr/bin/awk -v id="$identity_uid" -v name="$identity_name" '$NF == id { count++; if ($1 == name) own=1 } END { exit !(count == 1 && own) }' <<< "$records" || { identity::error 'Worker numeric UID is not exclusive to its local account'; return 1; }
    records=$(identity::command::run /usr/bin/dscl /Local/Default -list /Groups PrimaryGroupID) || return 1
    /usr/bin/awk -v id="$identity_gid" -v name="$identity_name" '$NF == id { count++; if ($1 == name) own=1 } END { exit !(count == 1 && own) }' <<< "$records" || { identity::error 'Worker numeric GID is not exclusive to its local group'; return 1; }
}

identity::process::check() {
    local status
    if identity::command::run /usr/bin/pgrep -u "$identity_uid" >/dev/null; then
        identity::error 'Worker identity still has running processes; refusing to remove its account'
        return 1
    else
        status=$?
    fi
    (( status == 1 )) || { identity::error 'Unable to inspect worker processes'; return 1; }
}

identity::account::ownership::check() {
    local receipt=$1 partial=${2:-0} kind record expected status exists
    identity::receipt::load "$receipt" || return 1
    identity::process::check || return 1
    # Validate EVERY existing record before removing either. During rollback a
    # failed create may have only the nonce; uninstall requires complete settings.
    for kind in /Users /Groups; do
        if identity::record::exists "$kind"; then exists=1; else status=$?; exists=0; fi
        if (( ! exists )); then
            (( status == 1 )) || { identity::error 'Cannot inspect account records'; return 1; }
            continue
        fi
        record="$kind/$identity_name"
        if [[ "$kind" == /Users ]]; then expected=$identity_user_guid; else expected=$identity_group_guid; fi
        identity::record::attribute::check "$record" GeneratedUID "$expected" || return 1
        if (( ! partial )); then
            if [[ "$kind" == /Users ]]; then identity::account::user::check; else identity::account::group::check; fi || return 1
        fi
    done
}

identity::account::delete() {
    local receipt=$1 partial=${2:-0} kind record expected status exists
    identity::account::ownership::check "$receipt" "$partial" || return 1
    for kind in /Users /Groups; do
        if identity::record::exists "$kind"; then exists=1; else status=$?; exists=0; fi
        if (( ! exists )); then
            (( status == 1 )) || return 1
            continue
        fi
        record="$kind/$identity_name"
        if [[ "$kind" == /Users ]]; then expected=$identity_user_guid; else expected=$identity_group_guid; fi
        # Re-read the ownership nonce immediately before each deletion.
        identity::record::attribute::check "$record" GeneratedUID "$expected" || return 1
        identity::command::run /usr/bin/dscl /Local/Default -delete "$record" || return 1
        if identity::record::exists "$kind"; then
            identity::error "Directory record remains after deletion: $record"
            return 1
        else
            status=$?
        fi
        (( status == 1 )) || { identity::error 'Cannot verify directory record removal'; return 1; }
    done
}

identity::path::root::check() {
    local path=$1 mode
    while :; do
        [[ -d "$path" && ! -L "$path" && $(/usr/bin/stat -f %u "$path") == 0 ]] || { identity::error "Untrusted data directory: $path"; return 1; }
        mode=$(/usr/bin/stat -f %Lp "$path") || return 1
        (( (8#$mode & 0022) == 0 )) || { identity::error "Writable data directory: $path"; return 1; }
        if /bin/ls -lde "$path" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
            identity::error "Data directory has an ACL grant: $path"
            return 1
        fi
        [[ "$path" == / ]] && break
        path=$(/usr/bin/dirname "$path")
    done
}

identity::home::vacancy::check() {
    identity::path::root::check /private/var/db || return 1
    [[ ! -e "$identity_data" && ! -L "$identity_data" ]] || { identity::error "Pre-existing worker data path; refusing to reuse it: $identity_data"; return 1; }
}

identity::home::create() {
    local receipt=$1
    identity::account::check "$receipt" || return 1
    identity::home::vacancy::check || return 1
    /bin/mkdir -m 755 "$identity_data" || return 1
    identity_home_created=1
    /usr/bin/install -f '' -o root -g wheel -m 600 "$receipt" "$identity_data/IDENTITY" || return 1
    /bin/chmod -N "$identity_data/IDENTITY" || return 1
    /bin/mkdir -m 700 "$identity_home" || return 1
    identity_home_directory_created=1
    /usr/sbin/chown "$identity_uid:$identity_gid" "$identity_home" || return 1
    /bin/chmod -N "$identity_home" || return 1
    identity::home::parent::check "$receipt" && identity::home::directory::check
}

identity::home::parent::check() {
    local receipt=$1 marker="$identity_data/IDENTITY" entry
    identity::receipt::load "$receipt" || return 1
    identity::path::root::check "$identity_data" || return 1
    [[ -f "$marker" && ! -L "$marker" && $(/usr/bin/stat -f '%u:%g:%Lp:%l' "$marker") == 0:0:600:1 ]] || { identity::error 'Untrusted worker-home ownership receipt'; return 1; }
    if /bin/ls -le "$marker" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
        identity::error 'Worker-home receipt has an ACL grant'
        return 1
    fi
    /usr/bin/cmp -s "$receipt" "$marker" || { identity::error 'Worker-home receipt does not match this installation'; return 1; }
    # Nothing outside these two owned names is ever removed.
    for entry in "$identity_data"/* "$identity_data"/.[!.]* "$identity_data"/..?*; do
        [[ -e "$entry" || -L "$entry" ]] || continue
        case "${entry##*/}" in IDENTITY|worker) ;; *) identity::error "Unexpected worker-data entry: $entry"; return 1 ;; esac
    done
}

identity::home::directory::check() {
    local mounts home_device parent_device
    [[ -d "$identity_home" && ! -L "$identity_home" && $(/usr/bin/stat -f '%u:%g:%Lp' "$identity_home") == "$identity_uid:$identity_gid:700" ]] || { identity::error 'Worker home owner or permissions changed'; return 1; }
    home_device=$(/usr/bin/stat -f %d "$identity_home") || return 1
    parent_device=$(/usr/bin/stat -f %d "$identity_data") || return 1
    [[ "$home_device" =~ ^[0-9]+$ && "$parent_device" =~ ^[0-9]+$ && "$home_device" == "$parent_device" ]] || { identity::error 'Worker home is a different filesystem; refusing deletion'; return 1; }
    mounts=$(identity::command::run /sbin/mount) || { identity::error 'Cannot inspect worker-home mount status'; return 1; }
    [[ -n "$mounts" ]] || { identity::error 'Empty mount inventory; refusing worker-home deletion'; return 1; }
    if /usr/bin/grep -Fq -e " on $identity_home (" -e " on ${identity_home#/private} (" <<< "$mounts"; then
        identity::error 'Worker home is a mount point; refusing deletion'
        return 1
    fi
    if /bin/ls -lde "$identity_home" | /usr/bin/sed '1d' | /usr/bin/grep -q ' allow '; then
        identity::error 'Worker home has an ACL grant'
        return 1
    fi
}

identity::home::tree::delete() {
    # Caller proves ownership and quiescence first. Physical traversal unlinks
    # symlinks, never their targets; -x does not cross mounted filesystems. Do
    # not chmod/chflags descendants, which could be hard links outside the home.
    /usr/bin/find -P -x "$1" -depth -delete || return 1
    # BSD find can report success even when -delete prints an unlink error.
    # Account removal requires evidence that the entire owned home is gone.
    [[ ! -e "$1" && ! -L "$1" ]] || { identity::error 'Worker home remains after physical deletion'; return 1; }
}

identity::home::directory::delete() {
    local receipt=$1 partial=${2:-0}
    if (( partial && ! identity_home_created )); then return 0; fi
    if [[ ! -e "$identity_data" && ! -L "$identity_data" ]]; then return 0; fi
    if (( partial && identity_home_created )) && [[ ! -e "$identity_data/IDENTITY" && ! -L "$identity_data/IDENTITY" ]]; then
        # The only safe rollback without a persisted nonce is an empty directory
        # this invocation successfully reserved itself.
        /bin/rmdir "$identity_data"
        return
    fi
    identity::home::parent::check "$receipt" || return 1
    identity::process::check || return 1
    if [[ ! -e "$identity_home" && ! -L "$identity_home" ]]; then return 0; fi
    if (( partial && identity_home_directory_created )) && [[ ! -L "$identity_home" && $(/usr/bin/stat -f %u "$identity_home") == 0 ]]; then
        # Failed chown before the worker ever ran: remove only an empty home.
        /bin/rmdir "$identity_home"
        return
    fi
    identity::home::directory::check || return 1
    # Full records are mandatory whenever data exists, even on retries. Missing
    # records are accepted only after the home is gone (or was never created).
    identity::account::check "$receipt" || return 1
    identity::process::check || return 1
    identity::home::tree::delete "$identity_home" || { identity::error 'Worker-home deletion failed; account and ownership receipts are retained'; return 1; }
}

identity::home::parent::delete() {
    local receipt=$1 partial=${2:-0} kind status
    if (( partial && ! identity_home_created )); then return 0; fi
    if [[ ! -e "$identity_data" && ! -L "$identity_data" ]]; then return 0; fi
    identity::home::parent::check "$receipt" || return 1
    [[ ! -e "$identity_home" && ! -L "$identity_home" ]] || { identity::error 'Worker home still exists; retaining parent and receipt'; return 1; }
    for kind in /Users /Groups; do
        if identity::record::exists "$kind"; then
            identity::error 'Worker account record still exists; retaining ownership receipt'
            return 1
        else
            status=$?
        fi
        (( status == 1 )) || { identity::error 'Cannot prove worker account records are absent'; return 1; }
    done
    /bin/rm "$identity_data/IDENTITY" && /bin/rmdir "$identity_data"
}

identity::installation::delete() {
    # Bind the directory to current account GUIDs before touching home contents,
    # then recheck records immediately before deleting the numeric identities.
    identity::account::ownership::check "$1" "${2:-0}" &&
    identity::home::directory::delete "$1" "${2:-0}" &&
    identity::account::delete "$1" "${2:-0}" &&
    identity::home::parent::delete "$1" "${2:-0}"
}
