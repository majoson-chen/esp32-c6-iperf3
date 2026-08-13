// Author: Cursor Grok 4.6
// Purpose: no_std iperf3 protocol library stub; real protocol lands in WP1.

#![no_std]

/// 占位版本串；WP1 再展开协议公开面。
pub fn proto_version() -> &'static str {
    "0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_version_is_nonempty() {
        assert!(!proto_version().is_empty());
    }
}
