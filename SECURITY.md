# Security model and limitations

GalaxyBridge is an experimental network driver. A code review and passing tests are not a guarantee that it has no vulnerabilities. Do not connect an untrusted USB device simply because it presents a familiar vendor ID or RNDIS interface.

## Trust boundaries

1. **Installer:** `setup.sh` runs as an ordinary user, downloads a fixed HTTPS release with user curl configuration disabled, enforces a kernel file-size limit, and checks a SHA-256 pinned in the bootstrap before extraction. The bootstrap itself is trusted repository code delivered over HTTPS; this is not independent publisher authentication. Download failure stops before Apple tool installation or sudo. The offline installer then handles administrator authorization and performs no downloads or builds as root. It copies a validated bundle into root-owned directories, removes copied ACL grants and BSD flags, and refuses symlink or unrelated destination collisions. Toolchain dispatchers and resolved verification tools must be rooted in non-writable, root-owned paths. Checksums are rechecked in root-private staging. Before sudo, source ownership, permissions, ACLs and ancestors are checked to reject bundles writable by another local user. The optional launchd arguments are constrained and stored in a root-only plist. Uninstallation verifies account identity and quiescence before removing its owned home and account; it must not follow worker-created symlinks into other locations.
2. **Root supervisor:** owns BPF and the two virtual interfaces it creates. System commands use absolute paths, separate argument arrays, a cleared environment, and timeouts. It does not receive or parse RNDIS USB messages. Route changes preserve interface identity and are verified after application; unreadable route state fails closed.
3. **USB worker:** a separate executable inside `USBWorker.app` runs with App Sandbox and the USB-device entitlement. The supervisor validates its fixed root-owned path, code signature, bundle identity and exact entitlement allowlist. Before exec, it clears supplementary groups and permanently changes UID/GID to the installation-owned `_galaxybridge` account. The disabled account and group are tied to a private UUID/UID/GID receipt. Only null/pipe standard streams and the unnamed Unix datagram socket cross exec; terminal and privileged log descriptors are not passed directly to the worker. Diagnostics are bounded and escaped, and the child starts a new session without a controlling terminal. Other descriptors become close-on-exec, and the working directory is `/`. The USB executable also rejects root and requires its own sandbox/USB code-signing entitlements before parsing device data. There is no unsandboxed fallback. Public device inspection also invokes the sandboxed executable, under the inspecting user.
4. **IPC:** only a fixed-size validated unicast MAC address or a bounded Ethernet frame is accepted. The worker cannot ask the supervisor to run commands, read files, bind BPF to another interface, or choose arbitrary interface names.
5. **Phone/network:** the selected device remains a network peer. Its Ethernet and DHCP traffic enters macOS's network stack. It can announce DNS/gateway settings and observe traffic whose encryption does not protect it. VID, PID and serial filters are spoofable identifiers, not cryptographic authentication.

The sandbox has no network-client/server entitlement or broad filesystem exception. It permits the USB APIs and its own container. The dedicated service home is mode 0700 under a root-owned parent. A compromised worker still owns the authorized USB and IPC capabilities: it can send traffic through the phone and inject allowed frames into the dedicated tethering link. Blocking direct network sockets is not a prohibition on all communications. The unsandboxed root supervisor, kernel, IOKit, nusb, native ABI bindings, Rust dependencies, installer and Apple sandbox implementation remain part of the trusted computing base.

## Input and resource controls

- USB control/data transfers are bounded to 16KiB and Ethernet frames to 1,514 bytes.
- RNDIS types, response IDs/statuses, declared message lengths, offsets, and optional metadata are validated using checked slices.
- USB termination padding is separate from RNDIS message length.
- RX transfers and TX submissions are bounded. Backpressure can drop frames instead of allocating an unbounded queue.
- Root commands have bounded execution time; worker shutdown is cooperative with a final forced-termination fallback.
- Device disappearance, malformed data or failed initialization ends the session. Automatic mode retries rather than bypassing validation.
- No shell evaluates data received from a USB device, route command, serial number, or MAC address.
- The root bridge accepts only IPv4 and ARP Ethernet types in either direction; IPv6, VLAN-tagged and other frame types are dropped. This drops direct IPv6 router advertisements on this bridge; it does not inspect tunneled traffic or control other host interfaces.
- Core dumps are disabled; worker descriptors and process creation are bounded. These are resource limits, not proof against denial of service.

Intentional denial of service by a faulty/malicious phone is still possible. The test corpus does not substitute for a hardware fuzzing campaign or a formal audit.

## Routing and privacy

This is an IPv4 connectivity tool, **not a VPN or leak-prevention product**. Existing connections, more-specific routes, application-bound interfaces, IPv6 and resolver selection may use other paths. The supervisor conservatively preserves detected IPv4 `utun` routes, including split-default routes. VPN implementations using other interface names or policies need separate testing. These checks constrain GalaxyBridge's own default-route writes; DHCP supplied by the phone can still affect DNS and other OS network settings, including routes. The driver does not implement a separate privileged DHCP policy parser or promise protection from a malicious network peer.

There is no telemetry collector, packet-content logger, background update downloader or credential store. Logs contain lifecycle/errors and aggregate frame counts, not packet payloads. USB serial numbers are printed only when explicitly requested; device errors may still contain operating-system diagnostics. Review/redact logs before sharing them.

A configured serial filter is not a secret: although its plist is root-readable, command arguments or launchd/process inspection may reveal it locally. Do not use serial filters as a confidentiality or authentication mechanism.

SIP, Secure Boot policy and USB debugging do not need to be weakened. These facts do not remove the risk of installing a third-party root service. `feth` is a private macOS interface and can change across OS updates.

## Supply chain

`Cargo.lock` fixes dependency versions and checksums. Release builds inspect architecture, minimum OS target, runtime library paths and ad-hoc code-signature validity. Only clean system-library paths are allowed. Dependency license texts accompany release bundles.

**SHA-256 files served beside an archive and ad-hoc signatures do not authenticate the publisher.** An attacker controlling the GitHub account or release pipeline could replace both. Binaries are not Apple Developer ID signed or notarized. Use trusted release sources, review changes, and build from the recorded commit if your environment requires source review. Do not disable Gatekeeper or SIP to work around a suspicious artifact.

## Review record

Before the initial release, separate agent reviews examined the root/worker boundary, USB protocol handling, and installer/packaging. They found routing, RNDIS padding/notification, and packaging-validation issues; corrections and regression tests were added. This is development review, **not an independent professional security audit**. Version 0.2.0 adds adversarial installer/runtime regression tests, seeded parser fuzzing with AddressSanitizer, and App Sandbox runtime denial tests. The exact scope and evidence are recorded in [the hardening report](docs/security-hardening-0.2.0.md) and [the parser fuzz report](fuzz/REPORT.md).

Known-advisory checks and test counts are recorded in the release notes at the time they run; a historical clean result does not establish future safety.

## Reporting

Report security-sensitive findings through [GitHub private vulnerability reporting](https://github.com/tkddu1591/galaxybridge/security/advisories/new). Do not post USB serial numbers, IP addresses, credentials or packet captures in public issues. Include the GalaxyBridge version/commit, macOS version, phone model/One UI version, and minimal reproduction steps.
