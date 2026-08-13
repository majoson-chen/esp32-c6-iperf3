// Author: Cursor Grok 4.6
// Purpose: iperf 3.21 control-channel signed state bytes (iperf_api.h).

/// PARAM_EXCHANGE — server → client，随后 client 发参数 JSON。
pub const PARAM_EXCHANGE: i8 = 9;
/// CREATE_STREAMS — server → client，随后 client 建数据连接。
pub const CREATE_STREAMS: i8 = 10;
/// TEST_START — server → client。
pub const TEST_START: i8 = 1;
/// TEST_RUNNING — server → client，之后才泵数据。
pub const TEST_RUNNING: i8 = 2;
/// TEST_END — client → server。
pub const TEST_END: i8 = 4;
/// EXCHANGE_RESULTS — server → client，随后双方各发一份结果 JSON。
pub const EXCHANGE_RESULTS: i8 = 13;
/// DISPLAY_RESULTS — server → client。
pub const DISPLAY_RESULTS: i8 = 14;
/// IPERF_DONE — client → server。
pub const IPERF_DONE: i8 = 16;
/// ACCESS_DENIED — 已有测试在跑。
pub const ACCESS_DENIED: i8 = -1;
/// SERVER_ERROR — 再跟两个 32-bit 大端：iperf 错误号、errno。
pub const SERVER_ERROR: i8 = -2;
