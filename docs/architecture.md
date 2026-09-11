# Architecture

```mermaid
flowchart LR
    Apps[Mac applications] <--> Stack[macOS network stack]
    Stack <--> Feth[Owned feth pair]
    Feth <--> Root[Root supervisor / BPF]
    Root <-->|Bounded Unix datagrams| Worker[App Sandbox USB worker]
    Worker <-->|nusb / IOKit| Phone[RNDIS device]
    Phone <--> Internet[Phone internet connection]
```

The supervisor launches the separate, signed `USBWorker.app` under the dedicated `_galaxybridge` identity. It validates the app signature, fixed root-owned path and exact sandbox/USB entitlement policy. Public device inspection also goes through the signed app. The USB program refuses root and unsigned development invocations before USB enumeration. It creates interfaces only when one eligible device is present. Interface names come from `ifconfig feth create`, are validated, and are never copied from USB strings.

The USB worker claims a control interface and its CDC data companion, reads the RNDIS notification endpoint, initializes the device, queries its MAC and sets the packet filter. It validates USB message framing in safe Rust and forwards only Ethernet frames through an unnamed socketpair. The initial MAC handshake has a fixed format and is validated by the supervisor.

The supervisor applies the MAC, starts DHCP and bridges only IPv4/ARP BPF frames. Other Ethernet types, including direct IPv6 router advertisements, are dropped at the root boundary. The BPF descriptor is held only by the supervisor. The small native C file binds to system BPF headers and performs the async-signal-safe descriptor/credential setup before the USB worker execs. It does not parse USB data.

## Ownership

| Module | Responsibility |
| --- | --- |
| `rndis::control` | Request encoding, completion validation and negotiated limits |
| `rndis::packet` | Bounded Ethernet packet framing and parsing |
| `usb::devices` | Manufacturer-neutral RNDIS eligibility and exact-one selection |
| `usb::layout` | Whole-configuration validation and unambiguous control/data/endpoint selection |
| `usb::confinement` | Require the executing helper's sandbox and USB signing entitlements |
| `usb::session` | USB interface claims and protocol control exchanges |
| `worker` | Bounded USB RX/TX and keepalive lifecycle |
| `ipc` | Root/worker message schema |
| `macos::interface` | Owned interface creation, MAC, DHCP and cleanup |
| `macos::bpf` | BPF descriptor and capture record parsing |
| `macos::route` | Fail-closed route inspection, preference and restoration |
| `macos::process` | Fixed helper launch, irreversible credential dropping and bounded worker lifetime |
| `macos::identity` | Private receipt, disabled account/group and directory lookup agreement |
| `macos::access` | Root-owned path/state-file validation and inherited descriptor policy |
| `macos::worker_app` | Signature, bundle identity and exact entitlement allowlist |
| `service` | Session orchestration and optional reconnect loop |

## Failure behavior

Device loss or malformed data ends the worker session. The supervisor stops/reaps its worker, closes BPF, withdraws the owned DHCP/DNS service and destroys only its pair. It then checks the previous physical service's current gateway and recovers routing if macOS has not independently selected another default. Automatic mode retries after a delay. It does not weaken validation after a failed connection.

Normal shutdown handles SIGINT/SIGTERM/SIGHUP. The worker also checks that its original supervisor still exists, so a forced parent exit does not intentionally leave USB ownership behind. A forced kill of the supervisor or an OS crash can prevent interface destructors from running; leftover virtual interfaces may require a reboot. The tool deliberately does not delete arbitrary pre-existing feth interfaces.

The driver favors bounded, reviewable behavior over throughput tuning: eight pending RX transfers, one TX transfer at a time, fixed frame and transfer limits. No performance claim is made until measured on real hardware.

## Protocol references

- [RNDIS control messages](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-messaging)
- [Control channel notifications](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/control-channel-characteristics)
- [Packet messages](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-packet-msg)
- [USB short-packet termination](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/usb-short-packets)
- [nusb API](https://docs.rs/nusb/0.2.7/nusb/)
- [Apple feth](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/net/if_fake.c)

TetherKit inspired the initial investigation of user-space tethering. GalaxyBridge's driver code was written independently against protocol/platform documentation; TetherKit source and binaries are not part of its build or runtime dependency graph.
