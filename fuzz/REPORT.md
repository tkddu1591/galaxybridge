# Parser security validation — 2026-09-11

This is a bounded adversarial test result for the v0.2.0 working tree, not a
security certification or a claim that all Android devices are supported.

The 2026-09-11 campaign below is historical. The later packed-aggregation
compatibility correction is documented in [PACKED-AGGREGATION.md](PACKED-AGGREGATION.md).
In particular, aggregate message starts no longer require eight-byte alignment;
message and payload bounds remain mandatory.

## Findings corrected

- **Availability, P2:** nusb 0.2.7's `transfer_blocking` cancels a timed-out
  transfer and then waits indefinitely for cancellation completion. A failed USB
  backend can therefore leave the worker waiting despite its nominal timeout.
  The worker now submits once, waits at most 500 ms for a completion, and ends
  the session on timeout or a short write. Endpoint destruction requests
  cancellation; this removes the explicit unbounded Rust wait, but does not
  establish a bound on underlying IOKit/kernel calls.
- **Function-selection ambiguity, P2:** a malformed or duplicate CDC Union
  descriptor could be skipped and replaced with an adjacent-interface guess.
  Configuration validation now consumes the entire bounded descriptor chain,
  rejects malformed tails, duplicate alternates/endpoints, overlapping interface
  associations, cross-function data selection and multiple usable data alternates.
  Missing Union descriptors retain the conventional adjacent-interface fallback;
  malformed Union descriptors do not.
- **Protocol hardening:** standalone query/initialization parsers validate their
  own message type, length and status; unsupported initialization metadata and
  malformed status buffers are rejected. Notifications require the exact
  eight-byte format. Media-disconnect indications end the session, and more than
  32 responses without the requested completion fail closed. At this campaign's
  revision, packet data offsets and aggregate starts required RNDIS alignment;
  the latter requirement was subsequently corrected for Android compatibility.

These findings concern availability, selection ambiguity and protocol acceptance.
The review did not establish an exploitable memory-corruption or privilege-
escalation vulnerability in these modules.

## Executed tests

Platform: aarch64 macOS 26.5.1, build 25F80. Tooling: `cargo-fuzz 0.13.2`,
`libfuzzer-sys 0.4.13`, `rustc 1.100.0-nightly (67eda617e 2026-09-10)`.
The normal default Rust toolchain was not changed. Common package versions in
`fuzz/Cargo.lock` matched the root `Cargo.lock` when the targets were built.

Both campaigns used AddressSanitizer, sanitizer coverage, debug assertions and
the normal production configuration (`--no-cfg-fuzzing`; see the README for the
nusb upstream helper build issue). They ran concurrently and each exited zero.

| Target | Synthetic seeds | Random seed | Executions | Runtime | Final coverage counters | Final features | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `protocol` | 22 | 2768401152 | 15,135,463 | 181 s | 360 | 800 | 754 MiB |
| `usb-layout` | 4 | 2768401210 | 62,018,205 | 181 s | 549 | 1,160 | 529 MiB |

Total: **77,153,668 executions**, no sanitizer report, assertion failure, timeout
artifact or crash artifact. Coverage counters/features are libFuzzer metrics,
**not source coverage percentages**. Allocation quarantine and runtime overhead
are included in the sanitizer process RSS figures.

Commands (start with an empty corpus in a fresh checkout to reproduce the seed
set; see `seed.py`):

```sh
python3 fuzz/seed.py
cargo +nightly fuzz run --no-cfg-fuzzing protocol -- \
  -max_total_time=180 -max_len=16385 -rss_limit_mb=1024 -print_final_stats=1
cargo +nightly fuzz run --no-cfg-fuzzing usb-layout -- \
  -max_total_time=180 -max_len=65536 -rss_limit_mb=1024 -print_final_stats=1
```

Fourteen additional deterministic tests cover malformed/duplicate CDC Union and
IAD descriptors, malformed descriptor tails, endpoint addresses/directions/sizes,
alternate selection, every byte mutation of the notification format, independent
control-header validation, status-buffer bounds, aggregate/data-offset alignment,
and 1,501 frame lengths across four data offsets. These passed alongside the
existing 17 protocol tests. `cargo clippy --locked --all-targets -- -D warnings`
also passed at the time of this review.

## Tested source fingerprints

SHA-256 for the parser and fuzz-target sources used by these campaigns:

```text
343dd1c8074aea8fd09d4d289fd78b664d013d50aa428a7712107a44b1cee550  src/rndis/control.rs
0cb9b12efa13bcb96b30679e7a4ade5dec4624d8fb03d04ab5cfb236c42a3c37  src/rndis/packet.rs
58edc19f735f31d364b1c973d4cd2696df39f066ce4d8542af147b3b391113b7  src/usb/layout.rs
56bdaba07d379cad34527b255f5e60ec51b3538b85effd5c28c136891fb08c04  fuzz/fuzz_targets/protocol.rs
ad895cc953bf12e344c1fc71628fc4668e87ae75dad03aec7ac3769bdd4b9e7c  fuzz/fuzz_targets/usb_layout.rs
```

## Limits and remaining trust

The targets exercise pure parsers and payload round-trip/bounds properties. They
do not invoke USB enumeration, real transfers, interface setup or routing. They
do not test actual IOKit cancellation, USB/kernel/network-stack vulnerabilities,
native BPF calls, permissions, installation, reconnect/sleep/reboot behavior,
throughput or physical device compatibility. Those require separate review and
integration evidence. No TetherKit source or binary is used by these targets.

USB vendor/product/serial values are selection hints, not cryptographic identity.
A malicious device can spoof them. A selected phone remains a network peer that
can supply Ethernet and DHCP traffic. Connecting an untrusted device is outside
the trusted-phone assumption; parser tests do not remove that risk.

The supported shape is one unambiguous Ethernet RNDIS 1.0 control/data function,
including a single usable data alternate and supported bulk/notification
endpoints. NCM and other USB-network protocols are outside this implementation.
Rejecting malformed or ambiguous firmware is intentional and may limit device
compatibility.

## Primary references

- [Microsoft RNDIS packet format](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-packet-msg)
- [Microsoft USB control channel](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/control-channel-characteristics)
- [Microsoft RNDIS status indications](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-indicate-status-msg)
- [Microsoft RNDIS initialization completion](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-initialize-cmplt)
- [Microsoft RNDIS query completion](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-query-cmplt)
- [USB-IF CDC specifications](https://www.usb.org/documents?items_per_page=50&order=title&search=cdc&sort=desc)
- [nusb 0.2.7 endpoint implementation](https://docs.rs/nusb/0.2.7/src/nusb/device.rs.html)
