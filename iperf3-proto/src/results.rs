// Author: Cursor Grok 4.6
// Purpose: 生成 3.21 client 能读完的最小结果 JSON（send_results 字段）。

use alloc::format;
use alloc::vec::Vec;

/// 单流计数；v1 只有一条流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamStats {
    pub id: i32,
    pub bytes: u64,
    pub retransmits: i64,
    pub errors: i64,
    pub packets: i64,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            id: 1,
            bytes: 0,
            retransmits: -1,
            errors: 0,
            packets: 0,
        }
    }
}

/// 未格式化 JSON 对象（无长度前缀）。`sender_has_retransmits`：接收端为 -1。
pub fn encode_results_json(stats: &StreamStats, sender_has_retransmits: i32) -> Vec<u8> {
    // jitter 固定写 0：固件可不算 RFC 1889；client 只要 Number。
    let s = format!(
        "{{\"cpu_util_total\":0,\"cpu_util_user\":0,\"cpu_util_system\":0,\"sender_has_retransmits\":{},\"streams\":[{{\"id\":{},\"bytes\":{},\"retransmits\":{},\"jitter\":0,\"errors\":{},\"packets\":{}}}]}}",
        sender_has_retransmits,
        stats.id,
        stats.bytes,
        stats.retransmits,
        stats.errors,
        stats.packets
    );
    s.into_bytes()
}
