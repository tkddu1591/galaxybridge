# Security model and limitations

GalaxyBridge is an experimental network driver. A code review and passing tests are not a guarantee that it has no vulnerabilities. Do not connect an untrusted USB device simply because it presents Samsung identifiers.

## Trust boundaries

1. **Installer:** downloaded code is reviewed and run with administrator authorization. It performs no downloads or builds as root. It copies a validated bundle into root-owned directories and refuses symlink or unrelated destination collisions. Checksums are rechecked in root-private staging. The optional launchd arguments are constrained and stored in a root-only plist.
2. **Root supervisor:** owns BPF and the two virtual interfaces it creates. System commands use absolute paths, separate argument arrays, a cleared environment, and timeouts. It does not receive or parse RNDIS USB messages. Route changes preserve interface identity and are verified after application; unreadable route state fails closed.
3. **USB worker:** a fresh process clears supplementary groups and permanently changes UID/GID to macOS `nobody` before USB enumeration and protocol parsing. It receives only an unnamed Unix datagram socket. BPF and lock descriptors are close-on-exec and are not passed to it.
4. **IPC:** only a fixed-size validated unicast MAC address or a bounded Ethernet frame is accepted. The worker cannot ask the supervisor to run commands, read files, bind BPF to another interface, or choose arbitrary interface names.
5. **Phone/network:** the selected device remains a network peer. Its Ethernet and DHCP traffic enters macOS's network stack. It can announce DNS/gateway settings and observe traffic whose encryption does not protect it. VID, PID and serial filters are spoofable identifiers, not cryptographic authentication.

The worker is **not sandboxed**. A compromise could still use the filesystem/network capabilities of `nobody`, interact with other processes under that shared identity, and inject frames into the dedicated tethering link. The kernel, IOKit, nusb, native ABI bindings, Rust dependencies, installer and root supervisor are part of the trusted computing base.

## Input and resource controls

- USB control/data transfers are bounded to 16KiB and Ethernet frames to 1,514 bytes.
- RNDIS types, response IDs/statuses, declared message lengths, offsets, and optional metadata are validated using checked slices.
- USB termination padding is separate from RNDIS message length.
- RX transfers and TX submissions are bounded. Backpressure can drop frames instead of allocating an unbounded queue.
- Root commands have bounded execution time; worker shutdown is cooperative with a final forced-termination fallback.
- Device disappearance, malformed data or failed initialization ends the session. Automatic mode retries rather than bypassing validation.
- No shell evaluates data received from a USB device, route command, serial number, or MAC address.

Intentional denial of service by a faulty/malicious phone is still possible. The test corpus does not substitute for a hardware fuzzing campaign or a formal audit.

## Routing and privacy

This is an IPv4 connectivity tool, **not a VPN or leak-prevention product**. Existing connections, more-specific routes, application-bound interfaces, IPv6 and resolver selection may use other paths. The supervisor conservatively preserves detected IPv4 `utun` routes, including split-default routes. VPN implementations using other interface names or policies need separate testing.

There is no telemetry collector, packet-content logger, background update downloader or credential store. Logs contain lifecycle/errors and aggregate frame counts, not packet payloads. USB serial numbers are printed only when explicitly requested; device errors may still contain operating-system diagnostics. Review/redact logs before sharing them.

SIP, Secure Boot policy and USB debugging do not need to be weakened. These facts do not remove the risk of installing a third-party root service. `feth` is a private macOS interface and can change across OS updates.

## Supply chain

`Cargo.lock` fixes dependency versions and checksums. Release builds inspect architecture, minimum OS target, runtime library paths and ad-hoc code-signature validity. Only clean system-library paths are allowed. Dependency license texts accompany release bundles.

**SHA-256 files served beside an archive and ad-hoc signatures do not authenticate the publisher.** An attacker controlling the GitHub account or release pipeline could replace both. Binaries are not Apple Developer ID signed or notarized. Use trusted release sources, review changes, and build from the recorded commit if your environment requires source review. Do not disable Gatekeeper or SIP to work around a suspicious artifact.

## Review record

Before the initial release, separate agent reviews examined the root/worker boundary, USB protocol handling, and installer/packaging. They found routing, RNDIS padding/notification, and packaging-validation issues; corrections and regression tests were added. This is development review, **not an independent professional security audit**.

Known-advisory checks and test counts are recorded in the release notes at the time they run; a historical clean result does not establish future safety.

## Reporting

Report security-sensitive findings through [GitHub private vulnerability reporting](https://github.com/tkddu1591/galaxybridge/security/advisories/new). Do not post USB serial numbers, IP addresses, credentials or packet captures in public issues. Include the GalaxyBridge version/commit, macOS version, phone model/One UI version, and minimal reproduction steps.
