# GalaxyBridge

**Your Android. Your Mac. One USB cable.**

An independent Android USB tethering driver for Apple Silicon, with optional automatic reconnect. Written in Rust. No TetherKit, HoRNDIS, libusb, kernel extension, or browser proxy.

[한국어](README.ko.md) · [Security model](SECURITY.md) · [Architecture](docs/architecture.md)

> Early experimental software. The binary targets macOS 13.3+, but a deployment target is not a hardware compatibility certificate. See the validation matrix below before installing.

## What it does

- Uses the phone's RNDIS USB network interface as an Ethernet connection on macOS.
- Discovers RNDIS devices by their USB interface descriptors, regardless of manufacturer. Optional vendor, product and serial filters narrow selection; they are not authentication.
- Keeps USB protocol parsing in a separately signed App Sandbox worker under a dedicated, disabled `_galaxybridge` service account. A separate root supervisor owns network setup and BPF.
- Supports manual sessions or an optional `launchd` service that waits for the phone and reconnects after USB loss.
- Changes the preferred **IPv4** route after DHCP succeeds, unless IPv4 `utun` routes are present. It does not promise VPN, IPv6, or DNS leak prevention.

Only one matching phone is selected. Multiple matching phones cause a refusal rather than an arbitrary choice. Optional product/serial filters reduce accidental selection; they are not device authentication.

When USB disconnects, the owned DHCP/DNS service and interfaces are removed before fallback routing is checked. A still-active previous Wi-Fi/Ethernet service uses its current gateway, rather than an expired saved address. Existing TCP connections may need to reconnect when the public IP changes; this is not seamless connection migration.

## Install once

> **0.2.0 release preparation is in progress; public installation is not available yet.** Physical transfer, reconnect and automatic Wi-Fi recovery tests have passed. The final normal-service check and release publication are being completed. The command stops without administrator authentication if the verified release has not been published.

Copy this **one command** into Terminal on an Apple Silicon Mac:

```sh
/bin/bash -c 'setup=$(/usr/bin/curl --disable -fsS --proto "=https" https://raw.githubusercontent.com/tkddu1591/galaxybridge/main/setup.sh) && /bin/bash -c "$setup"'
```

The setup downloads a fixed release, verifies its pinned SHA-256 before extraction, and asks for administrator authentication to install **automatic reconnect**. Afterward, connect your phone and enable **USB tethering**. Normal connections do not need a terminal or another administrator password.

If Apple Command Line Tools are missing, complete the Apple installation dialog that opens. Setup waits for it to finish; if it times out, finish the Apple installation and run the same command again. These tools are used for installation verification; Rust and Homebrew are unnecessary.

The command checks that the bootstrap download succeeded before running it. The bootstrap performs no privileged downloads and delegates installation to the same offline installer. You trust this GitHub repository and HTTPS delivery; a checksum or ad-hoc signature does not establish a separate publisher identity. You can [read setup.sh](https://github.com/tkddu1591/galaxybridge/blob/main/setup.sh) before running it.

An existing installation is preserved. This first-install command does not silently remove it or migrate old versions: use its documented uninstaller before replacing it.

<details>
<summary>Manual sessions, device filters, and offline installation</summary>

Download the archive and checksum from [Releases](https://github.com/tkddu1591/galaxybridge/releases), verify the archive, and extract it into a folder owned by your account. Run `./install.sh` for manual sessions, or `./install.sh --auto` for automatic reconnect. `setup.sh --manual` also downloads and installs without a background service.

For a manual session, enable USB tethering on the phone, then run:

```sh
galaxybridge devices
sudo galaxybridge connect
```

Keep that terminal open; `Ctrl+C` stops the session. If the convenience command cannot be created safely, use `/Library/PrivilegedHelperTools/io.galaxybridge/galaxybridge`.

To select a specific phone during offline installation:

```sh
galaxybridge devices --show-serial
./install.sh --auto --serial YOUR_PHONE_SERIAL
```

Alternatively use `--vendor 04e8 --product 6863`. These are selection hints, not authentication. Do not run another RNDIS driver alongside GalaxyBridge.

</details>

## Optional: enable tethering just by plugging in

On Android devices that expose the setting, select **Settings → Developer options → Default USB configuration → USB tethering** once. USB debugging is unnecessary. Mobile data must be available; some devices require unlocking before USB data access.

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

This is **not certified or independently audited security software**. Rust bounds checks and process separation reduce some risks; they do not make an untrusted USB device safe. The phone supplies Ethernet/DHCP traffic to macOS and can become the internet gateway. The worker uses App Sandbox and a dedicated service account. Its allowed USB and IPC channels remain powerful network capabilities; the sandbox does not make arbitrary USB devices trustworthy. The supervisor still runs as root.

No SIP changes, Reduced Security mode, USB debugging, packet-content logging, telemetry service, or background code downloads are required. Only the explicitly invoked setup command downloads a release. Archive hashes and ad-hoc signatures check integrity, **not publisher identity**. See [SECURITY.md](SECURITY.md) for the full trust boundary and reporting process.

## Compatibility and validation

| Area | Status |
| --- | --- |
| Apple Silicon | Native arm64 build; targets M-series Macs |
| macOS deployment target | 13.3, checked in the Mach-O output |
| Phone protocol | Manufacturer-neutral RNDIS selection with strict configuration and endpoint validation |
| NCM/ECM | Separate USB networking protocols; GalaxyBridge does not claim these interfaces. Check macOS Network settings for a native USB network service |
| Physical test device | M3 Pro / macOS 26.5.1 / Galaxy S25 Ultra |
| USB/RNDIS initialization | v0.2.0 passed under the dedicated account and App Sandbox |
| End-to-end independent-driver internet | v0.2.0: two 180-second sessions, 36/36 fresh USB-bound HTTPS requests, plus 8 MiB download and 2 MiB upload passed |
| Automatic Mac-side connection | v0.2.0 automatic connection and physical reconnect passed |
| USB unplug with Wi-Fi left enabled | v0.2.0: two trials; Wi-Fi route in 0.38–0.60 s and fresh HTTPS in 0.72–0.90 s after removal detection, with no Wi-Fi toggle |
| Other M-series / Android combinations | Not physically verified; please report exact model/OS/protocol |
| Reboot, sleep/wake, Wi-Fi disabled | Separate validation required |

See [the physical validation report](docs/validation-0.2.0.md) for timing methods, fingerprints and test limits.

The private macOS `feth` interface is an explicit compatibility risk. If relevant kernel settings differ from the expected defaults, GalaxyBridge refuses to silently change global settings.

## Build and test

Requires Rust 1.85+ with edition 2024 support and Xcode Command Line Tools. Development was tested with Rust 1.89.0.

```sh
MACOSX_DEPLOYMENT_TARGET=13.3 cargo test --locked
MACOSX_DEPLOYMENT_TARGET=13.3 cargo clippy --all-targets --locked -- -D warnings
MACOSX_DEPLOYMENT_TARGET=13.3 cargo build --release --locked
python3 scripts/check-setup.py
python3 scripts/check-installer.py
./scripts/build-release.sh
```

`Cargo.lock` pins the resolved dependency graph. `nusb` accesses IOKit directly; the application does not download or link TetherKit or libusb. Release bundles contain dependency license notices and checksums.

## Implementation sources

The protocol implementation is original code based on [Microsoft's RNDIS specifications](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-messaging). macOS integration uses Apple's system tools and BPF headers; [Apple's `feth` implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/net/if_fake.c) documents the private interface behavior. USB transport is provided by [nusb](https://github.com/kevinmehall/nusb).

MIT licensed. No affiliation with Android device manufacturers or Apple.
