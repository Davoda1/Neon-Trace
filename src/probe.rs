//! Native diagnostic engine. No external commands, raw-socket privileges, or services.
//! Linux: ICMP datagram sockets + IP_RECVERR/IPV6_RECVERR. Windows: IP Helper ICMP APIs.
use crate::target::{self, NormalizedTarget};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    runtime::Runtime,
    sync::{Notify, Semaphore, mpsc},
    task::JoinHandle,
};

pub const MAX_TARGETS: usize = 10;
pub const MAX_HOPS: u8 = 30;
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(900);
pub const PING_INTERVAL: Duration = Duration::from_secs(1);

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeConfig {
    pub target_id: u64,
    pub target: String,
    pub tcp_check: bool,
}
impl ProbeConfig {
    pub fn new(target_id: u64, target: impl Into<String>, tcp_check: bool) -> Self {
        Self {
            target_id,
            target: target.into(),
            tcp_check,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidConfig,
    InvalidTarget,
    Dns,
    PermissionDenied,
    Unsupported,
    Socket,
    Runtime,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeError {
    pub code: ErrorCode,
    pub detail: String,
}
impl ProbeError {
    fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
    #[cfg(target_os = "linux")]
    fn io(error: std::io::Error) -> Self {
        let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::PermissionDenied
        } else {
            ErrorCode::Socket
        };
        // A numeric OS code avoids persisting localized error text or private paths.
        Self::new(
            code,
            error
                .raw_os_error()
                .map(|v| format!("os_error_{v}"))
                .unwrap_or_else(|| "socket_error".into()),
        )
    }
}
impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}
impl std::error::Error for ProbeError {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Reply,
    TimeExceeded,
    Timeout,
    Unreachable,
    Error,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeObservation {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub responder: Option<IpAddr>,
    /// Round trip from this computer to responder, never a link delay.
    pub rtt_ms: Option<f64>,
    pub status: ObservationStatus,
    pub error: Option<ProbeError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopObservation {
    pub hop: u8,
    pub probe: ProbeObservation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpObservation {
    pub timestamp_ms: u64,
    pub connected: bool,
    pub connect_ms: Option<f64>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProbePayload {
    Resolved {
        host: String,
        chosen: IpAddr,
        candidates: Vec<IpAddr>,
    },
    Ping(ProbeObservation),
    TraceStarted {
        route_id: u64,
    },
    Hop {
        route_id: u64,
        observation: HopObservation,
    },
    TraceComplete {
        route_id: u64,
        reached: bool,
        max_hops: u8,
    },
    Tcp(TcpObservation),
    Error(ProbeError),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeEvent {
    pub target_id: u64,
    pub generation: u64,
    pub timestamp_ms: u64,
    pub payload: ProbePayload,
}
struct Control {
    cancel: Arc<AtomicBool>,
    route_epoch: Arc<AtomicU64>,
    retrace: Arc<Notify>,
    task: JoinHandle<()>,
}

/// Poll events from the UI timer. Queue capacity and tasks stay bounded; slow UI
/// consumers apply backpressure. Caller must reject events for old generations.
pub struct ProbeManager {
    runtime: Option<Runtime>,
    receiver: mpsc::Receiver<ProbeEvent>,
    sender: mpsc::Sender<ProbeEvent>,
    controls: HashMap<u64, Control>,
    next_generation: u64,
    global: Arc<Semaphore>,
}
impl ProbeManager {
    pub fn new() -> Result<Self, ProbeError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(24)
            .enable_all()
            .build()
            .map_err(|_| ProbeError::new(ErrorCode::Runtime, "runtime_creation_failed"))?;
        let (sender, receiver) = mpsc::channel(512);
        Ok(Self {
            runtime: Some(runtime),
            receiver,
            sender,
            controls: HashMap::new(),
            next_generation: 1,
            global: Arc::new(Semaphore::new(20)),
        })
    }
    /// Atomically validates the group before replacing current sessions.
    pub fn start(&mut self, configs: Vec<ProbeConfig>) -> Result<Vec<(u64, u64)>, ProbeError> {
        if configs.is_empty() || configs.len() > MAX_TARGETS {
            return Err(ProbeError::new(
                ErrorCode::InvalidConfig,
                "target_count_1_to_10",
            ));
        }
        let mut parsed = Vec::with_capacity(configs.len());
        let mut ids = std::collections::HashSet::new();
        for config in configs {
            if !ids.insert(config.target_id) {
                return Err(ProbeError::new(
                    ErrorCode::InvalidConfig,
                    "duplicate_target_id",
                ));
            }
            let target = target::normalize(&config.target)
                .map_err(|e| ProbeError::new(ErrorCode::InvalidTarget, e.to_string()))?;
            parsed.push((config, target));
        }
        self.stop();
        let mut generations = Vec::new();
        for (config, target) in parsed {
            let generation = self.next_generation;
            self.next_generation = self.next_generation.wrapping_add(1).max(1);
            let cancel = Arc::new(AtomicBool::new(false));
            let route_epoch = Arc::new(AtomicU64::new(1));
            let retrace = Arc::new(Notify::new());
            let ctx = Context {
                id: config.target_id,
                generation,
                cancel: cancel.clone(),
                route_epoch: route_epoch.clone(),
                retrace: retrace.clone(),
                sender: self.sender.clone(),
                global: self.global.clone(),
                local: Arc::new(Semaphore::new(2)),
            };
            let task =
                self.runtime
                    .as_ref()
                    .unwrap()
                    .spawn(run_target(config.clone(), target, ctx));
            self.controls.insert(
                config.target_id,
                Control {
                    cancel,
                    route_epoch,
                    retrace,
                    task,
                },
            );
            generations.push((config.target_id, generation));
        }
        Ok(generations)
    }
    pub fn stop(&mut self) {
        for (_, control) in self.controls.drain() {
            control.cancel.store(true, Ordering::Release);
            control.retrace.notify_waiters();
            control.task.abort();
        }
    }
    /// Refreshes only the route for the already pinned endpoint; ping continues.
    pub fn trace_again(&self, target_id: u64) -> bool {
        if let Some(c) = self.controls.get(&target_id) {
            c.route_epoch.fetch_add(1, Ordering::AcqRel);
            c.retrace.notify_one();
            true
        } else {
            false
        }
    }
    pub fn try_recv(&mut self) -> Option<ProbeEvent> {
        self.receiver.try_recv().ok()
    }
    pub fn active_count(&self) -> usize {
        self.controls.len()
    }
}
impl Drop for ProbeManager {
    fn drop(&mut self) {
        self.stop();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}
#[derive(Clone)]
struct Context {
    id: u64,
    generation: u64,
    cancel: Arc<AtomicBool>,
    route_epoch: Arc<AtomicU64>,
    retrace: Arc<Notify>,
    sender: mpsc::Sender<ProbeEvent>,
    global: Arc<Semaphore>,
    local: Arc<Semaphore>,
}
impl Context {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    async fn send(&self, payload: ProbePayload) -> bool {
        if self.cancelled() {
            return false;
        }
        let event = ProbeEvent {
            target_id: self.id,
            generation: self.generation,
            timestamp_ms: unix_ms(),
            payload,
        };
        tokio::select! {r=self.sender.send(event)=>r.is_ok(),_=wait_cancel(self.cancel.clone())=>false}
    }
}
async fn wait_cancel(cancel: Arc<AtomicBool>) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn run_target(config: ProbeConfig, target: NormalizedTarget, ctx: Context) {
    let resolved =
        tokio::select! {r=target::resolve(&target)=>r,_=wait_cancel(ctx.cancel.clone())=>return};
    let resolved = match resolved {
        Ok(v) => v,
        Err(e) => {
            ctx.send(ProbePayload::Error(ProbeError::new(
                ErrorCode::Dns,
                e.to_string(),
            )))
            .await;
            return;
        }
    };
    let ip = resolved.chosen;
    if !ctx
        .send(ProbePayload::Resolved {
            host: target.host,
            chosen: ip,
            candidates: resolved.candidates,
        })
        .await
    {
        return;
    }
    tokio::join!(
        endpoint_loop(ip, ctx.clone()),
        trace_loop(ip, ctx.clone()),
        tcp_loop(ip, config.tcp_check, ctx)
    );
}
async fn native_probe(ip: IpAddr, ttl: u8, sequence: u64, ctx: &Context) -> ProbeObservation {
    let fallback = || ProbeObservation {
        sequence,
        timestamp_ms: unix_ms(),
        responder: None,
        rtt_ms: None,
        status: ObservationStatus::Cancelled,
        error: None,
    };
    let global = tokio::select! {v=ctx.global.clone().acquire_owned()=>v.ok(),_=wait_cancel(ctx.cancel.clone())=>None};
    let Some(global) = global else {
        return fallback();
    };
    let local = tokio::select! {v=ctx.local.clone().acquire_owned()=>v.ok(),_=wait_cancel(ctx.cancel.clone())=>None};
    let Some(local) = local else {
        return fallback();
    };
    let cancel = ctx.cancel.clone();
    // Move permits into the blocking job: cancellation cannot free them early
    // while an OS call is still in flight.
    tokio::task::spawn_blocking(move || {
        let _permits = (global, local);
        platform::probe(ip, ttl, sequence, PROBE_TIMEOUT, cancel)
    })
    .await
    .unwrap_or_else(|_| ProbeObservation {
        sequence,
        timestamp_ms: unix_ms(),
        responder: None,
        rtt_ms: None,
        status: ObservationStatus::Error,
        error: Some(ProbeError::new(ErrorCode::Runtime, "probe_worker_failed")),
    })
}
async fn endpoint_loop(ip: IpAddr, ctx: Context) {
    let mut cadence = tokio::time::interval(PING_INTERVAL);
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sequence = 0;
    loop {
        tokio::select! {_=cadence.tick()=>{},_=wait_cancel(ctx.cancel.clone())=>break};
        if ctx.cancelled() {
            break;
        }
        sequence += 1;
        let observation = native_probe(ip, 64, sequence, &ctx).await;
        let fatal = observation.error.as_ref().is_some_and(|e| {
            matches!(e.code, ErrorCode::PermissionDenied | ErrorCode::Unsupported)
        });
        if !ctx.send(ProbePayload::Ping(observation)).await || fatal {
            break;
        }
    }
}
async fn trace_loop(ip: IpAddr, ctx: Context) {
    loop {
        if ctx.cancelled() {
            break;
        }
        let route_id = ctx.route_epoch.load(Ordering::Acquire);
        if !ctx.send(ProbePayload::TraceStarted { route_id }).await {
            break;
        }
        let mut reached = false;
        for hop in 1..=MAX_HOPS {
            if ctx.cancelled() || ctx.route_epoch.load(Ordering::Acquire) != route_id {
                break;
            }
            let observation = native_probe(ip, hop, hop as u64, &ctx).await;
            if ctx.cancelled() || ctx.route_epoch.load(Ordering::Acquire) != route_id {
                break;
            }
            reached =
                observation.status == ObservationStatus::Reply && observation.responder == Some(ip);
            let fatal = observation.status == ObservationStatus::Error;
            let unreachable = observation.status == ObservationStatus::Unreachable;
            if !ctx
                .send(ProbePayload::Hop {
                    route_id,
                    observation: HopObservation {
                        hop,
                        probe: observation,
                    },
                })
                .await
            {
                return;
            }
            if reached || fatal || unreachable {
                break;
            }
            // Avoid bursts when nearby routers respond immediately.
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        if ctx.cancelled() {
            break;
        }
        if ctx.route_epoch.load(Ordering::Acquire) == route_id {
            if !ctx
                .send(ProbePayload::TraceComplete {
                    route_id,
                    reached,
                    max_hops: MAX_HOPS,
                })
                .await
            {
                break;
            }
            loop {
                if ctx.cancelled() {
                    return;
                }
                if ctx.route_epoch.load(Ordering::Acquire) != route_id {
                    break;
                }
                tokio::select! {_=ctx.retrace.notified()=>{},_=wait_cancel(ctx.cancel.clone())=>return}
            }
        }
    }
}
async fn tcp_loop(ip: IpAddr, enabled: bool, ctx: Context) {
    if !enabled {
        return;
    }
    let mut cadence = tokio::time::interval(Duration::from_secs(10));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=cadence.tick()=>{},_=wait_cancel(ctx.cancel.clone())=>return};
        let global = tokio::select! {v=ctx.global.clone().acquire_owned()=>v.ok(),_=wait_cancel(ctx.cancel.clone())=>None};
        let Some(_global) = global else {
            return;
        };
        let local = tokio::select! {v=ctx.local.clone().acquire_owned()=>v.ok(),_=wait_cancel(ctx.cancel.clone())=>None};
        let Some(_local) = local else {
            return;
        };
        let start = std::time::Instant::now();
        let result = tokio::select! {v=tokio::time::timeout(PROBE_TIMEOUT,tokio::net::TcpStream::connect(SocketAddr::new(ip,443)))=>v,_=wait_cancel(ctx.cancel.clone())=>return};
        let observation = match result {
            Ok(Ok(stream)) => {
                drop(stream);
                TcpObservation {
                    timestamp_ms: unix_ms(),
                    connected: true,
                    connect_ms: Some(start.elapsed().as_secs_f64() * 1000.0),
                    error: None,
                }
            }
            Ok(Err(e)) => TcpObservation {
                timestamp_ms: unix_ms(),
                connected: false,
                connect_ms: None,
                error: Some(
                    e.raw_os_error()
                        .map(|c| format!("os_error_{c}"))
                        .unwrap_or_else(|| "tcp_error".into()),
                ),
            },
            Err(_) => TcpObservation {
                timestamp_ms: unix_ms(),
                connected: false,
                connect_ms: None,
                error: Some("tcp_timeout".into()),
            },
        };
        if !ctx.send(ProbePayload::Tcp(observation)).await {
            return;
        }
    }
}
fn result(
    sequence: u64,
    status: ObservationStatus,
    responder: Option<IpAddr>,
    rtt_ms: Option<f64>,
    error: Option<ProbeError>,
) -> ProbeObservation {
    ProbeObservation {
        sequence,
        timestamp_ms: unix_ms(),
        responder,
        rtt_ms,
        status,
        error,
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{
        mem,
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
        time::Instant,
    };
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ExtendedError {
        errno: u32,
        origin: u8,
        kind: u8,
        code: u8,
        pad: u8,
        info: u32,
        data: u32,
    }
    fn checksum(bytes: &[u8]) -> u16 {
        let mut sum = 0u32;
        for chunk in bytes.chunks(2) {
            sum += u16::from_be_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)]) as u32;
        }
        while sum >> 16 != 0 {
            sum = (sum & 65535) + (sum >> 16);
        }
        !(sum as u16)
    }
    fn socket_address(ip: IpAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
        let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let len = match ip {
            IpAddr::V4(ip) => {
                let address = libc::sockaddr_in {
                    sin_family: libc::AF_INET as _,
                    sin_port: 0,
                    sin_addr: libc::in_addr {
                        s_addr: u32::from_ne_bytes(ip.octets()),
                    },
                    sin_zero: [0; 8],
                };
                unsafe {
                    std::ptr::write(&mut storage as *mut _ as *mut libc::sockaddr_in, address)
                };
                mem::size_of::<libc::sockaddr_in>()
            }
            IpAddr::V6(ip) => {
                let address = libc::sockaddr_in6 {
                    sin6_family: libc::AF_INET6 as _,
                    sin6_port: 0,
                    sin6_flowinfo: 0,
                    sin6_addr: libc::in6_addr {
                        s6_addr: ip.octets(),
                    },
                    sin6_scope_id: 0,
                };
                unsafe {
                    std::ptr::write(&mut storage as *mut _ as *mut libc::sockaddr_in6, address)
                };
                mem::size_of::<libc::sockaddr_in6>()
            }
        };
        (storage, len as _)
    }
    unsafe fn read_ip(address: *const libc::sockaddr, len: usize) -> Option<IpAddr> {
        if len < mem::size_of::<libc::sa_family_t>() {
            return None;
        }
        let family =
            unsafe { std::ptr::read_unaligned(address as *const libc::sa_family_t) } as i32;
        if family == libc::AF_INET && len >= mem::size_of::<libc::sockaddr_in>() {
            let v = unsafe { std::ptr::read_unaligned(address as *const libc::sockaddr_in) };
            Some(IpAddr::V4(std::net::Ipv4Addr::from(
                v.sin_addr.s_addr.to_ne_bytes(),
            )))
        } else if family == libc::AF_INET6 && len >= mem::size_of::<libc::sockaddr_in6>() {
            let v = unsafe { std::ptr::read_unaligned(address as *const libc::sockaddr_in6) };
            Some(IpAddr::V6(std::net::Ipv6Addr::from(v.sin6_addr.s6_addr)))
        } else {
            None
        }
    }
    fn option(fd: i32, level: i32, name: i32, value: i32) -> Result<(), ProbeError> {
        let r = unsafe {
            libc::setsockopt(
                fd,
                level,
                name,
                &value as *const _ as *const _,
                mem::size_of::<i32>() as _,
            )
        };
        if r < 0 {
            Err(ProbeError::io(std::io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }
    pub fn probe(
        ip: IpAddr,
        ttl: u8,
        sequence: u64,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    ) -> ProbeObservation {
        if cancel.load(Ordering::Acquire) {
            return result(sequence, ObservationStatus::Cancelled, None, None, None);
        }
        let v6 = ip.is_ipv6();
        let family = if v6 { libc::AF_INET6 } else { libc::AF_INET };
        let protocol = if v6 {
            libc::IPPROTO_ICMPV6
        } else {
            libc::IPPROTO_ICMP
        };
        let raw = unsafe {
            libc::socket(
                family,
                libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                protocol,
            )
        };
        if raw < 0 {
            return result(
                sequence,
                ObservationStatus::Error,
                None,
                None,
                Some(ProbeError::io(std::io::Error::last_os_error())),
            );
        }
        let socket = unsafe { OwnedFd::from_raw_fd(raw) };
        let fd = socket.as_raw_fd();
        let configured = if v6 {
            option(fd, libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS, ttl as i32)
                .and_then(|_| option(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVERR, 1))
        } else {
            option(fd, libc::IPPROTO_IP, libc::IP_TTL, ttl as i32)
                .and_then(|_| option(fd, libc::IPPROTO_IP, libc::IP_RECVERR, 1))
        };
        if let Err(error) = configured {
            return result(sequence, ObservationStatus::Error, None, None, Some(error));
        }
        let (address, len) = socket_address(ip);
        let mut packet = [0u8; 32];
        packet[0] = if v6 { 128 } else { 8 };
        packet[6..8].copy_from_slice(&(sequence as u16).to_be_bytes());
        packet[8..16].copy_from_slice(b"NeonTrce");
        packet[16..24].copy_from_slice(&sequence.to_be_bytes());
        if !v6 {
            let check = checksum(&packet);
            packet[2..4].copy_from_slice(&check.to_be_bytes());
        }
        let start = Instant::now();
        let sent = unsafe {
            libc::sendto(
                fd,
                packet.as_ptr() as _,
                packet.len(),
                0,
                &address as *const _ as _,
                len,
            )
        };
        if sent < 0 {
            return result(
                sequence,
                ObservationStatus::Error,
                None,
                None,
                Some(ProbeError::io(std::io::Error::last_os_error())),
            );
        }
        while start.elapsed() < timeout {
            if cancel.load(Ordering::Acquire) {
                return result(sequence, ObservationStatus::Cancelled, None, None, None);
            }
            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLERR,
                revents: 0,
            };
            let remain = timeout.saturating_sub(start.elapsed()).as_millis().min(25) as i32;
            let polled = unsafe { libc::poll(&mut pollfd, 1, remain.max(1)) };
            if polled < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return result(
                    sequence,
                    ObservationStatus::Error,
                    None,
                    None,
                    Some(ProbeError::io(e)),
                );
            }
            if pollfd.revents & libc::POLLIN != 0 {
                let mut bytes = [0u8; 2048];
                let mut sender: libc::sockaddr_storage = unsafe { mem::zeroed() };
                let mut sender_len = mem::size_of_val(&sender) as libc::socklen_t;
                let n = unsafe {
                    libc::recvfrom(
                        fd,
                        bytes.as_mut_ptr() as _,
                        bytes.len(),
                        libc::MSG_DONTWAIT,
                        &mut sender as *mut _ as _,
                        &mut sender_len,
                    )
                };
                if n >= 32
                    && bytes[0] == if v6 { 129 } else { 0 }
                    && bytes[6..8] == packet[6..8]
                    && bytes[8..24] == packet[8..24]
                {
                    let responder =
                        unsafe { read_ip(&sender as *const _ as _, sender_len as usize) };
                    // Accept only echo replies from the pinned endpoint.
                    if responder == Some(ip) {
                        return result(
                            sequence,
                            ObservationStatus::Reply,
                            responder,
                            Some(start.elapsed().as_secs_f64() * 1000.0),
                            None,
                        );
                    }
                }
            }
            if pollfd.revents & libc::POLLERR != 0 {
                if let Some((status, responder, error)) = read_error(fd, v6) {
                    let timing = if responder.is_some() {
                        Some(start.elapsed().as_secs_f64() * 1000.0)
                    } else {
                        None
                    };
                    return result(sequence, status, responder, timing, error);
                }
            }
        }
        result(sequence, ObservationStatus::Timeout, None, None, None)
    }
    fn read_error(
        fd: i32,
        v6: bool,
    ) -> Option<(ObservationStatus, Option<IpAddr>, Option<ProbeError>)> {
        let mut payload = [0u8; 2048];
        let mut control = [0u64; 128];
        let mut source: libc::sockaddr_storage = unsafe { mem::zeroed() };
        let mut iov = libc::iovec {
            iov_base: payload.as_mut_ptr() as _,
            iov_len: payload.len(),
        };
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_name = &mut source as *mut _ as _;
        msg.msg_namelen = mem::size_of_val(&source) as _;
        msg.msg_control = control.as_mut_ptr() as _;
        msg.msg_controllen = mem::size_of_val(&control);
        let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT) };
        if n < 0 {
            return None;
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 {
            return None;
        }
        let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !cmsg.is_null() {
            let header = unsafe { &*cmsg };
            let expected_level = if v6 {
                libc::IPPROTO_IPV6
            } else {
                libc::IPPROTO_IP
            };
            let expected_type = if v6 {
                libc::IPV6_RECVERR
            } else {
                libc::IP_RECVERR
            };
            if header.cmsg_level == expected_level && header.cmsg_type == expected_type {
                let head_len = unsafe { libc::CMSG_LEN(0) } as usize;
                if header.cmsg_len < head_len + mem::size_of::<ExtendedError>() {
                    return None;
                }
                let data = unsafe { libc::CMSG_DATA(cmsg) };
                let ee = unsafe { std::ptr::read_unaligned(data as *const ExtendedError) };
                let offender_len = header.cmsg_len - head_len - mem::size_of::<ExtendedError>();
                let responder = unsafe {
                    read_ip(data.add(mem::size_of::<ExtendedError>()) as _, offender_len)
                };
                if ee.origin == if v6 { 3 } else { 2 } {
                    let status = if ee.kind == if v6 { 3 } else { 11 } {
                        ObservationStatus::TimeExceeded
                    } else {
                        ObservationStatus::Unreachable
                    };
                    return Some((status, responder, None));
                } else if ee.origin == 1 {
                    return Some((
                        ObservationStatus::Error,
                        None,
                        Some(ProbeError::io(std::io::Error::from_raw_os_error(
                            ee.errno as i32,
                        ))),
                    ));
                }
            }
            cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
        }
        None
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn checksum_matches_rfc1071_vector() {
            assert_eq!(checksum(&[0, 1, 0xf2, 3, 0xf4, 0xf5, 0xf6, 0xf7]), 0x220d);
        }
        #[test]
        fn sockaddr_roundtrip() {
            for s in ["192.0.2.17", "2001:db8::ff"] {
                let ip = s.parse().unwrap();
                let (a, n) = socket_address(ip);
                assert_eq!(
                    unsafe { read_ip(&a as *const _ as _, n as usize) },
                    Some(ip)
                );
            }
        }
        #[test]
        fn cancelled_probe_never_opens_socket() {
            let o = probe(
                "192.0.2.1".parse().unwrap(),
                64,
                1,
                Duration::from_secs(1),
                Arc::new(AtomicBool::new(true)),
            );
            assert_eq!(o.status, ObservationStatus::Cancelled);
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::{ffi::c_void, ptr};
    type Handle = *mut c_void;
    #[repr(C)]
    struct Options {
        ttl: u8,
        tos: u8,
        flags: u8,
        options_size: u8,
        options_data: *mut u8,
    }
    #[repr(C)]
    struct SockAddr6 {
        family: u16,
        port: u16,
        flow: u32,
        address: [u8; 16],
        scope: u32,
    }
    // Microsoft's IPV6_ADDRESS_EX is packed(1), unlike sockaddr_in6.
    // https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/NetworkManagement/IpHelper/struct.IPV6_ADDRESS_EX.html
    #[repr(C, packed(1))]
    #[derive(Clone, Copy)]
    struct Address6 {
        port: u16,
        flow: u32,
        address: [u16; 8],
        scope: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Reply6 {
        address: Address6,
        status: u32,
        rtt: u32,
    }
    fn decode_reply6(bytes: &[u8]) -> Option<(u32, IpAddr, u32)> {
        if bytes.len() < std::mem::size_of::<Reply6>() {
            return None;
        }
        let reply = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<Reply6>()) };
        // Copy before borrowing: packed address fields may be unaligned.
        let words = reply.address.address;
        let mut octets = [0u8; 16];
        for (index, word) in words.into_iter().enumerate() {
            octets[index * 2..index * 2 + 2].copy_from_slice(&word.to_ne_bytes());
        }
        Some((
            reply.status,
            IpAddr::V6(std::net::Ipv6Addr::from(octets)),
            reply.rtt,
        ))
    }
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn IcmpCreateFile() -> Handle;
        fn Icmp6CreateFile() -> Handle;
        fn IcmpCloseHandle(handle: Handle) -> i32;
        fn IcmpSendEcho2Ex(
            handle: Handle,
            event: Handle,
            apc: *mut c_void,
            context: *mut c_void,
            source: u32,
            destination: u32,
            data: *mut c_void,
            size: u16,
            options: *mut Options,
            reply: *mut c_void,
            reply_size: u32,
            timeout: u32,
        ) -> u32;
        fn Icmp6SendEcho2(
            handle: Handle,
            event: Handle,
            apc: *mut c_void,
            context: *mut c_void,
            source: *mut SockAddr6,
            destination: *mut SockAddr6,
            data: *mut c_void,
            size: u16,
            options: *mut Options,
            reply: *mut c_void,
            reply_size: u32,
            timeout: u32,
        ) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLastError() -> u32;
    }
    struct IcmpHandle(Handle);
    impl Drop for IcmpHandle {
        fn drop(&mut self) {
            unsafe {
                IcmpCloseHandle(self.0);
            }
        }
    }
    fn status(code: u32) -> ObservationStatus {
        match code {
            0 => ObservationStatus::Reply,
            11010 => ObservationStatus::Timeout,
            11013 | 11041 => ObservationStatus::TimeExceeded,
            11002..=11005 | 11018 | 11040 | 11045 => ObservationStatus::Unreachable,
            _ => ObservationStatus::Error,
        }
    }
    fn error(code: u32) -> ProbeError {
        ProbeError::new(
            match code {
                5 => ErrorCode::PermissionDenied,
                50 | 120 => ErrorCode::Unsupported,
                _ => ErrorCode::Socket,
            },
            format!("os_error_{code}"),
        )
    }
    pub fn probe(
        ip: IpAddr,
        ttl: u8,
        sequence: u64,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    ) -> ProbeObservation {
        if cancel.load(Ordering::Acquire) {
            return result(sequence, ObservationStatus::Cancelled, None, None, None);
        }
        let handle = unsafe {
            if ip.is_ipv6() {
                Icmp6CreateFile()
            } else {
                IcmpCreateFile()
            }
        };
        if handle as isize == -1 || handle.is_null() {
            return result(
                sequence,
                ObservationStatus::Error,
                None,
                None,
                Some(error(unsafe { GetLastError() })),
            );
        }
        let handle = IcmpHandle(handle);
        let mut options = Options {
            ttl,
            tos: 0,
            flags: 0,
            options_size: 0,
            options_data: ptr::null_mut(),
        };
        let mut payload = [0u8; 24];
        payload[..8].copy_from_slice(b"NeonTrce");
        payload[8..16].copy_from_slice(&sequence.to_be_bytes());
        // Aligned, comfortably larger than ICMP*_ECHO_REPLY + data + IO_STATUS_BLOCK.
        let mut reply = [0u64; 128];
        let count = unsafe {
            match ip {
                IpAddr::V4(ip) => IcmpSendEcho2Ex(
                    handle.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    u32::from_ne_bytes(ip.octets()),
                    payload.as_mut_ptr() as _,
                    payload.len() as _,
                    &mut options,
                    reply.as_mut_ptr() as _,
                    std::mem::size_of_val(&reply) as _,
                    timeout.as_millis().min(900) as _,
                ),
                IpAddr::V6(ip) => {
                    let mut source = SockAddr6 {
                        family: 23,
                        port: 0,
                        flow: 0,
                        address: [0; 16],
                        scope: 0,
                    };
                    let mut destination = SockAddr6 {
                        family: 23,
                        port: 0,
                        flow: 0,
                        address: ip.octets(),
                        scope: 0,
                    };
                    Icmp6SendEcho2(
                        handle.0,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut source,
                        &mut destination,
                        payload.as_mut_ptr() as _,
                        payload.len() as _,
                        &mut options,
                        reply.as_mut_ptr() as _,
                        std::mem::size_of_val(&reply) as _,
                        timeout.as_millis().min(900) as _,
                    )
                }
            }
        };
        if cancel.load(Ordering::Acquire) {
            return result(sequence, ObservationStatus::Cancelled, None, None, None);
        }
        if count == 0 {
            let code = unsafe { GetLastError() };
            let state = status(code);
            return result(
                sequence,
                state.clone(),
                None,
                None,
                if state == ObservationStatus::Error {
                    Some(error(code))
                } else {
                    None
                },
            );
        }
        let (code, responder, rtt) = if ip.is_ipv6() {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    reply.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(&reply),
                )
            };
            let Some(decoded) = decode_reply6(bytes) else {
                return result(
                    sequence,
                    ObservationStatus::Error,
                    None,
                    None,
                    Some(ProbeError::new(ErrorCode::Socket, "invalid_ipv6_reply")),
                );
            };
            decoded
        } else {
            let values = unsafe { std::slice::from_raw_parts(reply.as_ptr() as *const u32, 3) };
            (
                values[1],
                IpAddr::V4(std::net::Ipv4Addr::from(values[0].to_ne_bytes())),
                values[2],
            )
        };
        let state = status(code);
        let responder = if responder.is_unspecified() {
            None
        } else {
            Some(responder)
        };
        let timing = if responder.is_some()
            && matches!(
                state,
                ObservationStatus::Reply
                    | ObservationStatus::TimeExceeded
                    | ObservationStatus::Unreachable
            ) {
            Some(rtt as f64)
        } else {
            None
        };
        result(
            sequence,
            state.clone(),
            responder,
            timing,
            if state == ObservationStatus::Error {
                Some(error(code))
            } else {
                None
            },
        )
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn ipv6_reply_fixture_uses_packed_network_address_at_offset_six() {
            for address in ["::1", "2001:db8:1234:5678:90ab:cdef:1:2"] {
                let ip = address.parse::<std::net::Ipv6Addr>().unwrap();
                let mut bytes = [0u8; 36];
                bytes[2..6].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
                bytes[6..22].copy_from_slice(&ip.octets());
                bytes[22..26].copy_from_slice(&[0x90, 0xab, 0xcd, 0xef]);
                bytes[26..28].fill(0xff); // Outer Reply6 alignment padding.
                bytes[28..32].copy_from_slice(&11013u32.to_ne_bytes());
                bytes[32..36].copy_from_slice(&42u32.to_ne_bytes());
                assert_eq!(decode_reply6(&bytes), Some((11013, IpAddr::V6(ip), 42)));
            }
            assert!(decode_reply6(&[0u8; 35]).is_none());
        }
        #[test]
        fn windows_abi_offsets_match_sdk() {
            assert_eq!(std::mem::size_of::<SockAddr6>(), 28);
            assert_eq!(std::mem::size_of::<Address6>(), 26);
            assert_eq!(std::mem::offset_of!(Address6, address), 6);
            assert_eq!(std::mem::offset_of!(Reply6, status), 28);
            assert_eq!(std::mem::offset_of!(Reply6, rtt), 32);
            assert_eq!(std::mem::size_of::<Reply6>(), 36);
            assert_eq!(std::mem::size_of::<Options>(), 16);
        }
    }
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;
    pub fn probe(
        _ip: IpAddr,
        _ttl: u8,
        sequence: u64,
        _timeout: Duration,
        _cancel: Arc<AtomicBool>,
    ) -> ProbeObservation {
        result(
            sequence,
            ObservationStatus::Error,
            None,
            None,
            Some(ProbeError::new(
                ErrorCode::Unsupported,
                "unsupported_platform",
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Two local packets only. Supported CI hosts must produce actual echo
    /// replies; a restricted host must return a structured capability error.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn native_loopback_echo_ipv4_and_ipv6_or_explicit_capability_error() {
        for (sequence, address) in ["127.0.0.1", "::1"].into_iter().enumerate() {
            let ip = address.parse::<IpAddr>().unwrap();
            let observation = platform::probe(
                ip,
                64,
                sequence as u64 + 1,
                PROBE_TIMEOUT,
                Arc::new(AtomicBool::new(false)),
            );
            if observation.status == ObservationStatus::Reply {
                assert_eq!(observation.responder, Some(ip));
                assert!(
                    observation
                        .rtt_ms
                        .is_some_and(|rtt| rtt.is_finite() && rtt >= 0.0)
                );
                assert!(observation.error.is_none());
                eprintln!("native loopback {address}: actual ICMP echo reply");
            } else {
                let error = observation.error.as_ref().unwrap_or_else(|| {
                    panic!(
                        "native loopback {address}: unexpected nonreply {:?}",
                        observation.status
                    )
                });
                let capability_error = matches!(
                    error.code,
                    ErrorCode::PermissionDenied | ErrorCode::Unsupported
                ) || (ip.is_ipv6()
                    && error.code == ErrorCode::Socket
                    && error.detail == "os_error_97");
                assert!(
                    capability_error,
                    "native loopback {address}: unexpected error {error:?}"
                );
                assert_eq!(observation.status, ObservationStatus::Error);
                assert!(observation.rtt_ms.is_none());
                assert!(observation.responder.is_none());
                eprintln!("native loopback {address}: explicitly unavailable ({error:?})");
            }
        }
    }
    #[test]
    fn invalid_group_does_not_start_or_replace_sessions() {
        let mut manager = ProbeManager::new().unwrap();
        assert!(manager.start(Vec::new()).is_err());
        let configs = (0..11)
            .map(|id| ProbeConfig::new(id, "192.0.2.1", false))
            .collect();
        assert!(manager.start(configs).is_err());
        assert_eq!(manager.active_count(), 0);
    }
    #[test]
    fn duplicate_ids_and_invalid_targets_are_rejected_before_start() {
        let mut manager = ProbeManager::new().unwrap();
        assert!(
            manager
                .start(vec![
                    ProbeConfig::new(1, "192.0.2.1", false),
                    ProbeConfig::new(1, "192.0.2.2", false)
                ])
                .is_err()
        );
        assert!(
            manager
                .start(vec![ProbeConfig::new(1, "a;command", false)])
                .is_err()
        );
        assert_eq!(manager.active_count(), 0);
    }
    #[test]
    fn cancel_releases_waiters_and_respects_scheduler_limits() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let cancel = Arc::new(AtomicBool::new(false));
            let a = cancel.clone();
            let task = tokio::spawn(wait_cancel(a));
            cancel.store(true, Ordering::Release);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), task)
                    .await
                    .is_ok()
            );
        });
        assert_eq!(MAX_TARGETS, 10);
        assert_eq!(MAX_HOPS, 30);
        assert!(PROBE_TIMEOUT < PING_INTERVAL);
    }
}
