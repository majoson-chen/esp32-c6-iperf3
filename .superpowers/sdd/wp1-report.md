<!--
  Author: Cursor Grok 4.6
  意图: WP1 执行报告（iperf3-proto TDD）。含 RED/GREEN、公开 API、验证摘要与自审。
-->

# WP1 Report — iperf3-proto（TDD，host）

**Status:** DONE_WITH_CONCERNS  
**Branch:** `main`  
**Date:** 2026-08-14  
**Protocol SSOT:** `docs/design.md` §3；线格式对照 esnet/iperf **3.21**（`iperf.h` / `iperf_api.h` / `iperf_api.c` / `iperf_udp.c`）

## Summary

`iperf3-proto` 从 `proto_version()` stub 换成 no_std + alloc 的 **iperf3 server 子集**：控制状态机、参数 JSON 接受/拒绝、结果 JSON、UDP datagram 头与 connect 常量。不碰 `firmware/`。主机 `cargo test -p iperf3-proto` 17 passed。

## TDD RED / GREEN

每条行为先写测再实现。下面是记录到的失败与转绿（节选；全程 `cargo test -p iperf3-proto`）。

| Slice | RED（失败原因符合预期） | GREEN |
|---|---|---|
| Cookie 37 + 状态字节 | `cannot find value COOKIE_SIZE` / `unresolved module state` | 3 passed（含既有 `proto_version`） |
| JSON 分帧 | `cannot find function encode_json_frame` | 4 passed |
| 参数 JSON | `cannot find type TestParams` | 8 passed |
| UDP `len:8` 拒绝 | `unwrap_err()` on `Ok`（故意先拿掉检查） | 随 params 测试转绿 |
| UDP 头 / connect 常量 | `cannot find struct UdpHeader` / `UDP_CONNECT_MSG` | 10 passed |
| 结果 JSON | `cannot find function encode_results_json` | 11 passed |
| 会话 + ACCESS_DENIED | `cannot find type Server` / `Io` / `Start` | 14 passed，后补 UDP 通道与 cookie mismatch → **17 passed** |

最终 GREEN：

```
running 17 tests
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Doc-tests iperf3_proto: 0 tests
```

覆盖 brief 要求：cookie 长度、参数接受/拒绝、TCP 状态字节顺序、ACCESS_DENIED、结果 JSON 可解析、UDP 头 32/64-bit roundtrip。另有：错误 cookie 长度、数据面 cookie 不匹配、UDP `NeedDataChannel` + connect 应答、`SERVER_ERROR` 线格式。

## Public API（WP2 只应依赖这些）

驱动模型：**不碰 socket**。固件 `poll()` → 按 `Io` 读写控制通道 / 接数据面 / 泵数据 → `feed_ctrl` / `data_ready` / `add_bytes`。

```
Server::new()
  start_session() -> Start::Accepted(Session) | Start::AccessDenied([u8;1])
  end_session(session)   // 消耗 Session；Drop 同样清 busy。forget 会话则永远 ACCESS_DENIED
  is_busy()

Session
  poll() -> Io::WriteCtrl(Vec<u8>)
           | ReadCtrl(usize)          // 精确读这么多字节再 feed_ctrl
           | NeedDataChannel { cookie, transport }
           | Pump                     // 同时盯控制通道；TEST_END 用 feed_ctrl(&[4])
           | Done
  feed_ctrl(&[u8])
  data_ready(&[u8])                   // TCP 数据连接 cookie；不匹配 -> CookieMismatch
                                      //   固件应在数据 socket 写 ACCESS_DENIED（0xFF）
  udp_connect_reply(&[u8]) -> [u8;4]  // 校验 UDP_CONNECT_MSG，返回 UDP_CONNECT_REPLY
  end_test()                          // reverse 到时，等价 TEST_END
  add_bytes(u64)
  note_udp_datagram(&[u8])            // 计数 + 3.21 序号缺口丢失
  params() / cookie() / stats()

TestParams::parse_json(&[u8])
encode_json_frame / decode_json_len
encode_results_json(&StreamStats, sender_has_retransmits)
UdpHeader::encode/decode(counters_64bit, buf)
UDP_CONNECT_MSG = b"9876"             // 3.21 线上字节（宏按端序对调后的内存布局）
UDP_CONNECT_REPLY = b"6789"
COOKIE_SIZE = 37
state::*  （PARAM_EXCHANGE=9 … ACCESS_DENIED=-1, SERVER_ERROR=-2）
```

**固件循环要点**

1. `start_session`；若 `AccessDenied` 把那一字节写进新控制连接并关掉。
2. `ReadCtrl(n)` 用 `read_exact`；`WriteCtrl` 写控制 socket。
3. 参数 `feed_ctrl` 成功后即可 `params()`，**先 listen 数据口再写出** `CREATE_STREAMS`（避免 client 抢连）。
4. TCP：accept → 读 37 字节 cookie → `data_ready`。UDP：`recvfrom` → `udp_connect_reply` → 把应答发回 → `data_ready(session.cookie())`。
5. `Pump`：正向收、reverse 发；UDP reverse 按 `bandwidth_bps` 限速；控制口读到 `TEST_END` 则 `feed_ctrl`。
6. `Done` 后 `end_session`，回到 listen。

拒绝参数时状态机写出 `SERVER_ERROR`（0xFE）+ 大端 `i32` 错误号 + 大端 `errno`（0）。`parallel>1` / sctp / bidir / UDP 过短用 `IEPROTOCOL=131`；坏 JSON 用 `IERECVPARAMS=114`。

## Verification summary

| 项 | 结果 |
|----|------|
| `cargo test -p iperf3-proto` | ✅ 17 passed，无 warning |
| `cargo clippy -p iperf3-proto --all-targets -- -D warnings` | ✅ |
| `cargo build -p iperf3-proto --target riscv32imac-unknown-none-elf` | ✅ no_std 可交叉编库 |
| 未改 `firmware/` | ✅ |
| 未 vendor esnet/iperf C 源 | ✅ |
| 未 push | ✅ |

未做：与桌面 `iperf3` 3.21 的真 socket 对打（属 WP2/WP3）。

## 3.21 线格式锚点（未发明）

- Cookie 37；JSON = BE u32 长度 + UTF-8（`JSON_write` / `JSON_read`）。
- UDP 头：`sec`+`usec` 各 BE u32，序号 32-bit `htonl` 或 64-bit `htobe64`（`iperf_udp_send/recv`）。
- Connect：3.21 `iperf.h` 按 `BYTE_ORDER` 对调宏，**线上**固定为 ASCII `"9876"` / `"6789"`；legacy reply 线上 `B1 68 DE 3A`。
- 结果对象：`send_results` 的 `cpu_util_*`、`sender_has_retransmits`、`streams[].id/bytes/retransmits/jitter/errors/packets`。未写 `omitted_*` / `start_time`（3.21 允许缺）。

## Self-review

**做得好的**

- 状态机与 socket 解耦，WP2 只能靠 `Io` 驱动。
- UDP 常量按 **线上字节** 而不是主机 `uint32_t` 宏，避免和 3.21 的端序对调再错一层。
- 参数拒绝走真 `SERVER_ERROR` 帧，而不是只返回 Rust `Err`。

**Concerns（非阻塞，WP2 必须看见）**

1. **busy 释放：** `end_session` 或 drop 最后一轮 `Session` 都会清 busy。`mem::forget` 会话仍会永远 `ACCESS_DENIED`。固件在 `SERVER_ERROR` / `Done` 后仍应显式 `end_session`。
2. **`IEPROTOCOL`（131）用于 `-P 2` / bidir / sctp。** 桌面 client 会打印 “Protocol does not exist”，语义略歪，但不影响拒绝且不崩溃。
3. **`serde_json` + `alloc` 会进固件镜像。** 对 C6 flash 通常可接受；若 WP2 体积紧张再换成手写扫描，公开 `TestParams::parse_json` 形状不变。
4. **jitter 固定 0。** `note_udp_datagram` 只做序号/丢失，没做 RFC 1889。吞吐权威在 client，可接受。
5. **Pump 时 `poll()` 反复返回 `Pump`。** 固件必须自己 `select` 数据+控制；不要把 `Pump` 当成阻塞直到结束。
6. **未测 `end_test()` 的独立用例**（实现已有）。WP2 reverse 超时应走这条，不要只靠 `TEST_END`。

## Out of scope（故意不做）

- Client 角色、多流、bidir、IPv6、SCTP、鉴权
- Wi‑Fi / embassy / 真 socket
- README、CI、GitHub push
- 把 esnet/iperf C 文件拷进仓库

## Files touched

- `iperf3-proto/Cargo.toml`（`serde_json` alloc）
- `Cargo.lock`
- `iperf3-proto/src/lib.rs`（公开面 + 单测）
- `iperf3-proto/src/{state,frame,params,udp,results,session}.rs`（新）
- `.superpowers/sdd/wp1-report.md`（本文件）

## Important review fix（2026-08-14）

修正两项 Important：

1. **busy / `end_session`：** `Session` 持有 `Rc<Cell<bool>>`，`Drop` 清 busy（`poll` 语义不变）。rustdoc 写明：既不 `end_session` 也不 drop（`mem::forget`）→ 永远 `ACCESS_DENIED`。
2. **`SERVER_ERROR` 载荷：** `-P 2` / sctp / bidir 拒绝后断言首字节 `-2`，随后两个 BE i32：`IEPROTOCOL=131`、errno `0`。

覆盖测试：`rejected_params_send_server_error`、`dropping_last_session_clears_busy`、`end_session_clears_busy`、`busy_server_returns_access_denied`（持有会话时重叠仍拒绝）。

TDD：`dropping_last_session_clears_busy` RED = `assertion failed: !server.is_busy()`；实现 Drop 后 GREEN。payload 断言锁已有线格式（先写断言，实现未改 `queue_server_error`）。

### `cargo test -p iperf3-proto`

```
running 19 tests
test tests::cookie_is_37_bytes ... ok
test tests::dropping_last_session_clears_busy ... ok
test tests::busy_server_returns_access_denied ... ok
test tests::cookie_feed_rejects_wrong_length ... ok
test tests::json_frame_is_be_length_then_utf8 ... ok
test tests::end_session_clears_busy ... ok
test tests::data_cookie_mismatch_is_error ... ok
test tests::happy_tcp_control_state_order ... ok
test tests::params_accept_default_tcp ... ok
test tests::params_accept_udp_reverse_and_64bit ... ok
test tests::params_ignore_omit_window_mss_nodelay ... ok
test tests::params_reject_sctp_parallel_bidir ... ok
test tests::proto_version_is_nonempty ... ok
test tests::rejected_params_send_server_error ... ok
test tests::result_json_has_required_3_21_fields ... ok
test tests::state_bytes_match_iperf_3_21 ... ok
test tests::udp_header_roundtrip_32bit_and_64bit ... ok
test tests::udp_connect_constants_are_3_21_wire_bytes ... ok
test tests::udp_params_need_udp_channel_and_connect_reply ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests iperf3_proto
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo clippy -p iperf3-proto --all-targets -- -D warnings`：通过。
