# Packed RNDIS aggregation compatibility — 2026-09-14

The initial security hardening rejected a RNDIS message if its starting offset
within an aggregate was not a multiple of eight. This is too strict for Android
gadget implementations that concatenate the RNDIS header and Ethernet payload
without adding alignment padding. Single-message DHCP can succeed while later
aggregated traffic makes the worker end its session.

Primary implementation evidence:
[Android MSM gadget `u_ether.c`, revision 9930824a7c845e515f00dec329f0105283a84633](https://android.googlesource.com/kernel/msm/+/9930824a7c845e515f00dec329f0105283a84633/drivers/usb/gadget/function/u_ether.c).
Its `eth_start_xmit` aggregation path appends the header and payload and advances
the transfer length by their lengths without rounding to eight bytes. This is
evidence of an Android implementation pattern, not proof that a particular
phone runs that exact source revision.

The correction removes only the aggregate-start modulo-eight condition. Parsing
still advances by each validated `MessageLength`, never scans or rounds forward
to search for a later header. Message type, fixed header length, positive bounded
message length, checked payload offsets, payload size, relative data alignment,
zero unsupported metadata and the 16 KiB transfer bound remain checked. The wire
reader copies bytes into integers; it never dereferences an unaligned integer
pointer. Separate truncated-header and wrong-message-type errors retain only
offset/size metadata and do not log packet contents.

## Regression checks

`cargo test --locked --test protocol --test control_security` passed **25 tests**:
19 protocol tests and six control-security tests. The updated coverage includes:

- Three-message packed aggregates spanning every modulo-eight start offset,
  with and without trailing zero USB padding.
- Incorrect type, zero/short/oversized message length, escaping payload offsets,
  oversized payload and unsupported metadata in the second packed message.
- Nonzero truncated tails and inserted bytes before an otherwise valid later
  header, ensuring the parser does not resynchronize past corruption.
- Existing aligned aggregates and the relative `DataOffset` alignment rule.

`cargo clippy --locked --all-targets -- -D warnings` and `git diff --check` passed.

## AddressSanitizer campaign

Eight new synthetic packed-aggregate seeds were added to the retained protocol
corpus. Message starts include `[0, 104..111, 209..216]`. The campaign started with
145 corpus files, including earlier generated mutations, rather than a fresh
corpus. It used the same protocol target and sanitizer configuration as the
[earlier report](REPORT.md).

```sh
python3 fuzz/seed.py
cargo +nightly fuzz run --no-cfg-fuzzing protocol -- \
  -max_total_time=180 -max_len=16385 -rss_limit_mb=1024 -print_final_stats=1
```

Result: **6,868,703 executions in 181 seconds**, exit zero, no sanitizer report or
assertion failure. Random seed: `2923867856`. Final coverage counters: `362`;
features: `804`; peak RSS: `434 MiB`; new units: `146`. These are libFuzzer metrics,
not source-coverage percentages or a security guarantee. The output is retained
locally in ignored `fuzz/packed-aggregation.log`.

Source SHA-256:

```text
c38a779583d1a55864e0e38ab593d963b4369d6af021ed240e78c8fe64a77a07  src/rndis/packet.rs
56bdaba07d379cad34527b255f5e60ec51b3538b85effd5c28c136891fb08c04  fuzz/fuzz_targets/protocol.rs
```

This campaign covers parser compatibility and hostile-input bounds. It does not
verify actual USB hardware, IOKit behavior, routing or sustained internet access;
those require separate live evidence.
