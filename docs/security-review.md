# Development security review — 2026-09-11

This record describes agent-assisted development review, not an independent professional security audit. Reviewers examined separate areas and then re-reviewed the resulting fixes. Real USB hardware testing remains necessary in addition to tests.

| Finding | Correction |
| --- | --- |
| An unparsed `link#N` VPN gateway could be mistaken for no default route | Preserve interface metadata separately from an optional IPv4 gateway; fail closed on unknown route state |
| Overlapping phone/Wi-Fi subnets could select the wrong interface | Pass the intended `-ifp` and verify the default route after the change |
| Split-default VPN routes were not detected | Inspect the actual IPv4 route-table Netif column for `utun` routes |
| macOS `route get` can report a missing default with exit status zero, preventing fallback restoration | Recognize the exact missing-route diagnostic with empty output independently of exit status; continue to reject unknown or conflicting output |
| Undrained subprocess pipes could block network inspection on large output | Drain both pipes while the command runs, with bounded output and a timeout |
| USB short-packet termination byte was included in RNDIS MessageLength | Keep transport padding outside the protocol message length |
| Legal long zero padding could terminate a session | Accept bounded zero padding and reject corrupted/nonzero trailers |
| Control responses were polled without RESPONSE_AVAILABLE | Claim the interrupt endpoint and validate bounded notifications before response reads |
| Worker cancellation could be delayed by a receive batch | Check cancellation within batches and notice completed/panicked worker threads |
| Release build could copy an older artifact when Cargo's target directory changed | Use one explicit controlled target directory; regression test plants a stale binary |
| A textual system-library prefix allowed `..` traversal | Reject dot components, duplicate separators and other unclean paths before allowlisting |
| Stripped Rust binaries retained local source paths | Remap source paths for release builds and inspect archives for builder path leakage |

Additional live fixes: finalize the MAC before announcing the feth peer link so DHCP does not cache the initial generated address; treat bounded IPC/BPF ENOBUFS as congestion rather than a fatal disconnect.

## Checks

- Rust protocol, IPC, route and backpressure tests, including a deterministic malformed-input corpus.
- Installer argument/path validation and dependency-path attack cases.
- Packaging regression with an inherited alternate `CARGO_TARGET_DIR` and a deliberately stale artifact.
- Formatting, Clippy warnings-as-errors, locked builds, architecture/minimum OS/library-path checks, archive checksums, and ad-hoc signature verification.
- `cargo audit` against the RustSec advisory database on 2026-09-11: no reported vulnerabilities or warnings in the locked dependencies at that check.

The tests do not establish correctness for every phone, OS version, VPN, malicious USB device or network stack. They do not prove the absence of unknown vulnerabilities.

## Remaining trust

The worker is unprivileged but not sandboxed; the phone remains a network peer; the root supervisor and native/OS boundaries remain privileged; `feth` is private API. USB identifiers are not authentication. Neither adjacent checksums nor ad-hoc signatures prove publisher identity. See [SECURITY.md](../SECURITY.md).
