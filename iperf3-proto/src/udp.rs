// Author: Cursor Grok 4.6
// Purpose: iperf 3.21 UDP datagram 头编解码与 connect 握手常量（线上字节序）。

/// 客户端 UDP "connect" 报文线上字节（ASCII "9876"）。
pub const UDP_CONNECT_MSG: [u8; 4] = *b"9876";
/// 服务端 UDP "accept" 应答线上字节（ASCII "6789"）。
pub const UDP_CONNECT_REPLY: [u8; 4] = *b"6789";
/// 旧服务端应答（987654321 的小端内存布局，与 3.21 BE 宏一致）。
pub const LEGACY_UDP_CONNECT_REPLY: [u8; 4] = [0xB1, 0x68, 0xDE, 0x3A];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpHeader {
    pub sec: u32,
    pub usec: u32,
    pub packet_count: u64,
}

impl UdpHeader {
    /// 32-bit 模式 12 字节，64-bit 模式 16 字节；时间戳与序号均为大端。
    pub fn encoded_len(counters_64bit: bool) -> usize {
        if counters_64bit { 16 } else { 12 }
    }

    pub fn encode(&self, counters_64bit: bool, out: &mut [u8]) -> Result<usize, UdpError> {
        let n = Self::encoded_len(counters_64bit);
        if out.len() < n {
            return Err(UdpError);
        }
        out[0..4].copy_from_slice(&self.sec.to_be_bytes());
        out[4..8].copy_from_slice(&self.usec.to_be_bytes());
        if counters_64bit {
            out[8..16].copy_from_slice(&self.packet_count.to_be_bytes());
        } else {
            let pc = self.packet_count as u32;
            out[8..12].copy_from_slice(&pc.to_be_bytes());
        }
        Ok(n)
    }

    pub fn decode(counters_64bit: bool, buf: &[u8]) -> Result<Self, UdpError> {
        let n = Self::encoded_len(counters_64bit);
        if buf.len() < n {
            return Err(UdpError);
        }
        let sec = u32::from_be_bytes(buf[0..4].try_into().map_err(|_| UdpError)?);
        let usec = u32::from_be_bytes(buf[4..8].try_into().map_err(|_| UdpError)?);
        let packet_count = if counters_64bit {
            u64::from_be_bytes(buf[8..16].try_into().map_err(|_| UdpError)?)
        } else {
            u64::from(u32::from_be_bytes(
                buf[8..12].try_into().map_err(|_| UdpError)?,
            ))
        };
        Ok(Self {
            sec,
            usec,
            packet_count,
        })
    }
}

/// iperf 3.21 `iperf_udp.c`：`++packet_count` 后再写入报头；计数从 0 起，首包序号为 1。
pub fn next_udp_packet_count(seq: &mut u64) -> u64 {
    *seq = seq.saturating_add(1);
    *seq
}

pub fn is_udp_connect_msg(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[..4] == UDP_CONNECT_MSG
}
