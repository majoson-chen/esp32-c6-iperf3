<!--
  Author: Cursor Grok 4.6
  意图: 简体中文项目说明。构建、烧录、合同命令、限制。不含真实 Wi-Fi 凭证。
-->

# esp32-c6-iperf3

[English](README.md) | **简体中文**

面向 **ESP32-C6** 的公开 **no_std** 固件：以 STA 加入 2.4 GHz AP，在 TCP **5201** 上跑 **iperf3 server 子集**。用桌面 [esnet/iperf](https://github.com/esnet/iperf) 3.x client 测这套 Rust 栈（`esp-hal` + `esp-radio` + `embassy-net`）在该模组上的吞吐。

| | |
|---|---|
| 芯片 | ESP32-C6 |
| 角色 | 仅 iperf3 **server**（STA） |
| 监听 | TCP **5201**（daemon：测完继续听） |
| Wi-Fi | 2.4 GHz，WPA2-PSK |
| 许可证 | [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) |

设计合同：[`docs/design.md`](docs/design.md)。  
硬件数字（SSID 已打码）：[`docs/hardware-test.md`](docs/hardware-test.md)。

## 环境要求

- 带 USB Serial/JTAG 的 ESP32-C6 板
- [espflash](https://github.com/esp-rs/espflash)（Homebrew `espflash` 或 `cargo install espflash`）
- 已安装 `riscv32imac-unknown-none-elf` 目标的 Rust（见 `rust-toolchain.toml`）
- **2.4 GHz WPA2** AP；电脑与板子同一 L3 网段

```bash
rustup target add riscv32imac-unknown-none-elf
```

## 快速开始

凭证是**编译期**环境变量（`SSID` / `PASSWORD`），与官方 STA example 一致。[`.env.example`](.env.example) 只是占位——**不要提交 `.env` 或真实密码**。

```bash
git clone https://github.com/majoson-chen/esp32-c6-iperf3.git
cd esp32-c6-iperf3

export SSID=your-2.4ghz-ssid
export PASSWORD=your-wpa2-password

cargo run -p firmware --release --target riscv32imac-unknown-none-elf
```

[`cargo run`](.cargo/config.toml) 会调用 `espflash flash --monitor --chip esp32c6`。烧录会**覆盖**板上现有固件（包括 MicroPython）。若 Thonny 等占用串口，先释放 `/dev/cu.usbmodem*`（macOS）或 `/dev/ttyACM*`（Linux）。

只编译、再手动烧录：

```bash
SSID=your-2.4ghz-ssid PASSWORD=your-wpa2-password \
  cargo build -p firmware --release --target riscv32imac-unknown-none-elf
espflash flash --monitor --chip esp32c6 \
  target/riscv32imac-unknown-none-elf/release/firmware
```

DHCP 成功后串口打印 `IP4=<addr>`，把 iperf3 指到该地址。

## 合同命令

单流、IPv4。把 `<ip>` 换成板子地址。已对桌面 iperf **3.21** 验证。

```bash
iperf3 -c <ip> -t 10
iperf3 -c <ip> -R -t 10
iperf3 -c <ip> -u -t 10
iperf3 -c <ip> -u -R -t 10
```

默认 RX；TX 用 `-R`。测完 server 继续听（daemon）。`-P 2` 会被拒绝（`SERVER_ERROR`）；固件不得崩溃，之后单流测试仍须可跑。

吞吐以**桌面 client** 摘要为准，固件不另记一套 Mbps。见 [`docs/hardware-test.md`](docs/hardware-test.md)。

## 目录结构

```text
iperf3-proto/          no_std 协议 crate（主机单测）
firmware/              ESP32-C6 STA、DHCP、socket
docs/design.md         合同（SSOT）
docs/hardware-test.md  板上结果，SSID 已打码
```

主机测试（不需要板）：

```bash
cargo test -p iperf3-proto
```

## 限制

- 无 5 GHz（ESP32-C6）。
- 无 WPA3（`esp-radio`）。请用 2.4 GHz WPA2（混合模式 AP 可以）。
- 不做 iperf3 **client**、SoftAP、APSTA。
- 不应答 ICMP ping；合同路径是 TCP 5201。
- 这套 Rust 栈的 Mbps **不能**对标 ESP-IDF C iperf / iperf2 官方表（协议不同、栈不同）。
- 不做 `-P>1`、`--bidir`、IPv6、SCTP、鉴权、静态 IP、mDNS。

## 贡献

欢迎 Issue 与 Pull Request。请保持改动聚焦，并遵循现有 crate 边界（`iperf3-proto` 与 `firmware`）。

## 许可证

双许可：[MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)，任选其一。
