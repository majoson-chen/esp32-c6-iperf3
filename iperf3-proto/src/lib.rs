// Author: Cursor Grok 4.6
// Purpose: no_std iperf3 server-subset protocol library (control FSM, params, UDP).

#![no_std]

extern crate alloc;

pub mod frame;
pub mod params;
pub mod results;
pub mod session;
pub mod state;
pub mod udp;

/// Cookie 长度（iperf.h `COOKIE_SIZE`：ASCII UUID + NUL）。
pub const COOKIE_SIZE: usize = 37;

pub use frame::{FrameError, MAX_PARAMS_JSON, decode_json_len, encode_json_frame};
pub use params::{
    DEFAULT_DURATION_SECS, DEFAULT_TCP_BLKSIZE, DEFAULT_UDP_BLKSIZE, DEFAULT_UDP_RATE_BPS,
    MIN_UDP_BLOCKSIZE, ParamsError, TestParams, Transport,
};
pub use results::{StreamStats, encode_results_json};
pub use session::{IEPROTOCOL, IERECVPARAMS, Io, Server, Session, SessionError, Start};
pub use udp::{
    LEGACY_UDP_CONNECT_REPLY, UDP_CONNECT_MSG, UDP_CONNECT_REPLY, UdpError, UdpHeader,
    is_udp_connect_msg, next_udp_packet_count,
};

/// 占位版本串。
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

    #[test]
    fn cookie_is_37_bytes() {
        assert_eq!(COOKIE_SIZE, 37);
    }

    #[test]
    fn cookie_feed_rejects_wrong_length() {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        assert_eq!(s.feed_ctrl(&[0; 36]), Err(SessionError::CookieLen));
        s.feed_ctrl(&[0; COOKIE_SIZE]).unwrap();
        assert_eq!(drain_writes(&mut s), [state::PARAM_EXCHANGE as u8]);
    }

    #[test]
    fn state_bytes_match_iperf_3_21() {
        assert_eq!(state::PARAM_EXCHANGE, 9);
        assert_eq!(state::CREATE_STREAMS, 10);
        assert_eq!(state::TEST_START, 1);
        assert_eq!(state::TEST_RUNNING, 2);
        assert_eq!(state::TEST_END, 4);
        assert_eq!(state::EXCHANGE_RESULTS, 13);
        assert_eq!(state::DISPLAY_RESULTS, 14);
        assert_eq!(state::IPERF_DONE, 16);
        assert_eq!(state::ACCESS_DENIED, -1);
        assert_eq!(state::SERVER_ERROR, -2);
    }

    #[test]
    fn json_frame_is_be_length_then_utf8() {
        let payload = b"{\"tcp\":true}";
        let framed = encode_json_frame(payload);
        assert_eq!(&framed[..4], &(payload.len() as u32).to_be_bytes());
        assert_eq!(&framed[4..], payload);
        let (len, rest) = decode_json_len(&framed[..4]).unwrap();
        assert_eq!(len, payload.len());
        assert!(rest.is_empty());
    }

    #[test]
    fn params_accept_default_tcp() {
        let p = TestParams::parse_json(br#"{"tcp":true,"omit":0,"time":10,"parallel":1}"#).unwrap();
        assert_eq!(p.transport, Transport::Tcp);
        assert!(!p.reverse);
        assert_eq!(p.time_secs, 10);
        assert_eq!(p.parallel, 1);
        assert_eq!(p.blksize, DEFAULT_TCP_BLKSIZE);
    }

    #[test]
    fn params_accept_udp_reverse_and_64bit() {
        let p = TestParams::parse_json(
            br#"{"udp":true,"reverse":true,"time":5,"len":1460,"bandwidth":1000000,"udp_counters_64bit":1}"#,
        )
        .unwrap();
        assert_eq!(p.transport, Transport::Udp);
        assert!(p.reverse);
        assert_eq!(p.time_secs, 5);
        assert_eq!(p.blksize, 1460);
        assert_eq!(p.bandwidth_bps, 1_000_000);
        assert!(p.udp_counters_64bit);
    }

    #[test]
    fn params_reject_sctp_parallel_bidir() {
        assert_eq!(
            TestParams::parse_json(br#"{"sctp":true}"#).unwrap_err(),
            ParamsError::Sctp
        );
        assert_eq!(
            TestParams::parse_json(br#"{"tcp":true,"parallel":2}"#).unwrap_err(),
            ParamsError::Parallel
        );
        assert_eq!(
            TestParams::parse_json(br#"{"tcp":true,"bidirectional":true}"#).unwrap_err(),
            ParamsError::Bidir
        );
        assert_eq!(
            TestParams::parse_json(br#"{"tcp":true,"bidir":true}"#).unwrap_err(),
            ParamsError::Bidir
        );
        assert_eq!(
            TestParams::parse_json(br#"{"udp":true,"len":8}"#).unwrap_err(),
            ParamsError::UdpTooSmall
        );
    }

    #[test]
    fn params_ignore_omit_window_mss_nodelay() {
        let p = TestParams::parse_json(
            br#"{"tcp":true,"omit":2,"window":65535,"MSS":1400,"nodelay":true,"time":10,"parallel":1}"#,
        )
        .unwrap();
        assert_eq!(p.transport, Transport::Tcp);
        assert_eq!(p.time_secs, 10);
    }

    #[test]
    fn udp_header_roundtrip_32bit_and_64bit() {
        let h = UdpHeader {
            sec: 1,
            usec: 2,
            packet_count: 0x0102_0304_0506_0708,
        };
        let mut buf32 = [0u8; 12];
        assert_eq!(h.encode(false, &mut buf32).unwrap(), 12);
        assert_eq!(&buf32[..], &[0, 0, 0, 1, 0, 0, 0, 2, 5, 6, 7, 8]);
        let d32 = UdpHeader::decode(false, &buf32).unwrap();
        assert_eq!(d32.sec, 1);
        assert_eq!(d32.usec, 2);
        assert_eq!(d32.packet_count, 0x0506_0708);

        let mut buf64 = [0u8; 16];
        assert_eq!(h.encode(true, &mut buf64).unwrap(), 16);
        assert_eq!(
            &buf64[..],
            &[0, 0, 0, 1, 0, 0, 0, 2, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        let d64 = UdpHeader::decode(true, &buf64).unwrap();
        assert_eq!(d64, h);
    }

    #[test]
    fn udp_send_seq_starts_at_one_like_iperf_3_21() {
        // 3.21 iperf_udp.c：`++packet_count` 后再写入报头，首包序号为 1。
        let mut seq = 0u64;
        let hdr = UdpHeader {
            sec: 0,
            usec: 0,
            packet_count: next_udp_packet_count(&mut seq),
        };
        let mut buf = [0u8; 12];
        hdr.encode(false, &mut buf).unwrap();
        assert_eq!(UdpHeader::decode(false, &buf).unwrap().packet_count, 1);
        assert_eq!(next_udp_packet_count(&mut seq), 2);
    }

    #[test]
    fn udp_connect_constants_are_3_21_wire_bytes() {
        // 3.21 在 BE/LE 上对调宏，使线上字节固定为 ASCII "9876" / "6789"
        assert_eq!(&UDP_CONNECT_MSG, b"9876");
        assert_eq!(&UDP_CONNECT_REPLY, b"6789");
        assert_eq!(LEGACY_UDP_CONNECT_REPLY, [0xB1, 0x68, 0xDE, 0x3A]);
    }

    #[test]
    fn result_json_has_required_3_21_fields() {
        let stats = StreamStats {
            id: 1,
            bytes: 42,
            retransmits: -1,
            errors: 3,
            packets: 9,
        };
        let json = encode_results_json(&stats, -1);
        let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(v["cpu_util_total"], 0);
        assert_eq!(v["cpu_util_user"], 0);
        assert_eq!(v["cpu_util_system"], 0);
        assert_eq!(v["sender_has_retransmits"], -1);
        let s = &v["streams"][0];
        assert_eq!(s["id"], 1);
        assert_eq!(s["bytes"], 42);
        assert_eq!(s["retransmits"], -1);
        assert_eq!(s["jitter"], 0);
        assert_eq!(s["errors"], 3);
        assert_eq!(s["packets"], 9);
    }

    fn drain_writes(session: &mut Session) -> alloc::vec::Vec<u8> {
        let mut out = alloc::vec::Vec::new();
        while let Io::WriteCtrl(b) = session.poll() {
            out.extend_from_slice(&b);
        }
        out
    }

    #[test]
    fn busy_server_returns_access_denied() {
        let mut server = Server::new();
        // 必须持有会话：drop 最后一轮 Session 会清 busy。
        let Start::Accepted(_held) = server.start_session() else {
            panic!("first session should be accepted");
        };
        match server.start_session() {
            Start::AccessDenied(byte) => {
                assert_eq!(byte, [state::ACCESS_DENIED as u8]);
            }
            Start::Accepted(_) => panic!("overlapping session must be denied"),
        }
    }

    #[test]
    fn dropping_last_session_clears_busy() {
        let mut server = Server::new();
        {
            let Start::Accepted(_s) = server.start_session() else {
                panic!("expected session");
            };
            assert!(server.is_busy());
        }
        assert!(!server.is_busy());
        assert!(matches!(server.start_session(), Start::Accepted(_)));
    }

    #[test]
    fn end_session_clears_busy() {
        let mut server = Server::new();
        let Start::Accepted(s) = server.start_session() else {
            panic!("expected session");
        };
        assert!(server.is_busy());
        server.end_session(s);
        assert!(!server.is_busy());
        assert!(matches!(server.start_session(), Start::Accepted(_)));
    }

    #[test]
    fn happy_tcp_control_state_order() {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        assert_eq!(s.poll(), Io::ReadCtrl(COOKIE_SIZE));

        let cookie = [b'C'; COOKIE_SIZE];
        s.feed_ctrl(&cookie).unwrap();
        assert_eq!(drain_writes(&mut s), [state::PARAM_EXCHANGE as u8]);
        assert_eq!(s.poll(), Io::ReadCtrl(4));

        let params = br#"{"tcp":true,"time":10,"parallel":1}"#;
        let frame = encode_json_frame(params);
        s.feed_ctrl(&frame[..4]).unwrap();
        assert_eq!(s.poll(), Io::ReadCtrl(params.len()));
        s.feed_ctrl(params).unwrap();
        assert_eq!(drain_writes(&mut s), [state::CREATE_STREAMS as u8]);

        match s.poll() {
            Io::NeedDataChannel {
                cookie: c,
                transport,
            } => {
                assert_eq!(c, cookie);
                assert_eq!(transport, Transport::Tcp);
            }
            other => panic!("expected data channel, got {other:?}"),
        }

        s.data_ready(&cookie).unwrap();
        assert_eq!(
            drain_writes(&mut s),
            [state::TEST_START as u8, state::TEST_RUNNING as u8]
        );
        assert_eq!(s.poll(), Io::Pump);

        s.add_bytes(1000);
        s.feed_ctrl(&[state::TEST_END as u8]).unwrap();
        let mut wire = drain_writes(&mut s);
        assert_eq!(wire.remove(0), state::EXCHANGE_RESULTS as u8);
        assert!(wire.is_empty());

        let client_json = br#"{"cpu_util_total":0,"cpu_util_user":0,"cpu_util_system":0,"sender_has_retransmits":0,"streams":[]}"#;
        let client_frame = encode_json_frame(client_json);
        s.feed_ctrl(&client_frame[..4]).unwrap();
        assert_eq!(s.poll(), Io::ReadCtrl(client_json.len()));
        s.feed_ctrl(client_json).unwrap();

        let written = drain_writes(&mut s);
        let (len, _) = decode_json_len(&written[..4]).unwrap();
        let json = &written[4..4 + len];
        let v: serde_json::Value = serde_json::from_slice(json).unwrap();
        assert_eq!(v["streams"][0]["bytes"], 1000);
        assert_eq!(written[4 + len], state::DISPLAY_RESULTS as u8);

        assert_eq!(s.poll(), Io::ReadCtrl(1));
        s.feed_ctrl(&[state::IPERF_DONE as u8]).unwrap();
        assert_eq!(s.poll(), Io::Done);
        server.end_session(s);
        // 结束后应能再接一轮
        assert!(matches!(server.start_session(), Start::Accepted(_)));
    }

    /// reverse 到时若先 `end_test()`，会排队 EXCHANGE_RESULTS 并改读 4 字节 JSON 长度；
    /// client 随后的 TEST_END（1 字节 0x04）会被当成长度前缀。固件必须先等 TEST_END。
    #[test]
    fn end_test_then_test_end_byte_is_not_json_length() {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        let cookie = [b'C'; COOKIE_SIZE];
        s.feed_ctrl(&cookie).unwrap();
        let _ = drain_writes(&mut s);
        let params = br#"{"tcp":true,"time":10,"parallel":1,"reverse":true}"#;
        let frame = encode_json_frame(params);
        s.feed_ctrl(&frame[..4]).unwrap();
        s.feed_ctrl(params).unwrap();
        let _ = drain_writes(&mut s);
        let _ = s.poll();
        s.data_ready(&cookie).unwrap();
        let _ = drain_writes(&mut s);
        assert_eq!(s.poll(), Io::Pump);

        s.end_test().unwrap();
        assert_eq!(drain_writes(&mut s), [state::EXCHANGE_RESULTS as u8]);
        assert_eq!(s.poll(), Io::ReadCtrl(4));
        assert_eq!(
            s.feed_ctrl(&[state::TEST_END as u8]),
            Err(SessionError::Frame)
        );
    }

    fn assert_reject_sends_ieprotocol(params: &[u8]) {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        s.feed_ctrl(&[b'C'; COOKIE_SIZE]).unwrap();
        let _ = drain_writes(&mut s);
        let frame = encode_json_frame(params);
        s.feed_ctrl(&frame[..4]).unwrap();
        s.feed_ctrl(params).unwrap();
        let wire = drain_writes(&mut s);
        // SERVER_ERROR(-2) + BE i32 IEPROTOCOL(131) + BE i32 errno(0)
        assert_eq!(wire.len(), 9);
        assert_eq!(wire[0] as i8, state::SERVER_ERROR);
        assert_eq!(&wire[1..5], &IEPROTOCOL.to_be_bytes());
        assert_eq!(&wire[5..9], &0i32.to_be_bytes());
        assert_eq!(s.poll(), Io::Done);
    }

    #[test]
    fn rejected_params_send_server_error() {
        assert_reject_sends_ieprotocol(br#"{"tcp":true,"parallel":2}"#);
        assert_reject_sends_ieprotocol(br#"{"sctp":true}"#);
        assert_reject_sends_ieprotocol(br#"{"tcp":true,"bidirectional":true}"#);
    }

    #[test]
    fn udp_params_need_udp_channel_and_connect_reply() {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        s.feed_ctrl(&[b'U'; COOKIE_SIZE]).unwrap();
        let _ = drain_writes(&mut s);
        let params = br#"{"udp":true,"time":1,"parallel":1,"len":1460}"#;
        let frame = encode_json_frame(params);
        s.feed_ctrl(&frame[..4]).unwrap();
        s.feed_ctrl(params).unwrap();
        let _ = drain_writes(&mut s);
        match s.poll() {
            Io::NeedDataChannel { transport, .. } => assert_eq!(transport, Transport::Udp),
            other => panic!("expected UDP channel, got {other:?}"),
        }
        assert_eq!(
            s.udp_connect_reply(&UDP_CONNECT_MSG).unwrap(),
            UDP_CONNECT_REPLY
        );
        assert!(s.udp_connect_reply(b"xxxx").is_err());
    }

    #[test]
    fn data_cookie_mismatch_is_error() {
        let mut server = Server::new();
        let Start::Accepted(mut s) = server.start_session() else {
            panic!("expected session");
        };
        let cookie = [b'C'; COOKIE_SIZE];
        s.feed_ctrl(&cookie).unwrap();
        let _ = drain_writes(&mut s);
        let params = br#"{"tcp":true,"parallel":1}"#;
        let frame = encode_json_frame(params);
        s.feed_ctrl(&frame[..4]).unwrap();
        s.feed_ctrl(params).unwrap();
        let _ = drain_writes(&mut s);
        let _ = s.poll();
        let mut bad = cookie;
        bad[0] = b'X';
        assert_eq!(s.data_ready(&bad), Err(SessionError::CookieMismatch));
    }
}
