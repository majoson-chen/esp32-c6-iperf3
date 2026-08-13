// Author: Cursor Grok 4.6
// Purpose: 单 embassy 任务驱动 iperf3-proto：控制 5201、数据通道、泵。

use alloc::vec;
use core::ptr::addr_of_mut;

use embassy_futures::join::join;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpMetadata, UdpSocket};
use embassy_net::Stack;
use embassy_time::{Duration, Instant, Timer};
use esp_println::println;
use iperf3_proto::udp::UdpHeader;
use iperf3_proto::{
    COOKIE_SIZE, Io, MAX_PARAMS_JSON, Server, Session, SessionError, Start, TestParams, Transport,
};

const CONTROL_PORT: u16 = 5201;
/// 与官方 embassy_wifi_bench 同级的 16KiB 数据窗。
const DATA_BUF: usize = 16 * 1024;
const CTRL_BUF: usize = 8 * 1024;
const UDP_META: usize = 16;
const PUMP_CHUNK: usize = 4 * 1024;

static mut CTRL_RX: [u8; CTRL_BUF] = [0; CTRL_BUF];
static mut CTRL_TX: [u8; CTRL_BUF] = [0; CTRL_BUF];
static mut DATA_RX: [u8; DATA_BUF] = [0; DATA_BUF];
static mut DATA_TX: [u8; DATA_BUF] = [0; DATA_BUF];
static mut UDP_RX_META: [PacketMetadata; UDP_META] = [PacketMetadata::EMPTY; UDP_META];
static mut UDP_TX_META: [PacketMetadata; UDP_META] = [PacketMetadata::EMPTY; UDP_META];
static mut UDP_RX: [u8; DATA_BUF] = [0; DATA_BUF];
static mut UDP_TX: [u8; DATA_BUF] = [0; DATA_BUF];
static mut PUMP_BUF: [u8; PUMP_CHUNK] = [0; PUMP_CHUNK];

enum Fail {
    Ctrl,
    Data,
    Proto(SessionError),
    LinkDown,
}

/// 有 IPv4 时循环 accept；断线则返回，让 main 等 DHCP。
pub async fn serve_while_up(stack: Stack<'static>) {
    // Server/Session 含 Rc<Cell>，必须留在本任务。
    let mut server = Server::new();
    loop {
        let mut ctrl = TcpSocket::new(stack, unsafe { &mut *addr_of_mut!(CTRL_RX) }, unsafe {
            &mut *addr_of_mut!(CTRL_TX)
        });
        println!("listen TCP {CONTROL_PORT}");
        match select(ctrl.accept(CONTROL_PORT), stack.wait_config_down()).await {
            Either::Second(()) => {
                ctrl.abort();
                return;
            }
            Either::First(Err(e)) => {
                println!("ERR accept: {e:?}");
                ctrl.abort();
                continue;
            }
            Either::First(Ok(())) => {}
        }

        match server.start_session() {
            Start::AccessDenied(byte) => {
                println!("ACCESS_DENIED");
                let _ = write_all(&mut ctrl, &byte).await;
                ctrl.abort();
            }
            Start::Accepted(mut session) => {
                let fail = drive_session(stack, &mut ctrl, &mut session).await;
                server.end_session(session);
                ctrl.abort();
                if matches!(fail, Err(Fail::LinkDown)) {
                    return;
                }
                if let Err(e) = fail {
                    println!("ERR session: {e}");
                }
            }
        }
    }
}

impl core::fmt::Display for Fail {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Fail::Ctrl => write!(f, "control socket"),
            Fail::Data => write!(f, "data socket"),
            Fail::Proto(e) => write!(f, "proto {e:?}"),
            Fail::LinkDown => write!(f, "link down"),
        }
    }
}

async fn drive_session(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    session: &mut Session,
) -> Result<(), Fail> {
    let mut data_ready = false;
    let mut tcp_data: Option<TcpSocket<'static>> = None;
    let mut udp_data: Option<(UdpSocket<'static>, Option<UdpMetadata>)> = None;

    loop {
        if !stack.is_config_up() {
            return Err(Fail::LinkDown);
        }
        match session.poll() {
            Io::WriteCtrl(bytes) => {
                write_all(ctrl, &bytes).await?;
            }
            Io::ReadCtrl(n) => {
                if n == 0 || n > MAX_PARAMS_JSON.max(64 * 1024) {
                    return Err(Fail::Proto(SessionError::Frame));
                }
                let mut buf = vec![0u8; n];
                match select(read_exact(ctrl, &mut buf), stack.wait_config_down()).await {
                    Either::Second(()) => return Err(Fail::LinkDown),
                    Either::First(r) => r?,
                }
                session.feed_ctrl(&buf).map_err(Fail::Proto)?;
                // 参数刚吃进去：先 listen 数据口，再让后续 poll 写出 CREATE_STREAMS。
                if !data_ready {
                    if let Some(params) = session.params() {
                        open_data_before_create_streams(
                            stack,
                            ctrl,
                            session,
                            params,
                            &mut tcp_data,
                            &mut udp_data,
                        )
                        .await?;
                        data_ready = true;
                        println!(
                            "TEST start transport={:?} reverse={}",
                            params.transport, params.reverse
                        );
                    }
                }
            }
            Io::NeedDataChannel { .. } => {
                // CREATE_STREAMS 已在 open_data_before_create_streams 里写完并完成握手。
                return Err(Fail::Proto(SessionError::Unexpected));
            }
            Io::Pump => {
                let params = session.params().ok_or(Fail::Proto(SessionError::Unexpected))?;
                pump(stack, ctrl, session, params, &mut tcp_data, &mut udp_data).await?;
            }
            Io::Done => {
                println!("TEST end");
                return Ok(());
            }
        }
    }
}

/// 先把数据通道推进 LISTEN/BIND，再写出 CREATE_STREAMS，最后完成 cookie/UDP hello。
///
/// `join(accept, write)` 先 poll accept：embassy-net 在第一个 `.await` 之前同步 `listen()`。
async fn open_data_before_create_streams(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    session: &mut Session,
    params: TestParams,
    tcp_data: &mut Option<TcpSocket<'static>>,
    udp_data: &mut Option<(UdpSocket<'static>, Option<UdpMetadata>)>,
) -> Result<(), Fail> {
    match params.transport {
        Transport::Tcp => {
            let mut data = TcpSocket::new(stack, unsafe { &mut *addr_of_mut!(DATA_RX) }, unsafe {
                &mut *addr_of_mut!(DATA_TX)
            });
            data.set_nagle_enabled(false);
            let write_cs = write_create_streams(ctrl, session);
            // listen-before-CREATE_STREAMS：左操作数先 poll → smoltcp LISTEN，再写 0x0A。
            // 握手期断线：取消 accept/write，丢掉数据 socket，外层重试。
            let (acc, wr) = match select(join(data.accept(CONTROL_PORT), write_cs), stack.wait_config_down())
                .await
            {
                Either::Second(()) => {
                    data.abort();
                    return Err(Fail::LinkDown);
                }
                Either::First(pair) => pair,
            };
            wr?;
            acc.map_err(|_| Fail::Data)?;
            let mut cookie = [0u8; COOKIE_SIZE];
            match select(read_exact(&mut data, &mut cookie), stack.wait_config_down()).await {
                Either::Second(()) => {
                    data.abort();
                    return Err(Fail::LinkDown);
                }
                Either::First(Err(_)) => return Err(Fail::Data),
                Either::First(Ok(())) => {}
            }
            match session.data_ready(&cookie) {
                Ok(()) => {}
                Err(SessionError::CookieMismatch) => {
                    let _ = write_all(&mut data, &[iperf3_proto::state::ACCESS_DENIED as u8]).await;
                    data.abort();
                    return Err(Fail::Proto(SessionError::CookieMismatch));
                }
                Err(e) => {
                    data.abort();
                    return Err(Fail::Proto(e));
                }
            }
            *tcp_data = Some(data);
        }
        Transport::Udp => {
            let mut udp = UdpSocket::new(
                stack,
                unsafe { &mut *addr_of_mut!(UDP_RX_META) },
                unsafe { &mut *addr_of_mut!(UDP_RX) },
                unsafe { &mut *addr_of_mut!(UDP_TX_META) },
                unsafe { &mut *addr_of_mut!(UDP_TX) },
            );
            udp.bind(CONTROL_PORT).map_err(|_| Fail::Data)?;
            write_create_streams(ctrl, session).await?;
            let buf = unsafe { &mut *addr_of_mut!(PUMP_BUF) };
            let (n, meta) = match select(udp.recv_from(buf), stack.wait_config_down()).await {
                Either::Second(()) => return Err(Fail::LinkDown),
                Either::First(r) => r.map_err(|_| Fail::Data)?,
            };
            let reply = session.udp_connect_reply(&buf[..n]).map_err(Fail::Proto)?;
            udp.send_to(&reply, meta).await.map_err(|_| Fail::Data)?;
            let cookie = *session.cookie();
            session.data_ready(&cookie).map_err(Fail::Proto)?;
            *udp_data = Some((udp, Some(meta)));
        }
    }
    Ok(())
}

async fn write_create_streams(ctrl: &mut TcpSocket<'_>, session: &mut Session) -> Result<(), Fail> {
    match session.poll() {
        Io::WriteCtrl(bytes) => write_all(ctrl, &bytes).await,
        _ => Err(Fail::Proto(SessionError::Unexpected)),
    }
}

async fn pump(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    session: &mut Session,
    params: TestParams,
    tcp_data: &mut Option<TcpSocket<'static>>,
    udp_data: &mut Option<(UdpSocket<'static>, Option<UdpMetadata>)>,
) -> Result<(), Fail> {
    let deadline = Instant::now() + Duration::from_secs(params.time_secs as u64);
    let mut end_byte = [0u8; 1];

    match params.transport {
        Transport::Tcp => {
            let data = tcp_data.as_mut().ok_or(Fail::Data)?;
            if params.reverse {
                pump_tcp_reverse(stack, ctrl, data, session, deadline, &mut end_byte).await
            } else {
                pump_tcp_forward(stack, ctrl, data, session, &mut end_byte).await
            }
        }
        Transport::Udp => {
            let (udp, peer) = udp_data.as_mut().ok_or(Fail::Data)?;
            if params.reverse {
                pump_udp_reverse(stack, ctrl, udp, peer, session, params, deadline, &mut end_byte)
                    .await
            } else {
                pump_udp_forward(stack, ctrl, udp, session, &mut end_byte).await
            }
        }
    }
}

async fn pump_tcp_forward(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    data: &mut TcpSocket<'_>,
    session: &mut Session,
    end_byte: &mut [u8; 1],
) -> Result<(), Fail> {
    let buf = unsafe { &mut *addr_of_mut!(PUMP_BUF) };
    loop {
        match select3(
            read_exact_one(ctrl, end_byte),
            data.read(buf),
            stack.wait_config_down(),
        )
        .await
        {
            Either3::Third(()) => return Err(Fail::LinkDown),
            Either3::First(Ok(())) => {
                session.feed_ctrl(end_byte).map_err(Fail::Proto)?;
                return Ok(());
            }
            Either3::First(Err(e)) => return Err(e),
            Either3::Second(Ok(0)) => {
                // 数据 EOF：停计数，等控制面 TEST_END；不得 end_test()。
                return wait_ctrl_test_end(stack, ctrl, session, end_byte).await;
            }
            Either3::Second(Ok(n)) => session.add_bytes(n as u64),
            Either3::Second(Err(_)) => return Err(Fail::Data),
        }
    }
}

async fn pump_tcp_reverse(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    data: &mut TcpSocket<'_>,
    session: &mut Session,
    deadline: Instant,
    end_byte: &mut [u8; 1],
) -> Result<(), Fail> {
    let buf = unsafe { &mut *addr_of_mut!(PUMP_BUF) };
    buf.fill(0x61);
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match select3(
            read_exact_one(ctrl, end_byte),
            data.write(buf),
            select(stack.wait_config_down(), Timer::at(deadline)),
        )
        .await
        {
            Either3::First(Ok(())) => {
                session.feed_ctrl(end_byte).map_err(Fail::Proto)?;
                return Ok(());
            }
            Either3::First(Err(e)) => return Err(e),
            Either3::Second(Ok(n)) => session.add_bytes(n as u64),
            Either3::Second(Err(_)) => return Err(Fail::Data),
            Either3::Third(Either::First(())) => return Err(Fail::LinkDown),
            Either3::Third(Either::Second(())) => break,
        }
    }
    // duration 到：停写，等 client TEST_END；不得 end_test()。
    wait_ctrl_test_end(stack, ctrl, session, end_byte).await
}

async fn pump_udp_forward(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    udp: &mut UdpSocket<'_>,
    session: &mut Session,
    end_byte: &mut [u8; 1],
) -> Result<(), Fail> {
    let buf = unsafe { &mut *addr_of_mut!(PUMP_BUF) };
    loop {
        match select3(
            read_exact_one(ctrl, end_byte),
            udp.recv_from(buf),
            stack.wait_config_down(),
        )
        .await
        {
            Either3::Third(()) => return Err(Fail::LinkDown),
            Either3::First(Ok(())) => {
                session.feed_ctrl(end_byte).map_err(Fail::Proto)?;
                return Ok(());
            }
            Either3::First(Err(e)) => return Err(e),
            Either3::Second(Ok((n, _))) => session.note_udp_datagram(&buf[..n]),
            Either3::Second(Err(_)) => return Err(Fail::Data),
        }
    }
}

async fn pump_udp_reverse(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    udp: &mut UdpSocket<'_>,
    peer: &mut Option<UdpMetadata>,
    session: &mut Session,
    params: TestParams,
    deadline: Instant,
    end_byte: &mut [u8; 1],
) -> Result<(), Fail> {
    let meta = (*peer).ok_or(Fail::Data)?;
    let blk = (params.blksize as usize).clamp(UdpHeader::encoded_len(params.udp_counters_64bit), DATA_BUF);
    let interval = udp_interval(params);
    let mut seq = 0u64;
    let mut next = Instant::now();
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match select3(
            read_exact_one(ctrl, end_byte),
            Timer::at(next),
            select(stack.wait_config_down(), Timer::at(deadline)),
        )
        .await
        {
            Either3::First(Ok(())) => {
                session.feed_ctrl(end_byte).map_err(Fail::Proto)?;
                return Ok(());
            }
            Either3::First(Err(e)) => return Err(e),
            Either3::Third(Either::First(())) => return Err(Fail::LinkDown),
            Either3::Third(Either::Second(())) => break,
            Either3::Second(()) => {
                let buf = unsafe { &mut *addr_of_mut!(PUMP_BUF) };
                let n = blk.min(buf.len());
                buf[..n].fill(0);
                let now = Instant::now().as_micros();
                let hdr = UdpHeader {
                    sec: (now / 1_000_000) as u32,
                    usec: (now % 1_000_000) as u32,
                    packet_count: seq,
                };
                let _ = hdr.encode(params.udp_counters_64bit, &mut buf[..n]);
                udp.send_to(&buf[..n], meta).await.map_err(|_| Fail::Data)?;
                session.add_bytes(n as u64);
                seq = seq.saturating_add(1);
                next += interval;
                if next < Instant::now() {
                    next = Instant::now();
                }
            }
        }
    }
    // duration 到：停发，等 client TEST_END；不得 end_test()。
    wait_ctrl_test_end(stack, ctrl, session, end_byte).await
}

fn udp_interval(params: TestParams) -> Duration {
    if params.bandwidth_bps == 0 {
        return Duration::from_micros(1);
    }
    let us = (params.blksize as u64)
        .saturating_mul(8_000_000)
        .checked_div(params.bandwidth_bps)
        .unwrap_or(1)
        .max(1);
    Duration::from_micros(us)
}

/// 停泵后读控制面 TEST_END（0x04）再 `feed_ctrl`。deadline/EOF 不得调用 `end_test()`。
async fn wait_ctrl_test_end(
    stack: Stack<'static>,
    ctrl: &mut TcpSocket<'_>,
    session: &mut Session,
    end_byte: &mut [u8; 1],
) -> Result<(), Fail> {
    match select(read_exact_one(ctrl, end_byte), stack.wait_config_down()).await {
        Either::Second(()) => Err(Fail::LinkDown),
        Either::First(Ok(())) => session.feed_ctrl(end_byte).map_err(Fail::Proto),
        Either::First(Err(e)) => Err(e),
    }
}

async fn read_exact_one(sock: &mut TcpSocket<'_>, byte: &mut [u8; 1]) -> Result<(), Fail> {
    read_exact(sock, byte.as_mut_slice()).await
}

async fn read_exact(sock: &mut TcpSocket<'_>, buf: &mut [u8]) -> Result<(), Fail> {
    let mut filled = 0;
    while filled < buf.len() {
        match sock.read(&mut buf[filled..]).await {
            Ok(0) => return Err(Fail::Ctrl),
            Ok(n) => filled += n,
            Err(_) => return Err(Fail::Ctrl),
        }
    }
    Ok(())
}

async fn write_all(sock: &mut TcpSocket<'_>, mut buf: &[u8]) -> Result<(), Fail> {
    while !buf.is_empty() {
        match sock.write(buf).await {
            Ok(0) => return Err(Fail::Ctrl),
            Ok(n) => buf = &buf[n..],
            Err(_) => return Err(Fail::Ctrl),
        }
    }
    Ok(())
}
