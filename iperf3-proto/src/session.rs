// Author: Cursor Grok 4.6
// Purpose: iperf3 server 子集控制状态机：字节进、Io 命令出，不碰 socket。

use alloc::vec::Vec;

use crate::COOKIE_SIZE;
use crate::frame::{MAX_PARAMS_JSON, decode_json_len, encode_json_frame};
use crate::params::{ParamsError, TestParams, Transport};
use crate::results::{StreamStats, encode_results_json};
use crate::state;
use crate::udp::{UDP_CONNECT_REPLY, is_udp_connect_msg};

/// 3.21 `IEPROTOCOL`：不支持的协议/参数。
pub const IEPROTOCOL: i32 = 131;
/// 3.21 `IERECVPARAMS`。
pub const IERECVPARAMS: i32 = 114;

const MAX_RESULTS_JSON: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Io {
    WriteCtrl(Vec<u8>),
    ReadCtrl(usize),
    NeedDataChannel {
        cookie: [u8; COOKIE_SIZE],
        transport: Transport,
    },
    Pump,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    CookieLen,
    Frame,
    Unexpected,
    CookieMismatch,
    UdpHello,
}

#[derive(Debug)]
pub enum Start {
    Accepted(Session),
    AccessDenied([u8; 1]),
}

pub struct Server {
    busy: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self { busy: false }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn start_session(&mut self) -> Start {
        if self.busy {
            return Start::AccessDenied([state::ACCESS_DENIED as u8]);
        }
        self.busy = true;
        Start::Accepted(Session::new())
    }

    pub fn end_session(&mut self, _session: Session) {
        self.busy = false;
    }
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    WaitCookie,
    WaitJsonLen,
    WaitJsonBody { len: usize },
    NeedData,
    Pump,
    WaitClientLen,
    WaitClientBody { len: usize },
    WaitDone,
    Done,
}

#[derive(Debug)]
pub struct Session {
    phase: Phase,
    out: Vec<u8>,
    cookie: [u8; COOKIE_SIZE],
    params: Option<TestParams>,
    stats: StreamStats,
}

impl Session {
    fn new() -> Self {
        Self {
            phase: Phase::WaitCookie,
            out: Vec::new(),
            cookie: [0; COOKIE_SIZE],
            params: None,
            stats: StreamStats::default(),
        }
    }

    pub fn poll(&mut self) -> Io {
        if !self.out.is_empty() {
            return Io::WriteCtrl(core::mem::take(&mut self.out));
        }
        match self.phase {
            Phase::WaitCookie => Io::ReadCtrl(COOKIE_SIZE),
            Phase::WaitJsonLen | Phase::WaitClientLen => Io::ReadCtrl(4),
            Phase::WaitJsonBody { len } | Phase::WaitClientBody { len } => Io::ReadCtrl(len),
            Phase::NeedData => Io::NeedDataChannel {
                cookie: self.cookie,
                transport: self.params.map(|p| p.transport).unwrap_or(Transport::Tcp),
            },
            Phase::Pump => Io::Pump,
            Phase::WaitDone => Io::ReadCtrl(1),
            Phase::Done => Io::Done,
        }
    }

    pub fn feed_ctrl(&mut self, data: &[u8]) -> Result<(), SessionError> {
        match self.phase {
            Phase::WaitCookie => {
                if data.len() != COOKIE_SIZE {
                    return Err(SessionError::CookieLen);
                }
                self.cookie.copy_from_slice(data);
                self.out.push(state::PARAM_EXCHANGE as u8);
                self.phase = Phase::WaitJsonLen;
            }
            Phase::WaitJsonLen => {
                let len = read_json_len(data, MAX_PARAMS_JSON)?;
                self.phase = Phase::WaitJsonBody { len };
            }
            Phase::WaitJsonBody { len } => {
                if data.len() != len {
                    return Err(SessionError::Frame);
                }
                match TestParams::parse_json(data) {
                    Ok(p) => {
                        self.params = Some(p);
                        self.out.push(state::CREATE_STREAMS as u8);
                        self.phase = Phase::NeedData;
                    }
                    Err(e) => {
                        self.queue_server_error(e);
                        self.phase = Phase::Done;
                    }
                }
            }
            Phase::Pump => {
                if data.len() != 1 {
                    return Err(SessionError::Frame);
                }
                if data[0] as i8 != state::TEST_END {
                    return Err(SessionError::Unexpected);
                }
                self.begin_exchange_results();
            }
            Phase::WaitClientLen => {
                let len = read_json_len(data, MAX_RESULTS_JSON)?;
                self.phase = Phase::WaitClientBody { len };
            }
            Phase::WaitClientBody { len } => {
                if data.len() != len {
                    return Err(SessionError::Frame);
                }
                let shr = if self.params.map(|p| p.reverse).unwrap_or(false) {
                    0
                } else {
                    -1
                };
                let json = encode_results_json(&self.stats, shr);
                self.out.extend_from_slice(&encode_json_frame(&json));
                self.out.push(state::DISPLAY_RESULTS as u8);
                self.phase = Phase::WaitDone;
            }
            Phase::WaitDone => {
                if data.len() != 1 {
                    return Err(SessionError::Frame);
                }
                if data[0] as i8 != state::IPERF_DONE {
                    return Err(SessionError::Unexpected);
                }
                self.phase = Phase::Done;
            }
            Phase::NeedData | Phase::Done => return Err(SessionError::Unexpected),
        }
        Ok(())
    }

    pub fn data_ready(&mut self, cookie: &[u8]) -> Result<(), SessionError> {
        if !matches!(self.phase, Phase::NeedData) {
            return Err(SessionError::Unexpected);
        }
        if cookie != self.cookie.as_slice() {
            return Err(SessionError::CookieMismatch);
        }
        self.out.push(state::TEST_START as u8);
        self.out.push(state::TEST_RUNNING as u8);
        self.phase = Phase::Pump;
        Ok(())
    }

    /// UDP 数据面握手：client 发 `UDP_CONNECT_MSG`，server 回 `UDP_CONNECT_REPLY`。
    pub fn udp_connect_reply(&self, datagram: &[u8]) -> Result<[u8; 4], SessionError> {
        if !is_udp_connect_msg(datagram) {
            return Err(SessionError::UdpHello);
        }
        Ok(UDP_CONNECT_REPLY)
    }

    /// reverse 到时或对端关连接：等价于收到 TEST_END。
    pub fn end_test(&mut self) -> Result<(), SessionError> {
        if !matches!(self.phase, Phase::Pump) {
            return Err(SessionError::Unexpected);
        }
        self.begin_exchange_results();
        Ok(())
    }

    pub fn add_bytes(&mut self, n: u64) {
        self.stats.bytes = self.stats.bytes.saturating_add(n);
    }

    pub fn note_udp_datagram(&mut self, payload: &[u8]) {
        self.stats.bytes = self.stats.bytes.saturating_add(payload.len() as u64);
        let Some(p) = self.params else {
            return;
        };
        if p.transport != Transport::Udp {
            return;
        }
        let Ok(h) = crate::UdpHeader::decode(p.udp_counters_64bit, payload) else {
            return;
        };
        // 与 3.21 相同：序号前进记最高值，缺口记丢失。
        let prev = self.stats.packets as u64;
        if h.packet_count >= prev.saturating_add(1) {
            if h.packet_count > prev.saturating_add(1) {
                let gap = h.packet_count - 1 - prev;
                self.stats.errors = self.stats.errors.saturating_add(gap as i64);
            }
            self.stats.packets = h.packet_count as i64;
        } else if self.stats.errors > 0 {
            self.stats.errors -= 1;
        }
    }

    pub fn params(&self) -> Option<TestParams> {
        self.params
    }

    pub fn cookie(&self) -> &[u8; COOKIE_SIZE] {
        &self.cookie
    }

    pub fn stats(&self) -> StreamStats {
        self.stats
    }

    fn begin_exchange_results(&mut self) {
        self.out.push(state::EXCHANGE_RESULTS as u8);
        self.phase = Phase::WaitClientLen;
    }

    fn queue_server_error(&mut self, err: ParamsError) {
        let code = match err {
            ParamsError::InvalidJson => IERECVPARAMS,
            ParamsError::Sctp
            | ParamsError::Parallel
            | ParamsError::Bidir
            | ParamsError::UdpTooSmall => IEPROTOCOL,
        };
        self.out.push(state::SERVER_ERROR as u8);
        self.out.extend_from_slice(&code.to_be_bytes());
        self.out.extend_from_slice(&0i32.to_be_bytes());
    }
}

fn read_json_len(data: &[u8], max: usize) -> Result<usize, SessionError> {
    let (len, _) = decode_json_len(data).map_err(|_| SessionError::Frame)?;
    if data.len() != 4 || len == 0 || len > max {
        return Err(SessionError::Frame);
    }
    Ok(len)
}
