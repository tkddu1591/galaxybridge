# Security hardening for 0.2.0

This report describes agent-assisted adversarial review and reproducible checks.
It is not an independent professional audit, a certification, or proof that no
vulnerability exists. The review assumes the OS and administrator are trusted;
a USB worker may be compromised, device bytes may be malicious, and installation
inputs or writable paths may be controlled by another local user.

## Corrected boundaries

| Finding / attack scenario | Correction | Evidence |
| --- | --- | --- |
| Copied macOS source ACLs could preserve another user's write access to installed privileged code despite mode 0755 | Clear copied ACLs/BSD flags, then verify root ownership, links, mode and ACL state; retain quarantine | Real filesystem ACL reproduction and installer regression |
| `xcrun` can execute a selected developer directory's dispatcher; untrusted toolchain paths or inherited overrides could reach privileged execution | Clear overrides, validate the selected developer path/dispatcher and resolved tools before executing them | Dispatcher fixture and path validation tests |
| A shared `nobody` identity provided no isolation from other services using that UID | Dedicated disabled user/group, pinned UUID and numeric IDs, private receipt, strict local/NSS agreement | Identity parser/runtime and stateful account-lifecycle tests |
| A different local user could replace a group/ACL-writable source installer while administrator authentication was pending | Reject unsafe source ownership, writable permissions/ACLs and unsafe ancestors before tool dispatch and sanitized sudo handoff | Real shared-source and ACL rejection fixtures |
| An inherited working directory or non-CLOEXEC descriptor could expose access unrelated to tethering | Start at `/`, sweep inherited descriptors before exec, preserve only required streams/IPC, then drop credentials irreversibly | High-descriptor, exec-failure and credential-boundary regressions |
| An inherited terminal or privileged log descriptor could give a compromised worker unintended I/O authority | Pipe worker output, bound and escape diagnostics, and start a new session without a controlling terminal | Real controlling-PTY fixture, output-limit and terminal-escape regressions |
| A plain unprivileged USB worker could access ordinary files/network sockets permitted to its UID | Separate App Sandbox executable, only USB entitlement, fixed immutable app, no unsandboxed fallback | Runtime denial prototype, signature/entitlement negative tests, helper self-check |
| Special/permissive state files could bypass the lock/receipt assumptions | No-follow/nonblocking opens plus regular-file, owner, single-link, exact-mode and ACL checks | File/ACL regression fixtures |
| A USB transfer timeout could enter nusb's unbounded cancellation-completion loop | Bounded completion wait; terminate the session on timeout/short write | Reviewed dependency path and transfer code; OS-call timing remains outside this guarantee |
| Malformed/duplicate Union, IAD or endpoint descriptors could be ignored or guessed around | Validate the complete bounded USB descriptor chain and reject ambiguous functions/alternates | Deterministic hostile descriptor fixtures and seeded ASan fuzzing |
| Unsupported control metadata and ambiguous message boundaries expanded parser acceptance | Strict message/status/notification/offset/alignment validation | Deterministic protocol regressions and seeded ASan fuzzing |
| The root bridge accepted unrelated Ethernet types | Enforce IPv4/ARP only in both directions | Root frame-policy tests; no privileged DHCP parser added |

Live installation also established that `dscl` returns the hidden-user field as `dsAttrTypeNative:IsHidden`; both installer and runtime identity checks now parse that precise field while still rejecting duplicate aliases.

A live installation caught an invalid `install -f 0` option that component-level metadata tests had missed. The installer now supplies an empty symbolic flag list; a new regression executes the complete real copy command (with non-root ownership only substituted for CI), verifying flags, ACLs and retained quarantine.

Additional cleanup tests reject stale/missing account identities, mounted home roots, symlink/hardlink escapes and partial deletion. Teardown always requests removal of the verified dedicated launchd user domain, including an idle one. It does not query a per-user domain while waiting: resolving that context can recreate it. Successful scoped bootout and real/effective-UID process quiescence create a temporary proof bound to both numeric IDs and both ownership UUIDs; destructive steps require that proof. A partial-install exception is limited to a receipt created by the same invocation before any home was created. macOS `find` may return success after an unlink failure, so account removal requires the home to be absent; directory-service deletions also have absence postconditions. Release archives omit builder xattrs, ACLs, flags and local owner names.

## Parser fuzzing

Two 181-second AddressSanitizer/libFuzzer campaigns executed **77,153,668**
inputs with no sanitizer report or crash artifact. Seeds included valid protocol
messages/configurations so mutations could reach beyond initial header checks.
Exact target hashes, command lines, seeds, tool versions and coverage counters are
in [the parser report](../fuzz/REPORT.md). Those counters are not coverage percentages.
These targets do not exercise real USB transfers, the kernel, installer or routing.

A follow-up packed Android aggregation correction passed 6,868,703 additional
ASan/libFuzzer executions in 181 seconds, with no report or assertion failure.
It removes only the aggregate-start eight-byte restriction while retaining
message and payload bounds. See [the compatibility report](../fuzz/PACKED-AGGREGATION.md)
for the exact revised source hash, test inputs and remaining physical-test gate.

## App Sandbox evidence

A separately ad-hoc-signed app with only `com.apple.security.app-sandbox=true`
and `com.apple.security.device.usb=true` was compared with an unsandboxed control
on macOS 26.5.1 / arm64 under an ordinary logged-in account:

- Reading a benign Documents fixture changed from success to `EPERM`.
- Connecting to an active localhost TCP listener changed from success to `EPERM`.
- Bidirectional inherited Unix datagram FD 3 communication still worked.
- nusb enumeration saw the same five USB devices; no phone was claimed in this test.
- Changed-code v1 → v2 ad-hoc updates retained the app's own container data and the
  same denials. No custom designated requirement or weakened policy was used.

This establishes actual enforcement in that tested context, not merely the
presence of entitlement strings. On 2026-09-14 the installed dedicated account (UID/GID 60000) was tested in its
own Mach bootstrap context using `launchctl asuser`. The same production native
credential/descriptor setup was used for both control and sandbox runs. The
control could read a benign world-readable fixture and connect to a live localhost
listener; the sandbox received `EPERM` for both. Both exchanged FD 3 messages and
enumerated the same five USB devices. Actual real/effective UID/GID, supplementary
groups, no unrelated inherited descriptors, new session, and CORE=0/NOFILE=256/
NPROC=16 were checked. The installed native test also verified that root UID/GID
could not be recovered after dropping privileges.

The first system-launched attempt failed because UID dropping did not switch the
inherited root Mach bootstrap context: secinitd reported an euid/uid mismatch.
Matching the bootstrap context to the dedicated account fixed the probe. The automatic service now reaches real phone control and bulk traffic under this
account. Physical testing then exposed an aggregate-packet compatibility failure
after DHCP; stable transfer and unplug recovery remain separate release gates.

## Release validation

The dependency advisory scan performed on 2026-09-14 reported no known RustSec
vulnerabilities or warnings in the runtime lockfile. A clean advisory scan does
not cover unknown defects, operating-system issues or all build-tool risks.

Integrated Rust tests: 78 passed, with two explicit platform/privileged integration tests excluded from the default suite. Installer tests: 29 passed; account/home lifecycle tests: 32 passed; signed-worker packaging regression: passed. Formatting and Clippy warnings-as-errors passed.

The installed root-only real/effective/saved credential test and dedicated-account sandbox runtime probe passed on 2026-09-14. Automatic-service startup and DHCP have been observed, but the USB worker repeatedly exits during packet parsing. A packed Android RNDIS aggregation compatibility correction is under validation; stable USB transfer and Wi-Fi recovery remain release gates. A prior 0.1.0 connectivity test is not evidence for the new sandboxed worker.

The public setup bootstrap passed nine non-privileged fixture tests covering verified automatic/manual installation, corrupt and partial downloads, failed Apple-tool/install steps, cleanup, checked README download execution, and the actual inherited kernel file-size/core limits. No root installation or real phone traffic is exercised by these bootstrap fixtures.

## Remaining risk

The root supervisor and Apple's USB, BPF, network and sandbox implementations are
trusted. The reviewed installer source, the invoking user's own storage, sudo and root-owned Apple verification tools are also trusted; source permission checks do not authenticate the publisher or defend against already compromised same-user storage. App Sandbox blocks direct socket/file operations beyond its allowances;
the worker still has USB, its own container and the intended IPC capabilities.
An authorized phone can carry network traffic and supply DHCP/DNS settings. The
route guard constrains this driver's own default-route writes, not every change
that the OS may accept from DHCP. It is not a VPN or leak-prevention system.

Vendor/product/serial filters are spoofable selection hints. Manufacturer-neutral
RNDIS support does not authenticate Android, prove a device is safe, or establish
compatibility with every phone. Only a physical Galaxy S25 Ultra is available for
hardware validation; synthetic alternate-vendor fixtures are not hardware tests.

Binary signing remains ad-hoc; there is no Developer ID/notarization or publisher
identity guarantee. The private `feth` API, malicious-device denial of service,
system crashes/forced termination, sleep/reboot and other OS/device combinations
remain distinct validation concerns.

## Primary platform references

- [Apple DTS: sandboxed launchd jobs and app wrappers](https://developer.apple.com/forums/thread/802443)
- [Apple: provisioning profiles and unrestricted macOS sandbox entitlements](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)
- [Apple: App Sandbox USB and network entitlements](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html)
- [Microsoft: Remote NDIS](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis--rndis-2)
