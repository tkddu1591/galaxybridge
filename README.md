# GalaxyBridge

**Your Galaxy. Your Mac. One USB cable.**

An independent Android USB tethering driver for Apple Silicon, with optional automatic reconnect. Written in Rust. No TetherKit, HoRNDIS, libusb, kernel extension, or browser proxy.

[한국어](README.ko.md) · [Security model](SECURITY.md) · [Architecture](docs/architecture.md)

> Early experimental software. The binary targets macOS 13.3+, but a deployment target is not a hardware compatibility certificate. See the validation matrix below before installing.

## What it does

- Uses the phone's RNDIS USB network interface as an Ethernet connection on macOS.
- Discovers Samsung devices by vendor **and RNDIS interface descriptors**, rather than one Galaxy model's product ID.
- Keeps USB protocol parsing in an unprivileged `nobody` worker. A separate root supervisor owns network setup and BPF.
- Supports manual sessions or an optional `launchd` service that waits for the phone and reconnects after USB loss.
- Changes the preferred **IPv4** route after DHCP succeeds, unless IPv4 `utun` routes are present. It does not promise VPN, IPv6, or DNS leak prevention.

Only one matching phone is selected. Multiple matching phones cause a refusal rather than an arbitrary choice. Optional product/serial filters reduce accidental selection; they are not device authentication.

When USB disconnects, the owned DHCP/DNS service and interfaces are removed before fallback routing is checked. A still-active previous Wi-Fi/Ethernet service uses its current gateway, rather than an expired saved address. Existing TCP connections may need to reconnect when the public IP changes; this is not seamless connection migration.

## Install once

The installer uses Apple's `lipo` and `otool` to verify the release binary. Install **Apple Command Line Tools once** with `xcode-select --install` if they are missing; a configured full Xcode installation also supplies them. This prerequisite is for installer verification only. Running GalaxyBridge requires no Command Line Tools, Rust, or Homebrew.

Download the arm64 archive and checksum from [Releases](https://github.com/tkddu1591/galaxybridge/releases). Verify the archive before extracting it, review the included installer, then run it from the extracted folder:

```sh
./install.sh
```

This installs the command only. Start a manual session after enabling **USB tethering** on the phone:

```sh
galaxybridge devices
sudo galaxybridge connect
```

Leave that terminal running. `Ctrl+C` stops the session and cleans up the interfaces it created.

If the installer cannot safely create the convenience command, use `/Library/PrivilegedHelperTools/io.galaxybridge/galaxybridge` in place of `galaxybridge`.

For optional automatic reconnect, install with:

```sh
./install.sh --auto
```

Installation asks for macOS administrator authentication. Normal automatic connections do not. The release binary needs no Rust, Homebrew, or separate USB driver installation. Upgrades deliberately require uninstalling the previous GalaxyBridge installation first.

To select a particular device:

```sh
galaxybridge devices --show-serial
./install.sh --auto --serial YOUR_PHONE_SERIAL
# Or, less specifically:
./install.sh --auto --product 6863
```

The examples are alternatives, not sequential installs. Do not run another RNDIS driver alongside GalaxyBridge.

## Optional: enable tethering just by plugging in

On Galaxy devices that expose the setting, select **Settings → Developer options → Default USB configuration → USB tethering** once. USB debugging is unnecessary. Mobile data must be available; some devices require unlocking before USB data access.

The phone setting and the Mac's `--auto` service are independent. Without the phone setting, turn USB tethering on manually after connecting. Without `--auto`, start `galaxybridge connect` manually. One UI version, carrier policy, USB restrictions, and device firmware can change whether the phone setting exists or takes effect.

## Stop or remove

Pause the automatic service until restart or manual resume:

```sh
sudo launchctl bootout system/io.galaxybridge
```

Resume it:

```sh
sudo launchctl bootstrap system /Library/LaunchDaemons/io.galaxybridge.plist
```

Uninstall:

```sh
sudo /Library/PrivilegedHelperTools/io.galaxybridge/uninstall.sh
```

The installer keeps the binary in a root-owned location, validates checksums and runtime library paths, and refuses unrelated existing paths. The uninstaller removes only its own installation. To diagnose a failure, pause automatic mode and run `sudo galaxybridge connect` in a terminal; diagnostics go to standard error.

## Security, plainly

This is **not certified or independently audited security software**. Rust bounds checks and process separation reduce some risks; they do not make an untrusted USB device safe. The phone supplies Ethernet/DHCP traffic to macOS and can become the internet gateway. `nobody` is a restricted Unix identity, not a macOS sandbox. The supervisor still runs as root.

No SIP changes, Reduced Security mode, USB debugging, packet-content logging, telemetry service, or automatic code downloads are required. Archive hashes and ad-hoc signatures check integrity, **not publisher identity**. See [SECURITY.md](SECURITY.md) for the full trust boundary and reporting process.

## Compatibility and validation

| Area | Status |
| --- | --- |
| Apple Silicon | Native arm64 build; targets M-series Macs |
| macOS deployment target | 13.3, checked in the Mach-O output |
| Phone protocol | Samsung RNDIS descriptors; NCM/ECM are outside this implementation |
| Physical test device | M3 Pro / macOS 26.5.1 / Galaxy S25 Ultra |
| USB/RNDIS initialization | Passed without root USB parsing |
| End-to-end independent-driver internet | DHCP, preferred IPv4 route, HTTPS and Google HTTP 204 passed on the physical test device |
| Other M-series / Galaxy combinations | Not physically verified; please report exact model/OS/protocol |
| Reboot, sleep/wake, Wi-Fi disabled | Separate validation required |

The private macOS `feth` interface is an explicit compatibility risk. If relevant kernel settings differ from the expected defaults, GalaxyBridge refuses to silently change global settings.

## Build and test

Requires Rust 1.85+ with edition 2024 support and Xcode Command Line Tools. Development was tested with Rust 1.89.0.

```sh
MACOSX_DEPLOYMENT_TARGET=13.3 cargo test --locked
MACOSX_DEPLOYMENT_TARGET=13.3 cargo clippy --all-targets --locked -- -D warnings
MACOSX_DEPLOYMENT_TARGET=13.3 cargo build --release --locked
python3 scripts/check-installer.py
./scripts/build-release.sh
```

`Cargo.lock` pins the resolved dependency graph. `nusb` accesses IOKit directly; the application does not download or link TetherKit or libusb. Release bundles contain dependency license notices and checksums.

## Implementation sources

The protocol implementation is original code based on [Microsoft's RNDIS specifications](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-messaging). macOS integration uses Apple's system tools and BPF headers; [Apple's `feth` implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/net/if_fake.c) documents the private interface behavior. USB transport is provided by [nusb](https://github.com/kevinmehall/nusb).

MIT licensed. No affiliation with Samsung or Apple.
