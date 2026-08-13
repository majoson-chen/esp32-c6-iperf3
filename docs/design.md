<!--
  Author: Cursor Grok 4.6
  意图: 本仓库的设计合同（SSOT）。实现、README、CI、硬件验收都以本文为准。
-->

# ESP32-C6 iperf3 server（esp-hal）设计

公开的 no_std 固件：上电以 STA 加入 2.4 GHz AP，在 TCP 5201 上跑 **iperf3 server 子集**，供桌面 `iperf3` 测试该模组在这套 Rust 栈上的 Wi‑Fi 吞吐。

## 1. 目标与非目标

### 做

- 芯片：ESP32-C6。栈：`esp-hal` + `esp-radio` + `embassy-net`（smoltcp），`no_std`。
- 上电：STA 关联 → DHCP → 串口打印 IPv4 → 监听 **5201**，daemon（测完继续听）。
- 对端：桌面 [esnet/iperf](https://github.com/esnet/iperf) 3.x client（本机已验证 3.21）。
- 合同命令（单流、IPv4）：
  - `iperf3 -c <ip> -t 10`
  - `iperf3 -c <ip> -R -t 10`
  - `iperf3 -c <ip> -u -t 10`
  - `iperf3 -c <ip> -u -R -t 10`
  - 以上各命令连续两次成功
  - `iperf3 -c <ip> -P 2` 在控制通道被拒绝，server 不崩溃
- 固件只做 **server**。RX 默认、TX 靠 `-R`。
- 开源：公开 GitHub 仓库、MIT OR Apache-2.0、中英 README、主机单测 CI、交叉编译 CI。

### 不做

- C6 上的 iperf3 client
- SoftAP / APSTA（v1）
- 静态 IP、mDNS、串口配网、掉电保存密码
- `-P>1`、`--bidir`、IPv6、SCTP、RSA/用户名鉴权、文件源、zerocopy
- 服务端第二套 Mbps 账本（权威数字在桌面 client）
- 对标 ESP-IDF C iperf / iperf2 的官方吞吐表（协议不同、栈不同）
- 把 SSID/密码提交进 git
- 现在不上 crates.io

## 2. 架构

两层，协议与芯片解耦，这样主机才能测协议、CI 才不需要板子。

```
┌─────────────────────────────────────────┐
│ firmware（riscv32imac-unknown-none-elf） │
│  Wi‑Fi STA / DHCP / 串口 / TCP·UDP socket │
└──────────────────┬──────────────────────┘
                   │ 字节流 in / 状态机 out
┌──────────────────▼──────────────────────┐
│ iperf3-proto（no_std，也可 host 编译）     │
│  cookie / JSON 参数 / 状态字节 / 结果 JSON │
└─────────────────────────────────────────┘
```

- `iperf3-proto`：纯协议。不碰 Wi‑Fi、不碰 embassy。`std` 下用内存缓冲做单测。
- `firmware`：把 socket 读写接到状态机；负责关联、重试、一次一测、生命周期日志。

发布形态：Cargo workspace。固件是 binary crate，协议是 library crate。依赖只用来自 **crates.io 的已发布版本**，对齐当时官方 example 的 crate 组合；不跟踪 `esp-hal` 的 `main`（API 正在改）。

## 3. iperf3 协议合同

控制连接始终是 TCP。数据流另开。状态是 **单个 signed byte**。带 JSON 的消息：先发 32-bit 大端长度，再发 UTF-8 JSON。

状态取值（esnet/iperf `iperf_api.h`）：

| 值 | 名字 | 谁发给谁 |
|---:|---|---|
| 9 | `PARAM_EXCHANGE` | server → client，随后 client 发参数 JSON |
| 10 | `CREATE_STREAMS` | server → client，随后 client 建数据连接 |
| 1 | `TEST_START` | server → client |
| 2 | `TEST_RUNNING` | server → client，之后才泵数据 |
| 4 | `TEST_END` | client → server |
| 13 | `EXCHANGE_RESULTS` | server → client，随后双方各发一份结果 JSON |
| 14 | `DISPLAY_RESULTS` | server → client |
| 16 | `IPERF_DONE` | client → server |
| -1 | `ACCESS_DENIED` | 已有测试在跑 |
| -2 | `SERVER_ERROR` | 再跟两个 32-bit：iperf 错误号、errno |

### 正常时序（server 视角）

1. `accept` 控制连接，读 37 字节 cookie。
2. 若已有测试：发 `ACCESS_DENIED`，关连接。
3. 发 `PARAM_EXCHANGE`，读长度+JSON，解析为 `TestParams`。
4. 不支持则发 `SERVER_ERROR`（或等价拒绝）并结束；支持则发 `CREATE_STREAMS`。
5. `accept` **一条** 数据连接（TCP）或 UDP 流；数据连接开头必须再发同一 cookie。
6. 发 `TEST_START`，再发 `TEST_RUNNING`。
7. 泵数据直到控制通道收到 `TEST_END`，或 duration/bytes 到时（reverse 时由 server 停）。
8. 发 `EXCHANGE_RESULTS`，读 client JSON，写 server JSON。
9. 发 `DISPLAY_RESULTS`，等到 `IPERF_DONE` 或对端关闭。
10. 关掉本轮 socket，回到 listen。

### 参数 JSON（client → server）

解析需要用到的字段（其余忽略）：

| 字段 | 含义 | v1 规则 |
|---|---|---|
| `tcp` / `udp` / `sctp` | 协议 | `sctp` 拒绝；默认 TCP |
| `parallel` | 流数 | 缺省 1；`>1` 拒绝 |
| `reverse` | 反向 | 支持 |
| `bidirectional` / `bidir` | 双向同时 | 拒绝 |
| `time` | 秒 | 缺省 10 |
| `len` | block size | TCP 可用默认；UDP 必须能装 datagram 头 |
| `bandwidth` | UDP 目标码率 | reverse-UDP 发送时遵守；正向 UDP 只收 |
| `omit` | 跳过前 N 秒统计 | 忽略（仍跑满 duration） |
| `window` / `MSS` / `nodelay` | 套接字调参 | 忽略 |
| `udp_counters_64bit` | UDP 计数宽度 | 能则跟 client，否则按 32-bit |

### 结果 JSON（server → client）

必须是 client 3.21 能吃完、不挂死的最小合法对象。至少包含：

- `cpu_util_total` / `cpu_util_user` / `cpu_util_system`（可填 0）
- `sender_has_retransmits`（0）
- `streams[]`：`id`、`bytes`、`retransmits`、`jitter`、`errors`、`packets`

精确键名在实现时对照 3.21 的 `iperf_exchange_results` / `JSON_write`，以能跑完 client 为准，不发明第二套字段。

### 数据面

- **TCP 正向**：server 读到 EOF/`TEST_END`，计数 bytes。
- **TCP reverse**：server 写满 duration（或 client 关连接），payload 任意重复模式。
- **UDP**：datagram 布局跟 3.21 一致（时间戳 + 序号；64-bit 计数可选）。reverse-UDP 先完成 3.21 的 connect 握手（`UDP_CONNECT_MSG` / `UDP_CONNECT_REPLY`），再按 `bandwidth` 限速发送。
- 一次只服务一个测试。控制连接占用期间，新控制连接 → `ACCESS_DENIED`。

## 4. Wi‑Fi 与配置

- 模式：STA only。省电关闭。CPU 拉到芯片允许的最高频。
- 凭证：编译期环境变量 `SSID`、`PASSWORD`（与官方 example 一致）。仓库提供 `.env.example`，真实 `.env` gitignore。
- 地址：DHCPv4。串口打印一行可扫的 `IP4=<addr>`，以及关联/断线/测试开始结束/错误。
- 断线：关掉本轮数据和控制 socket，一直重试 `connect`；再次拿到 IP 后重新 listen。
- 认证：WPA2-PSK。C6 无 5 GHz；`esp-radio` 无 WPA3。测试 AP 必须是 2.4 GHz 且允许 WPA2（混合模式可以）。
- 本机开发网：电脑可以在 5 GHz / 以太网，只要与 C6 同一 L3 网段即可打 5201。

## 5. 日志与结果

- 串口：生命周期，不计算 Mbps。
- 吞吐：桌面 `iperf3` 输出为权威。
- 硬件测试报告：`docs/hardware-test.md`。写吞吐、信道、通过/失败、iperf 版本、固件 commit。**SSID 打码，密码永不出现。**

## 6. 开源与 CI

- 仓库：`majoson-chen/esp32-c6-iperf3`，public。
- 许可：MIT OR Apache-2.0（与 `stm32f103-serial-rs` 对齐）。
- README.md + README.zh-CN.md：构建、烧录、环境变量、合同命令、已知限制（无 WPA3、无 5 GHz、Rust 栈数字低于 IDF）。
- CI（无硬件）：
  1. `cargo test -p iperf3-proto`（host）
  2. `cargo build -p firmware --release --target riscv32imac-unknown-none-elf`（可用 dummy `SSID`/`PASSWORD`）
- 不把硬件测试塞进 GitHub Actions。

## 7. 验收

主机：协议单测覆盖 cookie、参数解析（含拒绝规则）、状态字节顺序、结果 JSON 可解析。

硬件（开发机，桌面 iperf 3.21，C6 连 2.4 GHz WPA2）：§1 合同命令全部通过；daemon 连续两次；`-P 2` 拒绝且仍可再测。

刷写会覆盖板上现有 MicroPython。烧录占用 `/dev/cu.usbmodem*`，须先释放 Thonny 等占用者。

## 8. 风险

- `esp-hal` / `esp-radio` 的 STA API 在 1.0 example 与 `main` 之间不兼容。实现必须钉死 crates.io 版本并按该版本官方 example 写 bring-up。
- smoltcp 窗口和 Wi‑Fi 缓冲会限制吞吐。v1 先协议正确，缓冲对齐官方 `embassy_wifi_bench` 量级；不把「接近 IDF 20 Mbps」当协议验收。
- UDP reverse 的握手和限速是最容易和 3.21 对不齐的部分；实现时对着 3.21 源码，而不是 wiki 的 2014 年描述。
