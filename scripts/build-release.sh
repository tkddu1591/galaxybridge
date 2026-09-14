#!/bin/bash
# Build locally as an ordinary user; release installation never downloads code.
set -euo pipefail
umask 022
release::binary::dependency::check() {
    local dependency=$1
    case "$dependency" in
        */../*|*/./*|*/..|*/.|*//*|*[[:space:]]*) printf 'Unclean dynamic dependency path: %s\n' "$dependency" >&2; exit 1 ;;
    esac
    case "$dependency" in
        /usr/lib/*|/System/Library/*) ;;
        *) printf 'Non-system runtime dependency: %s\n' "$dependency" >&2; exit 1 ;;
    esac
}

[[ $(uname -s) == Darwin ]] || { printf 'Build on macOS with Xcode Command Line Tools.\n' >&2; exit 1; }
(( EUID != 0 )) || { printf 'Do not build releases as root.\n' >&2; exit 1; }
repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$repository"
export MACOSX_DEPLOYMENT_TARGET=13.3
# Strip does not remove Rust diagnostic source paths. Keep local usernames and
# checkout/Cargo locations out of public binaries. These release flags override
# inherited Rust flags so package provenance does not depend on a shell profile.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=${HOME}=/build-user"$'\x1f'"--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"$'\x1f'"--remap-path-prefix=$repository=/galaxybridge"
unset RUSTFLAGS
readonly target=aarch64-apple-darwin
readonly cargo_output="$repository/.build/cargo"
for command in cargo rustc python3; do command -v "$command" >/dev/null; done
for name in README.md README.ko.md LICENSE SECURITY.md install.sh uninstall.sh; do
    [[ -f "$name" && ! -L "$name" ]] || { printf 'Missing release input: %s\n' "$name" >&2; exit 1; }
done
[[ -f scripts/identity.sh && ! -L scripts/identity.sh ]] || { printf 'Missing account lifecycle module: scripts/identity.sh\n' >&2; exit 1; }
[[ -f scripts/worker-entitlements.plist && ! -L scripts/worker-entitlements.plist ]] || { printf 'Missing worker entitlements\n' >&2; exit 1; }
[[ -d docs && ! -L docs ]] || { printf 'Missing release documentation directory: docs\n' >&2; exit 1; }
mkdir -p .build dist
staging=$(mktemp -d "$repository/.build/release.XXXXXXXX")
trap 'rm -rf -- "$staging"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cargo build --bins --release --locked --target "$target" --target-dir "$cargo_output"
cargo metadata --locked --format-version 1 --filter-platform "$target" > "$staging/metadata.json"
version=$(python3 - "$staging/metadata.json" <<'PY'
import json, re, sys
metadata = json.load(open(sys.argv[1]))
package = next(p for p in metadata['packages'] if p['id'] == metadata['resolve']['root'])
assert re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?', package['version'])
print(package['version'])
PY
)
readonly name="galaxybridge-$version-macos-arm64"
bundle="$staging/$name"
mkdir -p "$bundle/bin"
mkdir -p "$bundle/libexec"
cp scripts/identity.sh "$bundle/libexec/identity.sh"
chmod 644 "$bundle/libexec/identity.sh"
cp "$cargo_output/$target/release/galaxybridge" "$bundle/bin/galaxybridge"
worker_app="$bundle/libexec/USBWorker.app"
mkdir -p "$worker_app/Contents/MacOS"
cp "$cargo_output/$target/release/galaxybridge-usb" "$worker_app/Contents/MacOS/galaxybridge-usb"
chmod 755 "$worker_app/Contents/MacOS/galaxybridge-usb"
python3 - "$worker_app/Contents/Info.plist" "$version" <<'PY'
import pathlib, plistlib, sys
pathlib.Path(sys.argv[1]).write_bytes(plistlib.dumps({
    'CFBundleIdentifier': 'io.galaxybridge.usb-worker',
    'CFBundleExecutable': 'galaxybridge-usb',
    'CFBundleName': 'GalaxyBridge USB Worker',
    'CFBundlePackageType': 'APPL',
    'CFBundleVersion': sys.argv[2],
    'CFBundleShortVersionString': sys.argv[2],
    'LSMinimumSystemVersion': '13.3',
}))
PY
/usr/bin/codesign --force --sign - --entitlements "$repository/scripts/worker-entitlements.plist" "$worker_app"
/usr/bin/codesign --verify --strict "$worker_app"
/usr/bin/codesign --display --entitlements :- "$worker_app" > "$staging/worker-entitlements.plist" 2>/dev/null
python3 - "$staging/worker-entitlements.plist" <<'PY'
import plistlib, sys
entitlements = plistlib.load(open(sys.argv[1], 'rb'))
if not isinstance(entitlements, dict) or set(entitlements) != {'com.apple.security.app-sandbox', 'com.apple.security.device.usb'} or any(value is not True for value in entitlements.values()):
    raise SystemExit('Worker signed entitlements must contain exactly App Sandbox and USB access')
PY
python3 - "$bundle/bin/galaxybridge" "$worker_app/Contents/MacOS/galaxybridge-usb" "$repository" "$HOME" <<'PY'
import pathlib, sys
for binary_path in sys.argv[1:3]:
    binary = pathlib.Path(binary_path).read_bytes()
    for local_path in sys.argv[3:]:
        if local_path.encode() in binary:
            raise SystemExit('Release binary contains an unremapped local build path')
PY
cp install.sh uninstall.sh README.md README.ko.md LICENSE SECURITY.md "$bundle/"
python3 - "$repository/docs" "$bundle/docs" <<'PY'
import pathlib, shutil, sys
source = pathlib.Path(sys.argv[1])
for path in source.rglob('*'):
    if path.is_symlink():
        raise SystemExit(f'Refusing a symlink in release documentation: {path.relative_to(source)}')
shutil.copytree(source, sys.argv[2])
PY
# Keep the linked security evidence readable in the offline bundle. Copy only
# the reviewed reports, never fuzz logs, generated inputs or build artifacts.
[[ -d fuzz && ! -L fuzz ]] || { printf 'Missing fuzz report directory\n' >&2; exit 1; }
mkdir "$bundle/fuzz"
for report in REPORT.md PACKED-AGGREGATION.md; do
    [[ -f "fuzz/$report" && ! -L "fuzz/$report" ]] || { printf 'Missing or symlinked fuzz report: %s\n' "$report" >&2; exit 1; }
    cp "fuzz/$report" "$bundle/fuzz/$report"
done
chmod 755 "$bundle/bin/galaxybridge" "$bundle/install.sh" "$bundle/uninstall.sh"
/usr/bin/codesign --verify --strict "$bundle/bin/galaxybridge"
/usr/bin/codesign --display --entitlements :- "$bundle/bin/galaxybridge" > "$staging/supervisor-entitlements.plist" 2>/dev/null
if [[ -s "$staging/supervisor-entitlements.plist" ]]; then
    [[ $(/usr/bin/plutil -convert json -o - "$staging/supervisor-entitlements.plist") == '{}' ]] || { printf 'Supervisor must not carry application entitlements\n' >&2; exit 1; }
fi
[[ $(/usr/bin/lipo -archs "$bundle/bin/galaxybridge") == arm64 ]] || { printf 'Unexpected release architecture\n' >&2; exit 1; }
minimum=$(/usr/bin/otool -l "$bundle/bin/galaxybridge" | awk '/LC_BUILD_VERSION/ { build=1; next } build && /minos/ { print $2; exit }')
[[ "$minimum" == 13.3 ]] || { printf 'Unexpected minimum macOS version: %s\n' "$minimum" >&2; exit 1; }
/usr/bin/otool -L "$bundle/bin/galaxybridge" > "$staging/libraries.txt"
while IFS= read -r dependency; do
    release::binary::dependency::check "$dependency"
done < <(sed '1d;s/^[[:space:]]*//;s/ (compatibility version.*//' "$staging/libraries.txt")
[[ $(/usr/bin/lipo -archs "$worker_app/Contents/MacOS/galaxybridge-usb") == arm64 ]] || { printf 'Unexpected worker architecture\n' >&2; exit 1; }
worker_minimum=$(/usr/bin/otool -l "$worker_app/Contents/MacOS/galaxybridge-usb" | awk '/LC_BUILD_VERSION/ { build=1; next } build && /minos/ { print $2; exit }')
[[ "$worker_minimum" == 13.3 ]] || { printf 'Unexpected worker minimum macOS version\n' >&2; exit 1; }
/usr/bin/otool -L "$worker_app/Contents/MacOS/galaxybridge-usb" > "$staging/worker-libraries.txt"
while IFS= read -r dependency; do
    release::binary::dependency::check "$dependency"
done < <(sed '1d;s/^[[:space:]]*//;s/ (compatibility version.*//' "$staging/worker-libraries.txt")

# Include exact locked dependency notices (including build dependencies), without
# publishing Cargo metadata's absolute paths or the builder's environment.
python3 - "$staging/metadata.json" "$bundle" <<'PY'
import json, pathlib, shutil, sys
metadata = json.load(open(sys.argv[1]))
bundle = pathlib.Path(sys.argv[2])
resolved = {node['id'] for node in metadata['resolve']['nodes']}
packages = sorted((p for p in metadata['packages']
                   if p['id'] in resolved and p['id'] != metadata['resolve']['root']),
                  key=lambda p: p['name'])
index = ['# Third-party licenses', '',
         'Locked dependencies used by the macOS release, including build dependencies.', '',
         '| Package | Version | Declared license |', '|---|---|---|']
for package in packages:
    source = pathlib.Path(package['manifest_path']).parent
    files = sorted(p for p in source.iterdir() if p.is_file()
                   and p.name.lower().startswith(('license', 'copying', 'copyright')))
    if not files:
        raise SystemExit(f"Missing license files for {package['name']}")
    destination = bundle / 'licenses' / f"{package['name']}-{package['version']}"
    destination.mkdir(parents=True)
    for file in files:
        shutil.copyfile(file, destination / file.name)
    index.append(f"| {package['name']} | {package['version']} | {package['license']} |")
(bundle / 'THIRD_PARTY_LICENSES.md').write_text('\n'.join(index) + '\n')
PY
{
    printf 'GalaxyBridge %s\nTarget: %s\nMinimum macOS: %s\n' "$version" "$target" "$minimum"
    rustc --version
    cargo --version
    if commit=$(git rev-parse HEAD 2>/dev/null); then
        printf 'Source commit: %s\n' "$commit"
        if [[ -n $(git status --porcelain --untracked-files=normal) ]]; then
            printf 'Source tree: modified (development build)\n'
        else
            printf 'Source tree: clean\n'
        fi
    else
        printf 'Source commit: unavailable (source export or uncommitted development tree)\n'
    fi
    printf '\nRuntime libraries:\n'
    sed '1d' "$staging/libraries.txt"
    printf '\nUSB worker: io.galaxybridge.usb-worker; App Sandbox + USB only\n'
    printf 'Worker minimum macOS: %s\nWorker runtime libraries:\n' "$worker_minimum"
    sed '1d' "$staging/worker-libraries.txt"
} > "$bundle/BUILD-INFO.txt"
python3 - "$bundle" <<'PY'
import hashlib, pathlib, sys
bundle = pathlib.Path(sys.argv[1])
lines = []
for path in sorted(p for p in bundle.rglob('*') if p.is_file()):
    lines.append(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(bundle).as_posix()}')
(bundle / 'SHA256SUMS').write_text('\n'.join(lines) + '\n')
PY
archive="$repository/dist/$name.tar.gz"
[[ ! -e "$archive" && ! -L "$archive" ]] || { printf 'Refusing to overwrite release: %s\n' "$archive" >&2; exit 1; }
[[ ! -e "$archive.sha256" && ! -L "$archive.sha256" ]] || { printf 'Refusing to overwrite checksum: %s.sha256\n' "$archive" >&2; exit 1; }
# These are newly built artifacts. Do not publish the builder's uid/name,
# Finder/FileProvider provenance, ACLs, or BSD flags in archive metadata.
# Download quarantine is still preserved by the installer when users receive it.
COPYFILE_DISABLE=1 tar --no-xattrs --no-acls --no-fflags --no-mac-metadata \
    --uid 0 --gid 0 --uname root --gname wheel \
    -czf "$staging/$name.tar.gz" -C "$staging" "$name"
mv "$staging/$name.tar.gz" "$archive"
(cd dist && /usr/bin/shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256")
printf 'Release bundle: %s\nChecksum: %s.sha256\n' "$archive" "$archive"
