// Author: Cursor Grok 4.6
// Purpose: 解析 iperf 3.21 client 参数 JSON，套用 v1 接受/拒绝规则。

use serde_json::Value;

/// TCP 默认块长（iperf_api.h `DEFAULT_TCP_BLKSIZE`）。
pub const DEFAULT_TCP_BLKSIZE: u32 = 128 * 1024;
/// UDP 默认 datagram 长（`DEFAULT_UDP_BLKSIZE`）。
pub const DEFAULT_UDP_BLKSIZE: u32 = 1460;
/// UDP 默认目标码率 bit/s（iperf.h `UDP_RATE`）。
pub const DEFAULT_UDP_RATE_BPS: u64 = 1024 * 1024;
/// 缺省测试时长秒（`DURATION`）。
pub const DEFAULT_DURATION_SECS: u32 = 10;
/// UDP 头最小长度：sec + usec + 64-bit 序号（iperf.h `MIN_UDP_BLOCKSIZE`）。
pub const MIN_UDP_BLOCKSIZE: u32 = 4 + 4 + 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamsError {
    InvalidJson,
    Sctp,
    Parallel,
    Bidir,
    UdpTooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestParams {
    pub transport: Transport,
    pub reverse: bool,
    pub time_secs: u32,
    pub parallel: u32,
    pub blksize: u32,
    pub bandwidth_bps: u64,
    pub udp_counters_64bit: bool,
}

impl TestParams {
    pub fn parse_json(json: &[u8]) -> Result<Self, ParamsError> {
        let v: Value = serde_json::from_slice(json).map_err(|_| ParamsError::InvalidJson)?;
        if !v.is_object() {
            return Err(ParamsError::InvalidJson);
        }

        if json_true(&v, "sctp") {
            return Err(ParamsError::Sctp);
        }
        if json_true(&v, "bidirectional") || json_true(&v, "bidir") {
            return Err(ParamsError::Bidir);
        }

        let parallel = json_u32(&v, "parallel").unwrap_or(1);
        if parallel != 1 {
            return Err(ParamsError::Parallel);
        }

        let transport = if json_true(&v, "udp") {
            Transport::Udp
        } else {
            Transport::Tcp
        };

        let time_secs = json_u32(&v, "time").unwrap_or(DEFAULT_DURATION_SECS);
        let reverse = json_true(&v, "reverse");
        let udp_counters_64bit = json_u32(&v, "udp_counters_64bit").unwrap_or(0) != 0;

        let blksize = match json_u32(&v, "len") {
            Some(n) if n > 0 => n,
            _ => match transport {
                Transport::Tcp => DEFAULT_TCP_BLKSIZE,
                Transport::Udp => DEFAULT_UDP_BLKSIZE,
            },
        };

        if transport == Transport::Udp && blksize < MIN_UDP_BLOCKSIZE {
            return Err(ParamsError::UdpTooSmall);
        }

        let bandwidth_bps = match json_u64(&v, "bandwidth") {
            Some(n) if n > 0 => n,
            _ => match transport {
                Transport::Udp => DEFAULT_UDP_RATE_BPS,
                Transport::Tcp => 0,
            },
        };

        Ok(Self {
            transport,
            reverse,
            time_secs,
            parallel,
            blksize,
            bandwidth_bps,
            udp_counters_64bit,
        })
    }
}

fn json_true(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool) == Some(true)
}

fn json_u64(v: &Value, key: &str) -> Option<u64> {
    let n = v.get(key)?;
    n.as_u64()
        .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
        .or_else(|| n.as_f64().and_then(|f| (f >= 0.0).then_some(f as u64)))
}

fn json_u32(v: &Value, key: &str) -> Option<u32> {
    json_u64(v, key).and_then(|n| u32::try_from(n).ok())
}
