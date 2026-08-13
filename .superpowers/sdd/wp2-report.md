<!--
  Author: Cursor Grok 4.6
  意图: WP2 执行报告（firmware STA + iperf3-proto server）。含钉版本、验证、listen-before-CREATE_STREAMS 锚点与自审。
-->

# WP2 Report — firmware STA + iperf3 server loop

**Status:** DONE_WITH_CONCERNS  
**Branch:** `main`  
**Date:** 2026-08-14  
**Design SSOT:** `docs/design.md` §2 / §4  
**Bring-up SSOT:** crates.io `esp-hal` **1.1.2** 对齐 GitHub tag **`esp-hal-v1.1.0`** 的 `examples/wifi/embassy_dhcp`；缓冲对齐同 tag `qa-test/src/bin/embassy_wifi_bench.rs`（16 KiB）。未抄 `esp-hal` `main`。

## Summary

`firmware` 从 panic-loop stub 换成 ESP32-C6 STA daemon：官方 1.1.x Wi‑Fi 栈（`esp-radio` 0.18.0 + `esp-rtos` 0.3.0 + `embassy-net` 0.9）关联 AP、DHCPv4、打印 `IP4=`，在 **一个** embassy 任务里驱动 `iperf3-proto` 的 `Server`/`Session`（`!Send`）。TCP/UDP、正向/reverse 都接到状态机。未烧录（WP3）。

## Crate versions pinned（crates.io，非 git main）

| Crate | Req | Locked | 来源 |
|---|---|---|---|
| `esp-hal` | 1.1.2 | 1.1.2 | 官方 1.1.x 补丁；`esp-radio` 0.18 依赖 `~1.1.0-rc.0` |
| `esp-radio` | 0.18.0 | 0.18.0 | 与 `esp-hal-v1.1.0` example 同期 |
| `esp-rtos` | 0.3.0 | 0.3.0 | 同上；features `embassy` + `esp-radio` |
| `esp-alloc` | 0.10.0 | 0.10.0 | dhcp example：reclaimed 64KiB + extra 36KiB |
| `esp-backtrace` | 0.19.0 | 0.19.0 | panic-handler + println + `esp32c6` |
| `esp-println` | 0.17.0 | 0.17.0 | `esp32c6` |
| `esp-bootloader-esp-idf` | 0.5.0 | 0.5.0 | `esp_app_desc!` |
| `embassy-net` | 0.9.0 | **0.9.1** | tcp+udp+dhcpv4+medium-ethernet |
| `embassy-executor` | 0.10.0 | 0.10.0 | `#[embassy_executor::task]` |
| `embassy-time` | 0.5.0 | 0.5.1 | |
| `embassy-futures` | 0.1 | 0.1.2 | `join` / `select` |
| `static_cell` | 2.1.0 | 2.1.1 | 与官方 `mk_static!` 相同 |
| `iperf3-proto` | path | 0.1.0 | WP1 |

Bring-up API 按 **1.1.0 tag**，不是 `main`：`esp_radio::wifi::new` → `interfaces.station`（`main` 上已改成 `Interface::station()` + `WifiController::new`）。省电：`PowerSaveMode::None`（bench）。CPU：`CpuClock::max()`。凭证：`env!("SSID")` / `env!("PASSWORD")`，源码无字面量。

Release：`opt-level = 3`、`lto = "fat"`、`codegen-units = 1`、`debug = 2`（对齐 qa-test；ELF 含调试信息）。`.cargo/config.toml` 仅给 riscv target 加 `-Tlinkall.x` + `force-frame-pointers`，**不**设默认 target，以免 host `cargo test` 被交叉。

## listen-before-CREATE_STREAMS

协议在 `feed_ctrl(params JSON)` 后会把 `CREATE_STREAMS`（0x0A）排进 `Session.out`；下一次 `poll()` 就是 `WriteCtrl`。若先写再 listen，client SYN 会丢。

实现：`firmware/src/server.rs` 的 `open_data_before_create_streams`：

- **TCP** `firmware/src/server.rs:179`：`join(data.accept(5201), write_create_streams)`。`embassy-futures` 0.1 的 `Join` 先 poll 左操作数；`embassy-net` 0.9 `TcpSocket::accept` 在第一个 `.await` 之前同步调用 smoltcp `listen()`。因此 LISTEN 发生在写出 0x0A 之前。
- **UDP** `firmware/src/server.rs:206-207`：`udp.bind(5201)?` 同步，然后才 `write_create_streams`。

`Server`/`Session` 只活在 `main` 任务里的 `serve_while_up`（`firmware/src/main.rs` 注释 + `server.rs`），不 spawn。

## Verification summary

未烧录。Host 不能跑固件。WP2 通道 = 交叉编译 + proto 单测。

### `SSID=x PASSWORD=x cargo build -p firmware --release --target riscv32imac-unknown-none-elf`

首次缺 `-Tlinkall.x` 时 rust-lld 报 `_stack_end_cpu0` / `DefaultHandler` 等（官方脚本符号）。补上 rustflags 后：

```
   Compiling firmware v0.1.0 (/Users/majoson/CodeSpace/esp32-c6-iperf3/firmware)
    Finished `release` profile [optimized + debuginfo] target(s) in 14.27s
```

exit 0。产物 `target/riscv32imac-unknown-none-elf/release/firmware`（ELF ≈ 8.0M，含 `debug = 2`；负载段远小于此）。`rustc 1.97.1`；sysroot 已有 `liballoc`，未开 `build-std`。

### `cargo test -p iperf3-proto`

```
running 19 tests
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Doc-tests iperf3_proto: 0 tests
```

未 push；未 commit `.env` / 真实密码；SSID/PASSWORD 仅 `env!`。

## Acceptance checklist

| 项 | 结果 |
|---|---|
| crates.io 钉死 + 按该版本官方 STA+DHCP 写 bring-up | ✅ tag `esp-hal-v1.1.0` / crate 1.1.2 |
| STA only，`env!("SSID")`/`PASSWORD`，无源码密码 | ✅ |
| 省电关、CPU max、DHCPv4、`IP4=` | ✅ |
| 断线：丢 socket，connection 任务重试，IP 回来再 listen | ✅ `serve_while_up` 返回后 main 再 `wait_config_up` |
| embassy-net TCP+UDP，16KiB 级缓冲，dhcp 级堆 | ✅ |
| listen TCP 5201；单任务驱动 proto | ✅ |
| listen 先于 CREATE_STREAMS | ✅ `:179` / `:206-207` |
| TCP cookie / UDP hello / Pump / `end_session` | ✅ |
| 未烧录、未 README/CI、未 AP/client/多流 | ✅ |
| 交叉编译 + proto 19 passed | ✅ |

## Self-review

**做得好的**

- 版本钉在已发布的 1.1.x 组合，bring-up 从 tag 复制，避开 `main` 上已改的 `WifiController::new`。
- `join(accept, write)` 的 poll 顺序对照了 `embassy-futures` 0.1 源码和 `accept()` 里同步 `listen()`。
- 链接失败没有装成 host-only stub；补官方 `-Tlinkall.x` 后真实 Wi‑Fi 栈过了 rust-lld。

**Concerns（非阻塞；WP3 板上会暴露）**

1. **测中 ACCESS_DENIED：** 一次测试占用控制 socket 后不再另开 listen。重叠的第二路 `iperf3 -c` 拿不到 0xFF，只会连不上。`-P 2` 仍走同一控制连接的 `SERVER_ERROR`（设计合同）。WP3 若要测「测中再连」，需加第二颗 listen socket。
2. **泵缓冲 4KiB。** UDP reverse 的 datagram 被 clamp 到 `PUMP_BUF`；默认 1460 没问题，超大 `len` 会截断。
3. **ELF 8MB** 来自 `debug = 2`。`espflash` 只烧负载；若 WP4 CI 产物要小，可改 `debug = false`。
4. **测中第二 listen 与数据口同 5201** 在 smoltcp 上不好做；这是 embassy-net「listen+accept 共用一颗 socket」的模型限制，不是漏写 `Start::AccessDenied` 分支（accept 后若仍 busy 会写那一字节）。
5. **未与桌面 iperf3 对打**（WP3）。

## Out of scope（故意不做）

- 烧录 / 硬件合同命令 / `docs/hardware-test.md`（WP3）
- README、CI、GitHub push（WP4）
- AP / client / `-P>1` 数据面 / bidir

## Files touched

- `firmware/Cargo.toml`、`firmware/src/main.rs`、`firmware/src/server.rs`（新）
- `.cargo/config.toml`（新）
- `Cargo.toml`（`[profile.release]`）
- `Cargo.lock`
- `.superpowers/sdd/wp2-report.md`（本文件）
