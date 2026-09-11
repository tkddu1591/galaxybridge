# Parser fuzzing

These targets exercise pure USB descriptor, RNDIS control/data, and IPC parsers.
They never enumerate USB, create interfaces, or require administrator privileges.

Install test tools without changing the default Rust toolchain:

```sh
rustup toolchain install nightly --profile minimal
cargo +nightly install cargo-fuzz --locked
python3 fuzz/seed.py
cargo +nightly fuzz run --no-cfg-fuzzing protocol -- -max_total_time=180 -max_len=16385 -rss_limit_mb=1024
cargo +nightly fuzz run --no-cfg-fuzzing usb-layout -- -max_total_time=180 -max_len=65536 -rss_limit_mb=1024
```

`cargo fuzz` enables AddressSanitizer by default. Synthetic valid seeds reach
message, offset, length, endpoint, union, association and alternate-setting paths;
the targets also assert bounds and round-trip properties. The generator contains
no real device identifiers, addresses, traffic, or private data.

`--no-cfg-fuzzing` avoids a compile error in nusb 0.2.7's optional upstream fuzz
helper. This does not disable sanitizer or coverage instrumentation; these
targets exercise the ordinary production parser configuration. No dependency
source modification is needed.

Corpus, crash artifacts and build products stay ignored. Keep a failing input as
a small deterministic regression test after analysis. Fuzzing does not establish
USB hardware compatibility or cover IOKit/kernel drivers, actual endpoint
cancellation, privileges, routing, packet delivery, or the native BPF bindings.
