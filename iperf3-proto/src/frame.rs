// Author: Cursor Grok 4.6
// Purpose: iperf3 JSON messages: 32-bit big-endian length + UTF-8 body.

use alloc::vec::Vec;

/// 参数 JSON 上限（iperf.h `MAX_PARAMS_JSON_STRING`）。
pub const MAX_PARAMS_JSON: usize = 8 * 1024;

/// 长度字段过短或声明长度非法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameError;

/// 编码 `[len_be32][json]`。
pub fn encode_json_frame(json: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(json);
    out
}

/// 从至少 4 字节中读出 JSON 长度；其余字节原样返回。
pub fn decode_json_len(prefix: &[u8]) -> Result<(usize, &[u8]), FrameError> {
    if prefix.len() < 4 {
        return Err(FrameError);
    }
    let len = u32::from_be_bytes(prefix[..4].try_into().map_err(|_| FrameError)?) as usize;
    Ok((len, &prefix[4..]))
}
