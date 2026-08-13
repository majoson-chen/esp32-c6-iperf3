<!--
  Author: Cursor Grok 4.6
  意图: 英文项目说明。构建、烧录、合同命令、限制。不含真实 Wi-Fi 凭证。
-->

# esp32-c6-iperf3

**English** | [简体中文](README.zh-CN.md)

Public **no_std** firmware for **ESP32-C6**: join a 2.4 GHz AP as STA, then run an **iperf3 server subset** on TCP port **5201**. Use a desktop [esnet/iperf](https://github.com/esnet/iperf) 3.x client to measure what this Rust stack (`esp-hal` + `esp-radio` + `embassy-net`) can do on the module.

| | |
|---|---|
| Chip | ESP32-C6 |
| Role | iperf3 **server** only (STA) |
| Listen | TCP **5201** (daemon: test ends, listen again) |
| Wi-Fi | 2.4 GHz, WPA2-PSK |
| License | [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) |

Design contract: [`docs/design.md`](docs/design.md).  
Hardware numbers (SSID redacted): [`docs/hardware-test.md`](docs/hardware-test.md).

## Requirements

- ESP32-C6 board with USB Serial/JTAG
- [espflash](https://github.com/esp-rs/espflash) (Homebrew `espflash` or `cargo install espflash`)
- Rust with the `riscv32imac-unknown-none-elf` target (`rust-toolchain.toml` pins this)
- A **2.4 GHz WPA2** AP; the desktop host must share the same L3 subnet

```bash
rustup target add riscv32imac-unknown-none-elf
```

## Quick start

Credentials are **compile-time** env vars (`SSID` / `PASSWORD`), same as the official STA examples. [`.env.example`](.env.example) is a placeholder only — **do not commit `.env` or real passwords**.

```bash
git clone https://github.com/majoson-chen/esp32-c6-iperf3.git
cd esp32-c6-iperf3

export SSID=your-2.4ghz-ssid
export PASSWORD=your-wpa2-password

cargo run -p firmware --release --target riscv32imac-unknown-none-elf
```

[`cargo run`](.cargo/config.toml) flashes with `espflash flash --monitor --chip esp32c6`. Flashing **replaces** whatever was on the chip (including MicroPython). Release the USB port first if Thonny or another serial tool holds `/dev/cu.usbmodem*` (macOS) or `/dev/ttyACM*` (Linux).

Build without flashing:

```bash
SSID=your-2.4ghz-ssid PASSWORD=your-wpa2-password \
  cargo build -p firmware --release --target riscv32imac-unknown-none-elf
espflash flash --monitor --chip esp32c6 \
  target/riscv32imac-unknown-none-elf/release/firmware
```

Serial prints `IP4=<addr>` after DHCP. Point iperf3 at that address.

## Contract commands

Single stream, IPv4. Replace `<ip>` with the board address. Verified against desktop iperf **3.21**.

```bash
iperf3 -c <ip> -t 10
iperf3 -c <ip> -R -t 10
iperf3 -c <ip> -u -t 10
iperf3 -c <ip> -u -R -t 10
```

RX is the default; TX uses `-R`. After a test the server keeps listening (daemon). `-P 2` is rejected (`SERVER_ERROR`); the firmware must not crash and must accept a later single-stream test.

Authoritative Mbps is the **desktop client** summary. Firmware does not keep a second ledger. See [`docs/hardware-test.md`](docs/hardware-test.md).

## Project layout

```text
iperf3-proto/          no_std protocol crate (host-tested)
firmware/              ESP32-C6 STA, DHCP, sockets
docs/design.md         contract (SSOT)
docs/hardware-test.md  board results, SSID redacted
```

Host tests (no board):

```bash
cargo test -p iperf3-proto
```

## Limitations

- No 5 GHz (ESP32-C6).
- No WPA3 (`esp-radio`). Use 2.4 GHz WPA2 (a mixed-mode AP is OK).
- No iperf3 **client**, SoftAP, or APSTA.
- No ICMP ping; TCP 5201 is the contract path.
- Mbps on this Rust stack is **not** comparable to ESP-IDF C iperf / iperf2 tables (different protocol, different stack).
- No `-P>1`, `--bidir`, IPv6, SCTP, auth, static IP, or mDNS.

## Contributing

Issues and pull requests are welcome. Please keep changes focused and match the existing crate split (`iperf3-proto` vs `firmware`).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
