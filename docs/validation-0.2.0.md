# 0.2.0 physical and security validation

Validation dates: 2026-09-14 and 2026-09-15. Host: Apple M3 Pro, macOS 26.5.1. Phone: Galaxy S25 Ultra exposing RNDIS. Wi-Fi remained enabled during USB testing. This is development validation on one hardware/OS combination, not certification or evidence for every Android device.

## Installation and isolation

A complete real uninstall removed the dedicated account and its owned home, then a fresh installation enabled automatic reconnect. The installed supervisor, USB worker, identity module and uninstaller matched the checked release archive byte for byte.

A local diagnostic established why the earlier teardown failed: immediately after `launchctl bootout user/<uid>`, the system inventory contained no target user domain. A subsequent direct `launchctl print user/<uid>` recreated it. Production teardown no longer performs that query or parses the unsupported `print system` output. It relies on successful scoped bootout, current process quiescence, and an in-process proof bound to the account's numeric IDs and UUIDs.

The real USB worker ran as UID/GID 60000 with a separate root supervisor. The installed app's code signature verified, and its entitlements contained exactly App Sandbox and USB-device access. The earlier dedicated-account control/probe comparison used the production native credential and descriptor setup: ordinary fixture reads and localhost TCP access succeeded in the control and received `EPERM` under App Sandbox, while inherited FD 3 communication and USB enumeration succeeded. Supplementary groups, inherited descriptors, session detachment and resource limits were checked. A separate installed privileged test verified that the worker could not recover root credentials after dropping them. See [the security report](security-hardening-0.2.0.md) for the evidence boundary.

## Live transfer

The previous parser was observed rejecting an aggregate message at transfer offset 135. The revised parser accepts packed Android aggregates without dropping bounds, message-type, payload-range or metadata checks.

The first sustained test ran for 180 seconds. All 18 fresh HTTPS requests explicitly bound to the USB interface succeeded, including larger HTML responses. The default IPv4 route stayed on `feth0`; both process IDs remained unchanged; the worker retained its dedicated UID/GID; and no new driver error or session restart appeared.

The first physical unplug test recovered the Wi-Fi default route 0.597 seconds after USB removal was detected and completed a new HTTPS request after 0.903 seconds. Both virtual interfaces were removed; Wi-Fi power was on before and after the test, and the user was instructed to leave it unchanged. The active DNS configuration returned to the Wi-Fi resolver, which answered a new randomized DNS query in 10 ms. Timing is relative to polling detection (approximately 150 ms plus command latency), not an instrumented cable contact.

After reconnect, a bounded 8 MiB download and a 2 MiB upload to [Cloudflare’s documented test endpoints](https://github.com/cloudflare/speedtest) both completed with HTTP 200 while explicitly bound to the USB interface. They used synthetic test data and did not upload user files.

After reconnect, a second 180-second observation completed all 18 fresh HTTPS requests with no process restart or new driver error. A second physical unplug recovered the Wi-Fi route in 0.380 seconds and fresh HTTPS in 0.717 seconds; both virtual interfaces were removed and Wi-Fi remained on. This second measurement finished at 18:07:56 KST, before the diagnostic service configuration was cleaned up at 18:09:11, so the service restart did not cause the measured recovery.

The temporary log setting was removed, and the ordinary automatic service restarted successfully. On 2026-09-15 the phone reconnected under that normal configuration. A final 60-second observation completed all six fresh USB-bound HTTPS requests, with the same supervisor and worker processes and the dedicated UID/GID throughout. This brings the three observation windows to 42 successful requests. The signature and the four runtime fingerprints below were rechecked after this final test.

## Runtime fingerprints

```text
f4ecd8312f23d6b7e2be657f68ce5f214c6c8a644f40be538c7435c2126aaf17  galaxybridge
c75cc2ba06f5754029e42b8f3bf742b916889a0947cb768feacb29e108076cd4  USBWorker.app/Contents/MacOS/galaxybridge-usb
8361d24670ce02893979543c576f8f17b1c600372e598706be5ad5a6939cba7a  identity.sh
b470e82e285a8d4396df461f11b31e3cb4ed3fba876422f603a708030d11a4b1  uninstall.sh
```

## Limits

Sleep/wake, reboot, other Android models, other macOS versions, IPv6 routing and VPN leak prevention are not established by these tests. Signing is ad-hoc; Developer ID and notarization are not present. Automated security checks and parser fuzzing cannot prove the absence of unknown vulnerabilities.
