#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod appearance;
mod demo;
mod export;
mod geo;
mod i18n;
mod map;
mod map_view;
mod probe;
mod storage;
mod target;

use export::{
    Diagnostic, ExportBundle, GeoPoint, Hop, HopResponse, PingSample, SampleStatus, Session,
    TcpSample,
};
use i18n::{Key as K, Language};
use iced::widget::button as button_style;
use iced::widget::{
    Space, canvas, checkbox, column, container, opaque, pick_list, progress_bar, row,
    rule::horizontal as horizontal_rule, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Fill, Length, Size, Subscription, Task, Theme, window};
use map_view::{MapCache, MapNode, MapView, RouteVisual};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const JOURNEY: Duration = Duration::from_secs(8);
const SAMPLE_LIMIT: usize = 3600;
const GEO_LOCATION_LIMIT: usize = 512;

#[derive(Debug, Clone)]
enum Message {
    Tick(Instant),
    Input(String),
    Start,
    Stop,
    Retrace,
    Replay,
    Pause,
    Demo,
    ToggleMulti,
    OpenTargets,
    TargetInput(usize, String),
    AddTarget,
    RemoveTarget(usize),
    ApplyTargets,
    Select(usize),
    Settings,
    About,
    History,
    CloseModal,
    Language(Language),
    Tcp(bool),
    HistoryEnabled(bool),
    Effects(bool),
    Refresh,
    GeoDone(u64, Result<(), String>),
    GeoLookupDone(u64, u64, Vec<(IpAddr, geo::GeoLocation)>),
    NetworkSignature(u64, String),
    OriginDone(u64, geo::AddressFamily, Result<geo::OriginEstimate, String>),
    Export(&'static str),
    SavePath(&'static str, Option<PathBuf>),
    SaveDone(Result<(), String>),
    Screenshot(PathBuf, window::Screenshot),
    LoadHistory(String),
    DeleteHistory(String),
    ClearHistory,
    Copy(String),
    Theme(iced::theme::Mode),
    Window((window::Id, window::Event)),
    Keyboard(iced::keyboard::Event),
}
#[derive(Clone, Copy, PartialEq)]
enum Modal {
    Targets,
    Settings,
    About,
    History,
    Details,
}
struct Animation {
    start: Instant,
    paused: Option<Instant>,
    pause_total: Duration,
}
impl Animation {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            paused: None,
            pause_total: Duration::ZERO,
        }
    }
    fn new_paused(paused: bool) -> Self {
        let mut animation = Self::new();
        if paused {
            animation.paused = Some(animation.start)
        }
        animation
    }
    fn smooth(&self, now: Instant) -> f32 {
        (self
            .paused
            .unwrap_or(now)
            .saturating_duration_since(self.start)
            .saturating_sub(self.pause_total)
            .as_secs_f32()
            / JOURNEY.as_secs_f32())
        .clamp(0.0, 1.0)
    }
    fn toggle(&mut self) {
        let now = Instant::now();
        if let Some(at) = self.paused.take() {
            self.pause_total += now - at
        } else {
            self.paused = Some(now)
        }
    }
}
// The slot remains owned by the worker even if a new session cancels its task.
// Rapid Start/Stop cycles therefore cannot accumulate blocking MMDB jobs.
struct GeoLookupGuard(Arc<AtomicBool>);
impl Drop for GeoLookupGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct Presentation {
    sessions: Vec<Session>,
    single: Vec<Session>,
    multi: bool,
    saved: bool,
    selected: usize,
    targets: Vec<String>,
    input: String,
}
struct App {
    paths: storage::Paths,
    storage_available: bool,
    config: storage::Config,
    language: Language,
    history: storage::HistoryStore,
    manager: Option<probe::ProbeManager>,
    geo: Option<geo::GeoService>,
    geo_cancel: Arc<AtomicBool>,
    geo_epoch: u64,
    geo_rx: mpsc::Receiver<geo::GeoEvent>,
    geo_tx: mpsc::SyncSender<geo::GeoEvent>,
    geo_busy: bool,
    geo_status: K,
    geo_bytes: Option<(u64, Option<u64>)>,
    location_cache: HashMap<IpAddr, GeoPoint>,
    pending_geo: HashSet<IpAddr>,
    geo_lookup_busy: bool,
    geo_lookup_gate: Arc<AtomicBool>,
    geo_lookup_revision: u64,
    origin4: Option<geo::OriginEstimate>,
    origin6: Option<geo::OriginEstimate>,
    origins_requested: HashSet<bool>,
    sessions: Vec<Session>,
    retained_single: Vec<Session>,
    retained_demo: Option<Presentation>,
    route_ids: HashMap<u64, u64>,
    fatal_status: HashMap<u64, K>,
    multi_targets: Vec<String>,
    multi: bool,
    input: String,
    running: bool,
    demonstration: bool,
    saved: bool,
    selected: usize,
    animations: HashMap<u64, Animation>,
    presentation_paused: bool,
    routes: Vec<RouteVisual>,
    cache: MapCache,
    modal: Option<Modal>,
    fields: Vec<String>,
    field_errors: Vec<Option<K>>,
    status: K,
    detail: String,
    dark: bool,
    now: Instant,
    window_id: Option<window::Id>,
    history_entries: Vec<storage::HistoryEntry>,
    last_config_save: Instant,
    last_geo_check: Instant,
    #[cfg(windows)]
    last_theme_check: Instant,
    last_network_check: Instant,
    network_signature: Option<String>,
}
impl App {
    fn new() -> (Self, Task<Message>) {
        let available_paths = storage::Paths::discover().ok();
        let storage_available = available_paths.is_some();
        let paths = available_paths.unwrap_or_else(|| {
            let p = std::env::temp_dir().join("NeonTrace");
            storage::Paths {
                config_dir: p.join("config"),
                cache_dir: p.join("cache"),
                history_dir: p.join("history"),
            }
        });
        let mut config = if storage_available {
            paths.load_config().unwrap_or_default()
        } else {
            storage::Config::default()
        };
        if !storage_available {
            config.history_enabled = false;
        }
        let language = Language::from_code(&config.language);
        let history = storage::HistoryStore::new(paths.history_dir.clone(), config.history_enabled);
        let geo = if storage_available {
            geo::GeoService::open(paths.cache_dir.clone()).ok()
        } else {
            None
        };
        let geo_status = if geo.as_ref().is_some_and(|g| g.ready()) {
            K::DatabaseReady
        } else {
            K::DownloadOnFirstUse
        };
        let (geo_tx, geo_rx) = mpsc::sync_channel(32);
        let mut app = Self {
            paths,
            storage_available,
            config,
            language,
            history,
            manager: probe::ProbeManager::new().ok(),
            geo,
            geo_cancel: Arc::new(AtomicBool::new(false)),
            geo_epoch: 0,
            geo_rx,
            geo_tx,
            geo_busy: false,
            geo_status,
            geo_bytes: None,
            location_cache: HashMap::new(),
            pending_geo: HashSet::new(),
            geo_lookup_busy: false,
            geo_lookup_gate: Arc::new(AtomicBool::new(false)),
            geo_lookup_revision: 0,
            origin4: None,
            origin6: None,
            origins_requested: HashSet::new(),
            sessions: Vec::new(),
            retained_single: Vec::new(),
            retained_demo: None,
            route_ids: HashMap::new(),
            fatal_status: HashMap::new(),
            multi_targets: vec!["example.com".into()],
            multi: false,
            input: String::new(),
            running: false,
            demonstration: false,
            saved: false,
            selected: 0,
            animations: HashMap::new(),
            presentation_paused: false,
            routes: Vec::new(),
            cache: MapCache::default(),
            modal: None,
            fields: vec![String::new()],
            field_errors: vec![None],
            status: K::Idle,
            detail: String::new(),
            dark: true,
            now: Instant::now(),
            window_id: None,
            history_entries: Vec::new(),
            last_config_save: Instant::now(),
            last_geo_check: Instant::now(),
            #[cfg(windows)]
            last_theme_check: Instant::now(),
            last_network_check: Instant::now(),
            network_signature: None,
        };
        if std::env::args().any(|a| a == "--demo") {
            app.demonstration = true;
            app.sessions = demo::sessions(1);
            app.replay();
            app.status = K::DemoWarning;
        }
        (app, iced::system::theme().map(Message::Theme))
    }
    fn session_status(&self, s: &Session) -> K {
        if s.demonstration {
            return K::DemoLabel;
        }
        if self.saved {
            return K::SavedResult;
        }
        if let Some(k) = self.fatal_status.get(&s.target_id) {
            return *k;
        }
        if s.errors.iter().any(|e| e.code == "network_changed") {
            return K::NetworkChanged;
        }
        if s.errors.iter().any(|e| e.code == "Dns") {
            return K::ResolutionFailed;
        }
        if s.chosen_ip.is_none() {
            return if self.running {
                K::Resolving
            } else {
                K::Stopped
            };
        }
        if s.trace_complete && !s.destination_reached {
            return K::PartialRoute;
        }
        if self.running {
            K::Measuring
        } else {
            K::Stopped
        }
    }
    fn t(&self, k: K) -> &'static str {
        self.language.text(k)
    }
    fn seconds(&self, v: u64) -> String {
        self.language
            .format(K::SecondsValue, &[("value", &v.to_string())])
    }
    fn persist(&mut self) {
        if !self.storage_available {
            self.status = K::StorageError;
            return;
        }
        self.config.language = self.language.code().into();
        if self.paths.save_config(&self.config).is_err() {
            self.status = K::StorageError;
        }
        self.last_config_save = Instant::now();
    }
    fn bundle(&self) -> ExportBundle {
        ExportBundle {
            demonstration: self.demonstration,
            notes: format!(
                "{}\n{}",
                self.t(K::ObservedRoute),
                export::APPROXIMATION_NOTE
            ),
            sessions: self.sessions.clone(),
            ..Default::default()
        }
    }
    fn save_history(&mut self) {
        if !self.storage_available {
            return;
        }
        if !self.sessions.is_empty() && self.history.record(&self.bundle()).is_err() {
            self.status = K::StorageError;
        }
    }
    fn stop(&mut self) {
        self.status = K::Stopped;
        if let Some(m) = &mut self.manager {
            m.stop()
        }
        if self.running {
            for s in &mut self.sessions {
                s.ended_unix_ms = Some(export::now_ms());
            }
            self.running = false;
            self.save_history();
        }
    }
    fn replay(&mut self) {
        self.presentation_paused = false;
        self.animations.clear();
        for s in &self.sessions {
            self.animations
                .insert(s.target_id, Animation::new_paused(self.presentation_paused));
        }
        self.rebuild_routes();
    }
    fn refresh(&mut self, force: bool) -> Task<Message> {
        if self.demonstration || self.geo_busy {
            return Task::none();
        }
        let Some(service) = self.geo.clone() else {
            self.geo_status = K::DatabaseMissing;
            return Task::none();
        };
        if !force && !service.refresh_due() {
            return Task::none();
        }
        self.last_geo_check = Instant::now();
        self.geo_busy = true;
        self.geo_status = K::CheckingUpdates;
        self.geo_bytes = None;
        let cancel = self.geo_cancel.clone();
        let tx = self.geo_tx.clone();
        let epoch = self.geo_epoch;
        Task::perform(
            async move {
                service
                    .refresh(force, cancel, Some(tx))
                    .await
                    .map(|_| ())
                    .map_err(|e| e.code.to_string())
            },
            move |r| Message::GeoDone(epoch, r),
        )
    }
    fn request_origin(&mut self, family: geo::AddressFamily) -> Task<Message> {
        let key = family == geo::AddressFamily::V6;
        if !self.origins_requested.insert(key) {
            return Task::none();
        }
        let Some(service) = self.geo.clone() else {
            return Task::none();
        };
        let cancel = self.geo_cancel.clone();
        let epoch = self.geo_epoch;
        Task::perform(
            async move {
                service
                    .discover_origin(family, cancel)
                    .await
                    .map_err(|e| e.code.to_owned())
            },
            move |r| Message::OriginDone(epoch, family, r),
        )
    }
    fn start(&mut self) -> Task<Message> {
        if self.demonstration {
            self.sessions = demo::sessions(if self.multi {
                self.multi_targets.len()
            } else {
                1
            });
            self.replay();
            self.status = K::DemoWarning;
            return Task::none();
        }
        let inputs = if self.multi {
            self.multi_targets.clone()
        } else {
            vec![self.input.clone()]
        };
        let targets: Result<Vec<_>, _> = inputs.iter().map(|v| target::normalize(v)).collect();
        let targets = match targets {
            Ok(v) => v,
            Err(_) => {
                self.status = K::InvalidTarget;
                return Task::none();
            }
        };
        self.stop();
        let Some(manager) = &mut self.manager else {
            self.status = K::TraceUnavailable;
            return Task::none();
        };
        let configs = targets
            .iter()
            .enumerate()
            .map(|(i, t)| {
                probe::ProbeConfig::new(i as u64 + 1, t.canonical.clone(), self.config.tcp_check)
            })
            .collect();
        let gens = match manager.start(configs) {
            Ok(v) => v,
            Err(_) => {
                self.status = K::TraceUnavailable;
                return Task::none();
            }
        };
        self.sessions = targets
            .iter()
            .zip(gens)
            .map(|(t, (id, generation))| Session {
                target_id: id,
                generation,
                target: t.display.clone(),
                started_unix_ms: export::now_ms(),
                ..Default::default()
            })
            .collect();
        self.running = true;
        self.saved = false;
        self.selected = 0;
        self.animations.clear();
        self.route_ids = self.sessions.iter().map(|s| (s.target_id, 1)).collect();
        self.fatal_status.clear();
        self.routes.clear();
        self.status = K::Resolving;
        self.detail.clear();
        self.geo_epoch = self.geo_epoch.wrapping_add(1);
        self.geo_cancel.store(true, Ordering::Release);
        self.geo_cancel = Arc::new(AtomicBool::new(false));
        self.geo_busy = false;
        self.location_cache.clear();
        self.pending_geo.clear();
        self.geo_lookup_busy = false;
        self.geo_lookup_revision = self.geo_lookup_revision.wrapping_add(1);
        self.origins_requested.clear();
        self.origin4 = None;
        self.origin6 = None;
        self.network_signature = None;
        self.last_network_check = Instant::now() - Duration::from_secs(11);
        self.refresh(false)
    }
    fn geo_updates_allowed(&self) -> bool {
        !self.saved
            && !self.demonstration
            && !self
                .sessions
                .iter()
                .any(|s| s.errors.iter().any(|e| e.code == "network_changed"))
    }
    /// Current IPs only; saved route snapshots keep their original provenance.
    fn current_geo_ips(&self) -> HashSet<IpAddr> {
        let mut ips = HashSet::new();
        let mut add = |ip| {
            if ips.len() < GEO_LOCATION_LIMIT {
                ips.insert(ip);
            }
        };
        for origin in [&self.origin4, &self.origin6].into_iter().flatten() {
            add(origin.address);
        }
        for session in &self.sessions {
            if let Some(ip) = session.chosen_ip.as_deref().and_then(|v| v.parse().ok()) {
                add(ip);
            }
            for response in session.hops.iter().flat_map(|h| &h.responses) {
                if let Ok(ip) = response.address.parse() {
                    add(ip);
                }
            }
        }
        ips
    }
    fn queue_geo(&mut self, ip: IpAddr, refresh: bool) {
        if self.geo_updates_allowed()
            && (refresh || !self.location_cache.contains_key(&ip))
            && self.pending_geo.len() < GEO_LOCATION_LIMIT
        {
            self.pending_geo.insert(ip);
        }
    }
    fn queue_current_geo(&mut self) {
        if !self.geo_updates_allowed() {
            return;
        }
        let current = self.current_geo_ips();
        self.pending_geo.retain(|ip| current.contains(ip));
        for ip in current {
            self.queue_geo(ip, true);
        }
    }
    fn start_geo_lookup(&mut self) -> Task<Message> {
        if self.geo_lookup_busy || self.pending_geo.is_empty() || !self.geo_updates_allowed() {
            return Task::none();
        }
        let Some(service) = self.geo.clone() else {
            self.pending_geo.clear();
            return Task::none();
        };
        let current = self.current_geo_ips();
        self.pending_geo.retain(|ip| current.contains(ip));
        if self.pending_geo.is_empty()
            || self
                .geo_lookup_gate
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return Task::none();
        }
        let guard = GeoLookupGuard(self.geo_lookup_gate.clone());
        let batch: Vec<_> = self.pending_geo.drain().take(GEO_LOCATION_LIMIT).collect();
        if batch.is_empty() {
            return Task::none();
        }
        self.geo_lookup_busy = true;
        let epoch = self.geo_epoch;
        let revision = self.geo_lookup_revision;
        let locale = self.language.code();
        let cancel = self.geo_cancel.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let _guard = guard;
                    let mut results = Vec::with_capacity(batch.len());
                    for ip in batch {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        results.push((ip, service.lookup(ip, locale)));
                    }
                    results
                })
                .await
                .unwrap_or_default()
            },
            move |results| Message::GeoLookupDone(epoch, revision, results),
        )
    }
    fn apply_geo_lookup(
        &mut self,
        epoch: u64,
        revision: u64,
        results: Vec<(IpAddr, geo::GeoLocation)>,
    ) {
        // An older session must not release a newer session's in-flight slot.
        if epoch != self.geo_epoch {
            return;
        }
        self.geo_lookup_busy = false;
        if !self.geo_updates_allowed() {
            self.pending_geo.clear();
            return;
        }
        if revision != self.geo_lookup_revision {
            self.queue_current_geo();
            return;
        }
        let current = self.current_geo_ips();
        self.location_cache.retain(|ip, _| current.contains(ip));
        for (ip, location) in results.into_iter().take(GEO_LOCATION_LIMIT) {
            if current.contains(&ip)
                && (self.location_cache.contains_key(&ip)
                    || self.location_cache.len() < GEO_LOCATION_LIMIT)
            {
                self.location_cache.insert(ip, location_point(ip, location));
            }
        }
        self.enrich();
    }
    /// Presentation reads only the bounded in-memory cache; it never opens MMDB pages.
    fn point(&self, ip: IpAddr, origin: bool) -> Option<GeoPoint> {
        self.location_cache.get(&ip).cloned().map(|mut p| {
            p.estimated_network_origin = origin;
            p
        })
    }
    fn enrich(&mut self) {
        if !self.geo_updates_allowed() {
            return;
        }
        let previous: HashMap<u64, Vec<(f64, f64)>> = self
            .routes
            .iter()
            .map(|r| (r.id, r.nodes.iter().map(|n| (n.lat, n.lon)).collect()))
            .collect();
        let cache = &self.location_cache;
        let to_point = |ip: IpAddr, origin: bool| {
            cache.get(&ip).cloned().map(|mut p| {
                p.estimated_network_origin = origin;
                p
            })
        };
        for s in &mut self.sessions {
            if let Some(ip) = s.chosen_ip.as_ref().and_then(|v| v.parse::<IpAddr>().ok()) {
                s.endpoint = to_point(ip, false);
                let origin = if ip.is_ipv6() {
                    self.origin6.as_ref()
                } else {
                    self.origin4.as_ref()
                };
                s.origin = origin.and_then(|o| to_point(o.address, true));
            }
            for h in &mut s.hops {
                for r in &mut h.responses {
                    if let Ok(ip) = r.address.parse() {
                        r.location = to_point(ip, false);
                    }
                }
            }
        }
        self.rebuild_routes();
        for route in &mut self.routes {
            let points: Vec<_> = route.nodes.iter().map(|n| (n.lat, n.lon)).collect();
            if previous.get(&route.id) != Some(&points)
                && self
                    .sessions
                    .iter()
                    .any(|s| s.target_id == route.id && s.trace_complete)
            {
                self.animations
                    .insert(route.id, Animation::new_paused(self.presentation_paused));
                route.progress = 0.0;
            }
        }
    }
    fn event(&mut self, e: probe::ProbeEvent) -> Task<Message> {
        if !self.running || self.demonstration {
            return Task::none();
        }
        let Some(index) = self
            .sessions
            .iter()
            .position(|s| s.target_id == e.target_id && s.generation == e.generation)
        else {
            return Task::none();
        };
        use probe::ProbePayload as P;
        let route_id = match &e.payload {
            P::TraceStarted { route_id }
            | P::Hop { route_id, .. }
            | P::TraceComplete { route_id, .. } => Some(*route_id),
            _ => None,
        };
        if route_id.is_some_and(|r| {
            self.route_ids
                .get(&e.target_id)
                .is_some_and(|current| *current != r)
        }) {
            return Task::none();
        }
        match e.payload {
            P::Resolved {
                host,
                chosen,
                candidates,
            } => {
                self.queue_geo(chosen, false);
                let p = self.point(chosen, false);
                let s = &mut self.sessions[index];
                s.target = host;
                s.chosen_ip = Some(chosen.to_string());
                s.address_family = Some(if chosen.is_ipv6() { "IPv6" } else { "IPv4" }.into());
                s.resolved_candidates = candidates.iter().map(ToString::to_string).collect();
                s.endpoint = p;
                self.status = K::Measuring;
                return self.request_origin(if chosen.is_ipv6() {
                    geo::AddressFamily::V6
                } else {
                    geo::AddressFamily::V4
                });
            }
            P::Ping(p) => {
                let s = &mut self.sessions[index];
                s.pings.push(PingSample {
                    sequence: p.sequence,
                    observed_unix_ms: p.timestamp_ms,
                    status: if p.status == probe::ObservationStatus::Reply {
                        SampleStatus::Reply
                    } else if p.status == probe::ObservationStatus::TimeExceeded {
                        SampleStatus::Unavailable
                    } else {
                        sample_status(&p.status)
                    },
                    rtt_ms: if p.status == probe::ObservationStatus::Reply {
                        p.rtt_ms
                    } else {
                        None
                    },
                    detail: p.error.as_ref().map(|e| e.detail.clone()).or_else(|| {
                        (p.status == probe::ObservationStatus::TimeExceeded)
                            .then(|| "time_exceeded".into())
                    }),
                });
                if s.pings.len() > SAMPLE_LIMIT {
                    s.pings.remove(0);
                }
                if let Some(err) = p.error {
                    self.status = error_key(&err.code);
                    s.errors.push(Diagnostic {
                        observed_unix_ms: p.timestamp_ms,
                        code: format!("{:?}", err.code),
                        detail: err.detail,
                    });
                    if s.errors.len() > 64 {
                        s.errors.remove(0);
                    }
                    if matches!(
                        err.code,
                        probe::ErrorCode::PermissionDenied | probe::ErrorCode::Unsupported
                    ) {
                        self.fatal_status.insert(e.target_id, self.status);
                    }
                }
            }
            P::TraceStarted { route_id } => {
                self.animations.remove(&e.target_id);
                self.route_ids.insert(e.target_id, route_id);
                let s = &mut self.sessions[index];
                if let Some(ts) = s.route_timestamp_unix_ms {
                    if !s.hops.is_empty() {
                        s.route_snapshots.push(export::RouteSnapshot {
                            timestamp_unix_ms: ts,
                            complete: s.trace_complete,
                            reached: s.destination_reached,
                            hops: s.hops.clone(),
                        });
                        if s.route_snapshots.len() > 16 {
                            s.route_snapshots.remove(0);
                        }
                    }
                }
                s.hops.clear();
                s.route_timestamp_unix_ms = Some(e.timestamp_ms);
                s.trace_complete = false;
                s.destination_reached = false;
                self.status = K::Tracing;
            }
            P::Hop {
                route_id: _,
                observation: h,
            } => {
                let p = h.probe;
                if let Some(ip) = p.responder {
                    self.queue_geo(ip, false);
                }
                let loc = p.responder.and_then(|ip| self.point(ip, false));
                let s = &mut self.sessions[index];
                let hop = Hop {
                    ttl: h.hop,
                    observed_unix_ms: p.timestamp_ms,
                    status: sample_status(&p.status),
                    nonresponses: if p.responder.is_none() { 1 } else { 0 },
                    responses: p
                        .responder
                        .map(|ip| {
                            vec![HopResponse {
                                address: ip.to_string(),
                                rtt_ms: p.rtt_ms,
                                observed_unix_ms: p.timestamp_ms,
                                destination: s.chosen_ip.as_ref() == Some(&ip.to_string()),
                                location: loc,
                            }]
                        })
                        .unwrap_or_default(),
                };
                s.hops.push(hop);
                if let Some(err) = p.error {
                    self.status = error_key(&err.code);
                    if matches!(
                        err.code,
                        probe::ErrorCode::PermissionDenied | probe::ErrorCode::Unsupported
                    ) {
                        self.fatal_status.insert(e.target_id, self.status);
                    }
                    s.errors.push(Diagnostic {
                        observed_unix_ms: p.timestamp_ms,
                        code: format!("{:?}", err.code),
                        detail: err.detail,
                    });
                    if s.errors.len() > 64 {
                        s.errors.remove(0);
                    }
                }
            }
            P::TraceComplete {
                route_id: _,
                reached,
                ..
            } => {
                let s = &mut self.sessions[index];
                s.trace_complete = true;
                s.destination_reached = reached;
                self.animations
                    .insert(s.target_id, Animation::new_paused(self.presentation_paused));
                self.status = self
                    .fatal_status
                    .get(&e.target_id)
                    .copied()
                    .unwrap_or(if reached {
                        K::Measuring
                    } else {
                        K::PartialRoute
                    });
                self.save_history();
            }
            P::Tcp(t) => {
                let s = &mut self.sessions[index];
                s.tcp_checks.push(TcpSample {
                    observed_unix_ms: t.timestamp_ms,
                    port: 443,
                    status: if t.connected {
                        SampleStatus::Reply
                    } else {
                        SampleStatus::Unavailable
                    },
                    connection_ms: t.connect_ms,
                    detail: t.error,
                });
                if s.tcp_checks.len() > 360 {
                    s.tcp_checks.remove(0);
                }
            }
            P::Error(err) => {
                let terminal = matches!(
                    err.code,
                    probe::ErrorCode::Dns | probe::ErrorCode::InvalidTarget
                );
                self.status = error_key(&err.code);
                self.sessions[index].errors.push(Diagnostic {
                    observed_unix_ms: e.timestamp_ms,
                    code: format!("{:?}", err.code),
                    detail: err.detail,
                });
                if self.sessions[index].errors.len() > 64 {
                    self.sessions[index].errors.remove(0);
                }
                if terminal {
                    self.sessions[index].ended_unix_ms = Some(e.timestamp_ms);
                    if self.sessions.iter().all(|s| s.ended_unix_ms.is_some()) {
                        self.running = false;
                        if let Some(m) = &mut self.manager {
                            m.stop()
                        }
                    }
                }
            }
        }
        self.rebuild_routes();
        Task::none()
    }
    fn rebuild_routes(&mut self) {
        self.routes = self
            .sessions
            .iter()
            .map(|s| {
                let mut nodes = Vec::new();
                let mut unknown = false;
                let mut add = |p: &GeoPoint, label: String, missing: bool| -> bool {
                    if let (Some(lat), Some(lon)) = (p.latitude, p.longitude) {
                        if map::project(lat, lon).is_some() {
                            if nodes.last().is_none_or(|n: &MapNode| {
                                (n.lat - lat).abs() > 0.001 || (n.lon - lon).abs() > 0.001
                            }) {
                                nodes.push(MapNode {
                                    lat,
                                    lon,
                                    label,
                                    unknown_before: missing,
                                });
                            }
                            return true;
                        }
                    }
                    false
                };
                if let Some(p) = &s.origin {
                    add(
                        p,
                        p.city
                            .clone()
                            .or(p.country.clone())
                            .unwrap_or_else(|| self.t(K::Origin).into()),
                        false,
                    );
                } else {
                    unknown = true
                }
                for h in &s.hops {
                    if let Some(r) = h.responses.first() {
                        if let Some(p) = &r.location {
                            if add(
                                p,
                                p.city
                                    .clone()
                                    .or(p.country.clone())
                                    .unwrap_or_else(|| h.ttl.to_string()),
                                unknown,
                            ) {
                                unknown = false
                            } else {
                                unknown = true
                            }
                        } else {
                            unknown = true
                        }
                    } else {
                        unknown = true
                    }
                }
                // An untraced endpoint may be shown as a point; its dashed connector is explicitly unknown.
                if !s.destination_reached {
                    if let Some(p) = &s.endpoint {
                        add(p, p.city.clone().unwrap_or_else(|| s.target.clone()), true);
                    }
                }
                RouteVisual {
                    id: s.target_id,
                    label: s.target.clone(),
                    nodes,
                    progress: self
                        .animations
                        .get(&s.target_id)
                        .map(|a| a.smooth(self.now))
                        .unwrap_or(0.0),
                }
            })
            .collect();
    }
    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Tick(now) => {
                self.now = now;
                #[cfg(windows)]
                if self.last_theme_check.elapsed() >= Duration::from_secs(2) {
                    self.last_theme_check = now;
                    self.dark = appearance::dark_preference();
                }
                let mut tasks = Vec::new();
                let mut events = Vec::new();
                if let Some(m) = &mut self.manager {
                    for _ in 0..128 {
                        if let Some(e) = m.try_recv() {
                            events.push(e)
                        } else {
                            break;
                        }
                    }
                }
                for e in events {
                    tasks.push(self.event(e));
                }
                if self.running && !self.demonstration {
                    if self.last_geo_check.elapsed() > Duration::from_secs(3600) {
                        tasks.push(self.refresh(false));
                        self.last_geo_check = Instant::now();
                    }
                    if self.last_network_check.elapsed() > Duration::from_secs(10) {
                        self.last_network_check = Instant::now();
                        let epoch = self.geo_epoch;
                        tasks.push(Task::perform(
                            async {
                                tokio::task::spawn_blocking(network_signature)
                                    .await
                                    .unwrap_or_default()
                            },
                            move |s| Message::NetworkSignature(epoch, s),
                        ));
                    }
                }
                while let Ok(event) = self.geo_rx.try_recv() {
                    if self.demonstration {
                        continue;
                    }
                    match event {
                        geo::GeoEvent::Checking => self.geo_status = K::CheckingUpdates,
                        geo::GeoEvent::Downloading {
                            kind,
                            received,
                            total,
                        } => {
                            self.geo_status = if kind == geo::DatasetKind::City {
                                K::DownloadingCity
                            } else {
                                K::DownloadingAsn
                            };
                            self.geo_bytes = Some((received, total));
                        }
                        geo::GeoEvent::Validating { .. } => self.geo_status = K::ValidatingDatabase,
                        geo::GeoEvent::Ready { .. } => self.geo_status = K::DatabaseReady,
                        geo::GeoEvent::Failed { .. } => self.geo_status = K::DatabaseFailed,
                        _ => {}
                    }
                }
                tasks.push(self.start_geo_lookup());
                for r in &mut self.routes {
                    r.progress = self
                        .animations
                        .get(&r.id)
                        .map(|a| a.smooth(now))
                        .unwrap_or(0.0);
                }
                return Task::batch(tasks);
            }
            Message::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                use iced::keyboard::{Key, key::Named};
                match key {
                    Key::Named(Named::Tab) => {
                        return if modifiers.shift() {
                            iced::widget::operation::focus_previous()
                        } else {
                            iced::widget::operation::focus_next()
                        };
                    }
                    Key::Named(Named::Escape) => self.modal = None,
                    _ => {}
                }
            }
            Message::Input(v) => self.input = v,
            Message::Start => return self.start(),
            Message::Stop => self.stop(),
            Message::Retrace => {
                if self.running {
                    if let (Some(m), Some(s)) = (&self.manager, self.sessions.get(self.selected)) {
                        if m.trace_again(s.target_id) {
                            *self.route_ids.entry(s.target_id).or_insert(1) += 1;
                        }
                    }
                } else if self.demonstration {
                    self.replay()
                }
            }
            Message::Replay => self.replay(),
            Message::Pause => {
                self.presentation_paused = !self.presentation_paused;
                for a in self.animations.values_mut() {
                    if a.paused.is_some() != self.presentation_paused {
                        a.toggle()
                    }
                }
            }
            Message::Demo => {
                self.stop();
                self.geo_cancel.store(true, Ordering::Release);
                self.geo_epoch += 1;
                self.geo_busy = false;
                self.geo_lookup_busy = false;
                self.pending_geo.clear();
                if !self.demonstration {
                    self.retained_demo = Some(Presentation {
                        sessions: self.sessions.clone(),
                        single: self.retained_single.clone(),
                        multi: self.multi,
                        saved: self.saved,
                        selected: self.selected,
                        targets: self.multi_targets.clone(),
                        input: self.input.clone(),
                    });
                    self.demonstration = true;
                    self.saved = false;
                    self.sessions = demo::sessions(if self.multi {
                        self.multi_targets.len()
                    } else {
                        1
                    });
                    self.retained_single = demo::sessions(1);
                    self.status = K::DemoWarning;
                    self.replay();
                } else {
                    self.demonstration = false;
                    if let Some(p) = self.retained_demo.take() {
                        self.sessions = p.sessions;
                        self.retained_single = p.single;
                        self.multi = p.multi;
                        self.saved = p.saved;
                        self.selected = p.selected;
                        self.multi_targets = p.targets;
                        self.input = p.input;
                    } else {
                        self.sessions.clear();
                        self.retained_single.clear();
                        self.multi = false;
                        self.saved = false;
                    }
                    self.animations.clear();
                    self.status = if self.saved { K::SavedResult } else { K::Idle };
                    self.rebuild_routes();
                }
            }
            Message::ToggleMulti => {
                if self.multi {
                    self.stop();
                    self.multi = false;
                    self.sessions = std::mem::take(&mut self.retained_single);
                    self.selected = 0;
                    self.replay();
                } else {
                    self.fields = self.multi_targets.clone();
                    self.field_errors = vec![None; self.fields.len()];
                    self.modal = Some(Modal::Targets)
                }
            }
            Message::OpenTargets => {
                self.fields = self.multi_targets.clone();
                self.field_errors = vec![None; self.fields.len()];
                self.modal = Some(Modal::Targets)
            }
            Message::TargetInput(i, v) => {
                if let Some(f) = self.fields.get_mut(i) {
                    *f = v;
                    self.field_errors[i] = None
                }
            }
            Message::AddTarget => {
                if self.fields.len() < 10 {
                    self.fields.push(String::new());
                    self.field_errors.push(None)
                }
            }
            Message::RemoveTarget(i) => {
                if self.fields.len() > 1 && i < self.fields.len() {
                    self.fields.remove(i);
                    self.field_errors.remove(i);
                }
            }
            Message::ApplyTargets => {
                let mut targets = Vec::new();
                let mut seen = HashSet::new();
                let mut invalid = false;
                for (i, v) in self.fields.iter().enumerate() {
                    match target::normalize(v) {
                        Ok(t) => {
                            if seen.insert(t.canonical.clone()) {
                                targets.push(t.canonical)
                            }
                        }
                        Err(_) => {
                            self.field_errors[i] = Some(K::InvalidTarget);
                            invalid = true
                        }
                    }
                }
                if !invalid && !targets.is_empty() {
                    self.stop();
                    if !self.multi {
                        self.retained_single = self.sessions.clone()
                    }
                    self.multi = true;
                    self.multi_targets = targets;
                    self.sessions = if self.demonstration {
                        demo::sessions(self.multi_targets.len())
                    } else {
                        Vec::new()
                    };
                    self.selected = 0;
                    self.modal = None;
                    self.status = K::Idle;
                    self.replay();
                }
            }
            Message::Select(i) => {
                self.selected = i;
                if self.multi || self.config.window_width < 940.0 {
                    self.modal = Some(Modal::Details)
                }
            }
            Message::Settings => self.modal = Some(Modal::Settings),
            Message::About => self.modal = Some(Modal::About),
            Message::History => {
                match self.history.list() {
                    Ok(entries) => self.history_entries = entries,
                    Err(_) => {
                        self.history_entries.clear();
                        self.status = K::StorageError;
                    }
                }
                self.modal = Some(Modal::History)
            }
            Message::CloseModal => self.modal = None,
            Message::Language(v) => {
                self.language = v;
                self.geo_lookup_revision = self.geo_lookup_revision.wrapping_add(1);
                self.queue_current_geo();
                self.persist();
            }
            Message::Tcp(v) => {
                self.config.tcp_check = v;
                self.persist()
            }
            Message::HistoryEnabled(v) => {
                self.config.history_enabled = v && self.storage_available;
                self.history.set_enabled(self.config.history_enabled);
                self.persist()
            }
            Message::Effects(v) => {
                self.config.reduced_effects = v;
                self.persist()
            }
            Message::Refresh => return self.refresh(true),
            Message::NetworkSignature(epoch, signature) => {
                if epoch == self.geo_epoch && self.running && !self.saved && !self.demonstration {
                    let changed = self
                        .network_signature
                        .as_ref()
                        .is_some_and(|old| old != &signature);
                    self.network_signature = Some(signature);
                    if changed {
                        for session in &mut self.sessions {
                            if let Some(origin) = &mut session.origin {
                                origin.stale = true
                            }
                            session.errors.push(Diagnostic {
                                observed_unix_ms: export::now_ms(),
                                code: "network_changed".into(),
                                detail: "local_route_changed".into(),
                            });
                            if session.errors.len() > 64 {
                                session.errors.remove(0);
                            }
                        }
                        self.stop();
                        self.pending_geo.clear();
                        self.status = K::NetworkChanged;
                        self.origin4 = None;
                        self.origin6 = None;
                        self.origins_requested.clear();
                        let v4 = self
                            .sessions
                            .iter()
                            .any(|s| s.address_family.as_deref() == Some("IPv4"));
                        let v6 = self
                            .sessions
                            .iter()
                            .any(|s| s.address_family.as_deref() == Some("IPv6"));
                        let mut tasks = Vec::new();
                        if v4 {
                            tasks.push(self.request_origin(geo::AddressFamily::V4))
                        }
                        if v6 {
                            tasks.push(self.request_origin(geo::AddressFamily::V6))
                        }
                        return Task::batch(tasks);
                    }
                }
            }
            Message::GeoDone(epoch, r) => {
                if epoch == self.geo_epoch && !self.demonstration {
                    self.geo_busy = false;
                    self.geo_bytes = None;
                    self.geo_status = if r.is_ok() {
                        K::DatabaseReady
                    } else {
                        K::DatabaseFailed
                    };
                    self.geo_lookup_revision = self.geo_lookup_revision.wrapping_add(1);
                    self.queue_current_geo();
                }
            }
            Message::GeoLookupDone(epoch, revision, results) => {
                self.apply_geo_lookup(epoch, revision, results);
            }
            Message::OriginDone(epoch, family, result) => {
                if epoch == self.geo_epoch && !self.demonstration {
                    match result {
                        Ok(v) => {
                            let address = v.address;
                            let location = v.location.clone();
                            if family == geo::AddressFamily::V6 {
                                self.origin6 = Some(v)
                            } else {
                                self.origin4 = Some(v)
                            }
                            if self.geo_updates_allowed() {
                                let current = self.current_geo_ips();
                                self.location_cache.retain(|ip, _| current.contains(ip));
                                if self.location_cache.contains_key(&address)
                                    || self.location_cache.len() < GEO_LOCATION_LIMIT
                                {
                                    self.location_cache
                                        .insert(address, location_point(address, location));
                                }
                                // Discovery can finish after a database-edition/locale change.
                                self.queue_geo(address, true);
                                self.enrich();
                            }
                        }
                        Err(_) => {
                            if self.running {
                                self.status = K::OriginUnavailable
                            }
                        }
                    }
                }
            }
            Message::Export(kind) => {
                self.modal = None;
                let title = self
                    .t(match kind {
                        "png" => K::ExportPng,
                        "csv" => K::ExportCsv,
                        _ => K::ExportJson,
                    })
                    .to_owned();
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title(title)
                            .set_file_name(format!("NeonTrace-{}.{}", export::now_ms(), kind))
                            .add_filter(kind, &[kind])
                            .save_file()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    move |p| Message::SavePath(kind, p),
                );
            }
            Message::SavePath(kind, Some(path)) => {
                if kind == "png" {
                    if let Some(id) = self.window_id {
                        return window::screenshot(id)
                            .map(move |s| Message::Screenshot(path.clone(), s));
                    }
                } else {
                    let bundle = self.bundle();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                if kind == "csv" {
                                    export::write_csv(&path, &bundle)
                                } else {
                                    export::write_json(&path, &bundle)
                                }
                            })
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r.map_err(|e| e.to_string()))
                        },
                        Message::SaveDone,
                    );
                }
            }
            Message::Screenshot(path, s) => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || save_png(path, s))
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r)
                    },
                    Message::SaveDone,
                );
            }
            Message::SaveDone(r) => {
                self.status = if r.is_ok() {
                    K::ExportSucceeded
                } else {
                    K::ExportFailed
                }
            }
            Message::LoadHistory(id) => match self.history.load(&id) {
                Ok(b) => {
                    self.stop();
                    self.geo_cancel.store(true, Ordering::Release);
                    self.geo_epoch += 1;
                    self.geo_busy = false;
                    self.geo_lookup_busy = false;
                    self.pending_geo.clear();
                    self.location_cache.clear();
                    self.retained_demo = None;
                    self.demonstration = b.demonstration;
                    self.saved = true;
                    self.sessions = b.sessions;
                    self.multi = self.sessions.len() > 1;
                    if self.multi {
                        self.multi_targets =
                            self.sessions.iter().map(|s| s.target.clone()).collect()
                    } else if let Some(s) = self.sessions.first() {
                        self.input = s.target.clone();
                    }
                    self.selected = 0;
                    self.modal = None;
                    self.status = K::SavedResult;
                    self.replay();
                }
                Err(_) => self.status = K::StorageError,
            },
            Message::DeleteHistory(id) => {
                if self.history.delete(&id).is_err() {
                    self.status = K::StorageError
                }
                self.history_entries = self.history.list().unwrap_or_default();
            }
            Message::ClearHistory => {
                if self.history.clear().is_err() {
                    self.status = K::StorageError
                }
                self.history_entries = self.history.list().unwrap_or_default();
            }
            Message::Copy(v) => return iced::clipboard::write(v),
            Message::Theme(mode) => self.dark = mode != iced::theme::Mode::Light,
            Message::Window((id, event)) => {
                self.window_id = Some(id);
                match event {
                    window::Event::Resized(size) => {
                        self.config.window_width = size.width;
                        self.config.window_height = size.height;
                        self.last_config_save = Instant::now();
                    }
                    window::Event::CloseRequested => {
                        self.stop();
                        self.persist();
                        self.geo_cancel.store(true, Ordering::Release);
                        return window::close(id);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let active = self.routes.iter().any(|r| r.progress < 1.0)
            && self.animations.values().any(|a| a.paused.is_none());
        Subscription::batch([
            iced::time::every(Duration::from_millis(if active {
                if self.config.reduced_effects { 33 } else { 16 }
            } else {
                250
            }))
            .map(Message::Tick),
            window::events().map(Message::Window),
            iced::system::theme_changes().map(Message::Theme),
            iced::keyboard::listen().map(Message::Keyboard),
        ])
    }
    fn theme(&self) -> Theme {
        let palette = iced::theme::Palette {
            background: if self.dark {
                Color::from_rgb8(9, 11, 15)
            } else {
                Color::from_rgb8(235, 239, 243)
            },
            text: if self.dark {
                Color::from_rgb8(235, 239, 245)
            } else {
                Color::from_rgb8(30, 41, 53)
            },
            primary: Color::from_rgb8(43, 135, 229),
            success: Color::from_rgb8(57, 186, 113),
            warning: Color::from_rgb8(207, 149, 63),
            danger: Color::from_rgb8(212, 83, 87),
        };
        Theme::custom("NeonTrace", palette)
    }
    fn view(&self) -> Element<'_, Message> {
        let narrow = self.config.window_width < 940.0;
        let brand = row![
            text("NeonTrace").size(27).color(if self.dark {
                Color::from_rgb8(225, 237, 251)
            } else {
                Color::from_rgb8(29, 57, 82)
            }),
            Space::new().width(Fill),
            button(self.t(K::Settings)).on_press(Message::Settings),
            button(self.t(K::About)).on_press(Message::About)
        ]
        .spacing(10)
        .align_y(iced::Center);
        let target_field: Element<_> = if self.multi {
            text(self.language.format(
                K::CountTargets,
                &[("count", &self.multi_targets.len().to_string())],
            ))
            .size(16)
            .into()
        } else {
            text_input(self.t(K::TargetHint), &self.input)
                .on_input(Message::Input)
                .on_submit(Message::Start)
                .padding(11)
                .into()
        };
        let controls = row![
            target_field,
            button(self.t(if self.running { K::Stop } else { K::Start }))
                .on_press(if self.running {
                    Message::Stop
                } else {
                    Message::Start
                })
                .padding([10, 18])
                .style(if self.running {
                    button_style::danger
                } else {
                    button_style::primary
                }),
            button(self.t(if self.multi {
                K::SingleTarget
            } else {
                K::MultiTarget
            }))
            .on_press(Message::ToggleMulti)
        ]
        .spacing(8)
        .align_y(iced::Center);
        let mut playback = row![
            button(self.t(K::Replay)).on_press(Message::Replay),
            button(self.t(if self.presentation_paused {
                K::Resume
            } else {
                K::Pause
            }))
            .on_press(Message::Pause)
        ]
        .spacing(7);
        if narrow && !self.multi {
            playback = playback
                .push(Space::new().width(Fill))
                .push(button(self.t(K::Details)).on_press(Message::Select(self.selected)));
        }
        let actions = row![
            button(self.t(if self.demonstration {
                K::ExitDemo
            } else {
                K::Demo
            }))
            .on_press(Message::Demo)
            .style(button_style::secondary),
            Space::new().width(Fill),
            button(self.t(K::History)).on_press(Message::History),
            button(self.t(K::Export)).on_press(Message::Settings)
        ]
        .spacing(7)
        .align_y(iced::Center);
        let secondary: Element<_> = if narrow {
            column![playback, actions].spacing(6).into()
        } else {
            row![playback, actions.width(Fill)].spacing(12).into()
        };
        let mut mapcol = column![
            row![
                text(self.t(if self.demonstration {
                    K::DemoWarning
                } else if self.saved {
                    K::SavedResult
                } else {
                    K::ObservedRoute
                }))
                .size(12),
                Space::new().width(Fill),
                text(self.language.format(K::SecondsValue, &[("value", "8")])).size(12)
            ]
            .spacing(8),
            canvas(MapView {
                cache: &self.cache,
                routes: &self.routes,
                selected_id: self.sessions.get(self.selected).map(|s| s.target_id),
                dark: self.dark
            })
            .width(Fill)
            .height(Fill),
            text(self.t(K::Schematic)).size(11),
            text("Simplemaps · IP Geolocation by DB-IP · CC BY 4.0 · db-ip.com").size(9)
        ]
        .spacing(6)
        .height(Fill);
        if self.geo_busy && !self.demonstration {
            let transferred = self
                .geo_bytes
                .map(|(bytes, total)| {
                    format!(
                        "{:.1} MB / {}",
                        bytes as f64 / 1e6,
                        total
                            .map(|n| format!("{:.1} MB", n as f64 / 1e6))
                            .unwrap_or_else(|| "—".into())
                    )
                })
                .unwrap_or_default();
            mapcol = mapcol.push(
                row![
                    text(self.t(self.geo_status)).size(11).width(Fill),
                    text(transferred).size(11)
                ]
                .spacing(8),
            );
            if let Some((received, Some(total))) = self.geo_bytes {
                if total > 0 {
                    mapcol = mapcol.push(
                        progress_bar(
                            0.0..=1.0,
                            (received as f64 / total as f64).clamp(0.0, 1.0) as f32,
                        )
                        .girth(3),
                    );
                }
            }
        }
        if self.multi {
            let mut targets =
                column![button(self.t(K::ConfigureTargets)).on_press(Message::OpenTargets)]
                    .spacing(5);
            for (i, s) in self.sessions.iter().enumerate() {
                let stats = s.statistics();
                targets = targets.push(
                    button(
                        row![
                            text(format!("{}  {}", i + 1, s.target)).width(Fill),
                            text(ms(s.pings.last().and_then(|p| p.rtt_ms))).size(13),
                            text(self.t(self.session_status(s))).size(11),
                            text(format!("{}/{}", stats.replies, stats.sent)).size(12)
                        ]
                        .spacing(8),
                    )
                    .on_press(Message::Select(i))
                    .width(Fill),
                );
            }
            mapcol = mapcol.push(scrollable(targets).height(Length::Fixed(if narrow {
                115.0
            } else {
                140.0
            })));
        }
        let map_panel = container(mapcol)
            .padding(14)
            .width(Fill)
            .height(Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(iced::Background::Color(if self.dark {
                    Color::from_rgb8(7, 14, 24)
                } else {
                    Color::from_rgb8(207, 221, 232)
                })),
                border: iced::Border {
                    color: if self.dark {
                        Color::from_rgb8(28, 48, 68)
                    } else {
                        Color::from_rgb8(162, 183, 202)
                    },
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..Default::default()
            });
        let center: Element<_> = if !self.multi && !narrow {
            row![
                map_panel,
                container(scrollable(container(self.inspector()).padding(
                    iced::Padding {
                        right: 14.0,
                        ..Default::default()
                    }
                )))
                .padding(12)
                .width(Length::Fixed(268.0))
                .height(Fill)
                .style(container::rounded_box)
            ]
            .spacing(12)
            .height(Fill)
            .into()
        } else {
            map_panel.into()
        };
        let mut body = column![brand, controls, secondary, center]
            .spacing(if narrow { 8 } else { 12 })
            .padding(if narrow { 10 } else { 18 })
            .height(Fill);
        if !self.multi {
            body = body.push(self.hop_strip(narrow));
        }
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        body = body.push(
            row![
                text(self.t(self.status)).size(11).width(Fill),
                text(ts).size(11)
            ]
            .spacing(10),
        );
        let base: Element<_> = body.into();
        if let Some(modal) = self.modal {
            stack![
                base,
                opaque(
                    container(
                        container(self.modal_view(modal))
                            .padding(22)
                            .width(Length::FillPortion(1))
                            .max_width(650)
                            .max_height((self.config.window_height - 36.0).max(300.0))
                            .style(container::bordered_box)
                    )
                    .center_x(Fill)
                    .center_y(Fill)
                    .width(Fill)
                    .height(Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba8(0, 0, 0, 0.7).into()),
                        ..Default::default()
                    })
                )
            ]
            .into()
        } else {
            base
        }
    }
    fn inspector(&self) -> Element<'_, Message> {
        let Some(s) = self.sessions.get(self.selected) else {
            return column![
                text(self.t(K::Endpoint)).size(21),
                text(self.t(K::Idle)),
                horizontal_rule(1),
                text(self.t(K::DownloadOnFirstUse)).size(13),
                text(self.t(K::PrivacyNote)).size(12)
            ]
            .spacing(15)
            .into();
        };
        let stats = s.statistics();
        let mut content = column![
            text(self.t(K::Endpoint)).size(19),
            text(&s.target)
                .size(18)
                .wrapping(iced::widget::text::Wrapping::Glyph),
            text(self.t(self.session_status(s))).size(12),
            text(self.t(K::SelectedAddress)).size(11),
            button(
                text(s.chosen_ip.as_deref().unwrap_or(self.t(K::Unknown)))
                    .size(13)
                    .wrapping(iced::widget::text::Wrapping::Glyph)
            )
            .width(Fill)
            .on_press(Message::Copy(s.chosen_ip.clone().unwrap_or_default()))
            .style(button_style::text),
            text(s.address_family.as_deref().unwrap_or("—")).size(12),
            horizontal_rule(1),
            text(self.t(K::CurrentRtt)).size(12),
            text(ms(s.pings.last().and_then(|p| p.rtt_ms))).size(35)
        ]
        .spacing(9);
        for (k, v) in [
            (K::Minimum, ms(stats.minimum_ms)),
            (K::Average, ms(stats.average_ms)),
            (K::Maximum, ms(stats.maximum_ms)),
            (K::Replies, stats.replies.to_string()),
            (K::Nonresponses, stats.nonresponses.to_string()),
            (K::Samples, stats.sent.to_string()),
            (
                K::Elapsed,
                self.seconds(
                    s.ended_unix_ms
                        .unwrap_or_else(export::now_ms)
                        .saturating_sub(s.started_unix_ms)
                        / 1000,
                ),
            ),
            (
                K::RouteAge,
                s.route_timestamp_unix_ms
                    .map(|t| self.seconds(export::now_ms().saturating_sub(t) / 1000))
                    .unwrap_or_else(|| "—".into()),
            ),
        ] {
            content = content
                .push(row![text(self.t(k)).size(12).width(Fill), text(v).size(12)].spacing(8));
        }
        content = content
            .push(text(self.t(K::SampleWindow)).size(10))
            .push(horizontal_rule(1))
            .push(
                text(self.t(if s.destination_reached {
                    K::Complete
                } else {
                    K::PartialRoute
                }))
                .size(12),
            )
            .push(
                button(self.t(K::TraceAgain))
                    .on_press(Message::Retrace)
                    .width(Fill),
            );
        if let Some(t) = s.tcp_checks.last() {
            content = content.push(
                text(format!(
                    "TCP 443 · {} · {}",
                    self.t(if t.status == SampleStatus::Reply {
                        K::Complete
                    } else {
                        K::Unavailable
                    }),
                    ms(t.connection_ms)
                ))
                .size(12),
            );
        }
        content = content.push(text(self.t(K::Origin)).size(13));
        if let Some(p) = &s.origin {
            content = content
                .push(text(location(p, self.t(K::LocationUnknown))).size(12))
                .push(text(p.ip.clone().unwrap_or_default()).size(12));
        } else {
            content = content
                .push(
                    text(format!(
                        "○ {} · {}",
                        self.t(K::YourPc),
                        self.t(K::LocationUnknown)
                    ))
                    .size(12),
                )
                .push(text(self.t(K::OriginUnavailable)).size(12));
        }
        content = content.push(text(self.t(K::EstimatedOrigin)).size(11));
        if let Some(p) = &s.endpoint {
            content = content
                .push(text(self.t(K::Endpoint)).size(13))
                .push(text(location(p, self.t(K::LocationUnknown))).size(12));
            if let Some(asn) = p.asn {
                content = content.push(
                    text(format!(
                        "AS{asn} · {}",
                        p.organization.as_deref().unwrap_or(self.t(K::Unknown))
                    ))
                    .size(11),
                );
            }
            if let Some(provider) = &p.provider {
                content =
                    content.push(text(format!("{}: {provider}", self.t(K::DataSource))).size(10));
            }
            if p.city_edition.is_some() || p.asn_edition.is_some() {
                content = content.push(
                    text(format!(
                        "{}: {} / {}",
                        self.t(K::DataEdition),
                        p.city_edition.as_deref().unwrap_or("—"),
                        p.asn_edition.as_deref().unwrap_or("—")
                    ))
                    .size(10),
                );
            }
        }

        for h in &s.hops {
            let address = h
                .responses
                .first()
                .map(|r| r.address.clone())
                .unwrap_or_else(|| self.t(K::NoReply).into());
            content = content.push(
                text(format!("{} {} · {}", self.t(K::Hop), h.ttl, address))
                    .size(12)
                    .wrapping(iced::widget::text::Wrapping::Glyph),
            );
            for r in &h.responses {
                content = content.push(
                    button(
                        text(format!(
                            "{} · {}",
                            r.location
                                .as_ref()
                                .map(|p| location(p, self.t(K::LocationUnknown)))
                                .unwrap_or_else(|| self.t(K::LocationUnknown).into()),
                            ms(r.rtt_ms)
                        ))
                        .size(11),
                    )
                    .on_press(Message::Copy(r.address.clone()))
                    .style(button_style::text),
                );
                if let Some(p) = &r.location {
                    if let Some(asn) = p.asn {
                        content = content.push(
                            text(format!(
                                "AS{asn} · {}",
                                p.organization.as_deref().unwrap_or(self.t(K::Unknown))
                            ))
                            .size(10),
                        );
                    }
                }
            }
        }
        if !s.errors.is_empty() {
            content = content.push(text(self.t(K::TechnicalDetails)).size(12));
            for e in s.errors.iter().rev().take(3) {
                content = content.push(text(format!("{}: {}", e.code, e.detail)).size(11));
            }
        }
        content.into()
    }
    fn hop_strip(&self, _narrow: bool) -> Element<'_, Message> {
        let Some(route) = self.routes.get(self.selected) else {
            return text(self.t(K::NotDeliveryProgress)).size(11).into();
        };
        if route.nodes.len() < 2 {
            return text(self.t(K::LocationUnknown)).size(12).into();
        }
        let count = route.nodes.len() - 1;
        let mut bars = row![].spacing(8);
        for i in 0..count {
            let fill = (route.progress * count as f32 - i as f32).clamp(0.0, 1.0);
            bars = bars.push(
                column![
                    text(format!("{:02}  {}", i + 1, route.nodes[i + 1].label)).size(11),
                    progress_bar(0.0..=1.0, fill)
                        .girth(4)
                        .style(iced::widget::progress_bar::success),
                    text(format!("{:.0}%", fill * 100.0)).size(10)
                ]
                .spacing(5)
                .width(if count > 6 {
                    Length::Fixed(155.0)
                } else {
                    Length::Fill
                }),
            );
        }
        let content: Element<_> = if count > 6 {
            scrollable(bars)
                .direction(iced::widget::scrollable::Direction::Horizontal(
                    iced::widget::scrollable::Scrollbar::default(),
                ))
                .height(50)
                .into()
        } else {
            container(bars.width(Fill)).height(50).width(Fill).into()
        };
        column![
            text(self.t(K::AnimationProgress)).size(11),
            content,
            text(self.t(K::NotDeliveryProgress)).size(10)
        ]
        .spacing(5)
        .into()
    }
    fn modal_view(&self, modal: Modal) -> Element<'_, Message> {
        let title = match modal {
            Modal::Targets => K::ConfigureTargets,
            Modal::Settings => K::Settings,
            Modal::About => K::About,
            Modal::History => K::History,
            Modal::Details => K::Details,
        };
        let mut c = column![
            row![
                text(self.t(title)).size(24).width(Fill),
                button(self.t(K::Close)).on_press(Message::CloseModal)
            ]
            .spacing(10),
            horizontal_rule(1)
        ]
        .spacing(14);
        match modal {
            Modal::Targets => {
                c = c.push(text(self.t(K::TargetListHint)).size(13));
                let mut entries = column![].spacing(10);
                for (i, value) in self.fields.iter().enumerate() {
                    let mut entry = column![
                        row![
                            text((i + 1).to_string()).size(15),
                            text_input(self.t(K::TargetHint), value)
                                .on_input(move |v| Message::TargetInput(i, v))
                                .padding(9),
                            button(self.t(K::Remove)).on_press(Message::RemoveTarget(i))
                        ]
                        .spacing(7)
                        .align_y(iced::Center)
                    ]
                    .spacing(4);
                    if let Some(k) = self.field_errors.get(i).and_then(|v| *v) {
                        entry = entry.push(
                            text(self.t(k))
                                .size(11)
                                .color(Color::from_rgb8(210, 87, 86)),
                        )
                    }
                    entries = entries.push(entry);
                }
                c = c.push(scrollable(entries).height(Length::Fixed(
                    (self.fields.len() as f32 * 74.0).min(self.config.window_height - 270.0),
                )));
                c = c.push(
                    row![
                        button(self.t(K::AddTarget))
                            .on_press_maybe((self.fields.len() < 10).then_some(Message::AddTarget)),
                        Space::new().width(Fill),
                        button(self.t(K::Cancel)).on_press(Message::CloseModal),
                        button(self.t(K::Apply))
                            .on_press(Message::ApplyTargets)
                            .style(button_style::primary)
                    ]
                    .spacing(8),
                );
            }
            Modal::Settings => {
                c = c
                    .push(
                        row![
                            text(self.t(K::Language)).width(Fill),
                            pick_list(Language::ALL, Some(self.language), Message::Language)
                        ]
                        .spacing(10),
                    )
                    .push(text(self.t(K::SystemTheme)).size(12))
                    .push(
                        checkbox(self.config.tcp_check)
                            .label(self.t(K::TcpCheck))
                            .on_toggle(Message::Tcp),
                    )
                    .push(text(self.t(K::TcpNote)).size(11))
                    .push(
                        checkbox(self.config.history_enabled)
                            .label(self.t(K::EnableHistory))
                            .on_toggle(Message::HistoryEnabled),
                    )
                    .push(
                        text(self.t(if self.config.history_enabled {
                            K::HistoryRetention
                        } else {
                            K::HistoryDisabled
                        }))
                        .size(11),
                    )
                    .push(
                        checkbox(self.config.reduced_effects)
                            .label(self.t(K::ReducedEffects))
                            .on_toggle(Message::Effects),
                    )
                    .push(horizontal_rule(1))
                    .push(text(self.t(K::LocationData)).size(17))
                    .push(text(self.t(self.geo_status)).size(12));
                if let Some((bytes, total)) = self.geo_bytes {
                    c = c.push(
                        text(format!(
                            "{:.1} MB / {}",
                            bytes as f64 / 1e6,
                            total
                                .map(|v| format!("{:.1} MB", v as f64 / 1e6))
                                .unwrap_or_else(|| "—".into())
                        ))
                        .size(12),
                    );
                }
                if let Some(g) = &self.geo {
                    for e in g.editions() {
                        c = c.push(
                            text(format!(
                                "DB-IP {} · {} · {}",
                                self.t(if e.kind == geo::DatasetKind::City {
                                    K::CityDatabase
                                } else {
                                    K::AsnDatabase
                                }),
                                e.edition,
                                chrono::DateTime::from_timestamp(e.installed_unix as i64, 0)
                                    .map(|t| t.format("%Y-%m-%d").to_string())
                                    .unwrap_or_default()
                            ))
                            .size(11),
                        );
                    }
                }
                c = c
                    .push(button(self.t(K::Refresh)).on_press_maybe(
                        (!self.geo_busy && !self.demonstration).then_some(Message::Refresh),
                    ))
                    .push(horizontal_rule(1))
                    .push(
                        row![
                            button(self.t(K::ExportPng)).on_press(Message::Export("png")),
                            button(self.t(K::ExportJson)).on_press(Message::Export("json")),
                            button(self.t(K::ExportCsv)).on_press(Message::Export("csv"))
                        ]
                        .spacing(8),
                    )
                    .push(text(self.t(K::PrivacyNote)).size(11));
            }
            Modal::About => {
                c = c
                    .push(text("NeonTrace 1.0.0").size(22))
                    .push(text("Copyright (c) 2026 Davoda1 · MIT"))
                    .push(text(self.t(K::Credits)).size(17))
                    .push(text(self.t(K::MapCredit)).size(13))
                    .push(text("https://simplemaps.com/resources/svg-world").size(12))
                    .push(text(self.t(K::GeoCredit)).size(13))
                    .push(
                        text("https://db-ip.com/ · https://creativecommons.org/licenses/by/4.0/")
                            .size(12),
                    )
                    .push(text(self.t(K::PrivacyNote)).size(12))
                    .push(text(include_str!("../assets/SIMPLEMAPS-LICENSE.txt")).size(11));
            }
            Modal::History => {
                c = c.push(button(self.t(K::ClearHistory)).on_press(Message::ClearHistory));
                if self.history_entries.is_empty() {
                    c = c.push(text(self.t(K::EmptyHistory)))
                }
                for h in &self.history_entries {
                    c = c.push(
                        column![
                            text(h.targets.join(", ")).size(13),
                            text(format!(
                                "{} · {}",
                                chrono::DateTime::from_timestamp_millis(h.saved_unix_ms as i64)
                                    .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                    .unwrap_or_default(),
                                self.t(if h.demonstration {
                                    K::DemoLabel
                                } else {
                                    K::SavedResult
                                })
                            ))
                            .size(11),
                            row![
                                button(self.t(K::Replay))
                                    .on_press(Message::LoadHistory(h.id.clone())),
                                button(self.t(K::DeleteRecord))
                                    .on_press(Message::DeleteHistory(h.id.clone()))
                            ]
                            .spacing(8)
                        ]
                        .spacing(6),
                    );
                }
            }
            Modal::Details => c = c.push(self.inspector()),
        }
        scrollable(container(c).padding(iced::Padding {
            right: 14.0,
            ..Default::default()
        }))
        .height(Length::Shrink)
        .into()
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.geo_cancel.store(true, Ordering::Release);
        if let Some(m) = &mut self.manager {
            m.stop()
        }
    }
}
fn button<'a>(content: impl Into<Element<'a, Message>>) -> iced::widget::Button<'a, Message> {
    iced::widget::button(content).style(|theme: &Theme, status| {
        let palette = theme.extended_palette();
        let mut style = button_style::secondary(theme, status);
        style.background = Some(
            match status {
                button_style::Status::Hovered | button_style::Status::Pressed => {
                    palette.background.strong.color
                }
                _ => palette.background.weak.color,
            }
            .into(),
        );
        style.text_color = palette.background.base.text;
        style.border = iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        };
        style
    })
}
fn location_point(ip: IpAddr, p: geo::GeoLocation) -> GeoPoint {
    GeoPoint {
        ip: Some(ip.to_string()),
        country: p.country,
        city: p.city,
        latitude: p.latitude,
        longitude: p.longitude,
        asn: p.asn,
        organization: p.organization,
        provider: p.provider,
        city_edition: p.city_edition,
        asn_edition: p.asn_edition,
        estimated_network_origin: false,
        stale: false,
    }
}
fn sample_status(s: &probe::ObservationStatus) -> SampleStatus {
    use probe::ObservationStatus as O;
    match s {
        O::Reply | O::TimeExceeded => SampleStatus::Reply,
        O::Timeout => SampleStatus::Timeout,
        O::Unreachable => SampleStatus::Unavailable,
        O::Cancelled => SampleStatus::Cancelled,
        O::Error => SampleStatus::Error,
    }
}
fn error_key(e: &probe::ErrorCode) -> K {
    match e {
        probe::ErrorCode::InvalidConfig | probe::ErrorCode::InvalidTarget => K::InvalidTarget,
        probe::ErrorCode::Dns => K::ResolutionFailed,
        probe::ErrorCode::PermissionDenied => K::PermissionDenied,
        probe::ErrorCode::Unsupported => K::TraceUnavailable,
        _ => K::NetworkError,
    }
}
// Explicit developer acceptance command; ordinary startup and demo never execute this.
fn verify_live_data() -> i32 {
    let directory = std::env::temp_dir().join(format!(
        "neontrace-data-verify-{}-{}",
        std::process::id(),
        export::now_ms()
    ));
    let result = (|| -> Result<(), String> {
        let runtime = tokio::runtime::Runtime::new().map_err(|_| "runtime".to_owned())?;
        let service = geo::GeoService::open(directory.clone()).map_err(|e| e.code.to_owned())?;
        runtime.block_on(async {
            let cancel = Arc::new(AtomicBool::new(false));
            let editions = service
                .refresh(true, cancel.clone(), None)
                .await
                .map_err(|e| e.code.to_owned())?;
            if editions.len() != 2 || !service.ready() {
                return Err("databases_not_ready".into());
            }
            for e in editions {
                println!("DB-IP {} {} validated", e.kind.key(), e.edition);
            }
            let origin = service
                .discover_origin(geo::AddressFamily::Any, cancel)
                .await
                .map_err(|e| e.code.to_owned())?;
            println!(
                "Origin discovery valid: true; local location available: {}",
                origin.location.coordinates().is_some()
            );
            println!("External address omitted from validation output.");
            Ok(())
        })
    })();
    if let Err(code) = &result {
        eprintln!("Live data validation: {code}")
    }
    let _ = std::fs::remove_dir_all(directory);
    if result.is_ok() { 0 } else { 1 }
}
// Connecting a UDP socket chooses a local route without transmitting a datagram.
fn network_signature() -> String {
    [
        ("0.0.0.0:0", "192.0.2.1:443"),
        ("[::]:0", "[2001:db8::1]:443"),
    ]
    .iter()
    .map(|(bind, remote)| {
        std::net::UdpSocket::bind(bind)
            .and_then(|s| {
                s.connect(remote)?;
                s.local_addr()
            })
            .map(|a| a.ip().to_string())
            .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("|")
}
fn ms(v: Option<f64>) -> String {
    v.filter(|v| v.is_finite())
        .map(|v| format!("{v:.1} ms"))
        .unwrap_or_else(|| "—".into())
}
fn location(p: &GeoPoint, unknown: &str) -> String {
    [p.city.clone(), p.country.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
        .trim()
        .to_owned()
        .pipe_or(unknown)
}
trait Nonempty {
    fn pipe_or(self, fallback: &str) -> String;
}
impl Nonempty for String {
    fn pipe_or(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.into()
        } else {
            self
        }
    }
}
fn save_png(path: PathBuf, s: window::Screenshot) -> Result<(), String> {
    storage::atomic_write(&path, |f| {
        let mut encoder = png::Encoder::new(f, s.size.width, s.size.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_itxt_chunk("Attribution".into(), export::DATA_CREDITS.into())
            .map_err(std::io::Error::other)?;
        encoder
            .add_itxt_chunk(
                "Map license".into(),
                include_str!("../assets/SIMPLEMAPS-LICENSE.txt").into(),
            )
            .map_err(std::io::Error::other)?;
        encoder
            .add_itxt_chunk("Software license".into(), include_str!("../LICENSE").into())
            .map_err(std::io::Error::other)?;
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&s.rgba)
            .map_err(std::io::Error::other)
    })
    .map_err(|e| e.to_string())
}
fn main() -> iced::Result {
    if std::env::args().any(|arg| arg == "--verify-data") {
        std::process::exit(verify_live_data());
    }
    let config = storage::Paths::discover()
        .and_then(|p| p.load_config())
        .unwrap_or_default();
    iced::application(App::new, App::update, App::view)
        .title("NeonTrace")
        .subscription(App::subscription)
        .theme(App::theme)
        .font(include_bytes!("../assets/DejaVuSans.ttf").as_slice())
        .default_font(iced::Font::with_name("DejaVu Sans"))
        .window(window::Settings {
            size: Size::new(config.window_width, config.window_height),
            min_size: Some(Size::new(680.0, 540.0)),
            icon: window::icon::from_rgba(
                include_bytes!("../assets/neontrace-64.rgba").to_vec(),
                64,
                64,
            )
            .ok(),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_timeline_and_continuous_segment_bars() {
        let a = Animation::new();
        let at = a.start + Duration::from_millis(2500);
        let p = a.smooth(at);
        assert!((p - 0.3125).abs() < 0.00001);
        assert!(((p * 3.0 - 0.0).clamp(0.0, 1.0) - 0.9375).abs() < 0.0001);
        assert_eq!(a.smooth(a.start + JOURNEY), 1.0);
        assert_eq!(a.smooth(a.start + Duration::from_secs(90)), 1.0);
    }
    #[test]
    fn stale_session_and_route_results_cannot_mutate_current_measurements() {
        let (mut app, _) = App::new();
        app.history.set_enabled(false);
        app.running = true;
        app.sessions = vec![Session {
            target_id: 1,
            generation: 2,
            ..Default::default()
        }];
        app.route_ids.insert(1, 2);
        let ping = probe::ProbePayload::Ping(probe::ProbeObservation {
            sequence: 1,
            timestamp_ms: 1,
            responder: None,
            rtt_ms: Some(99.0),
            status: probe::ObservationStatus::Reply,
            error: None,
        });
        let _ = app.event(probe::ProbeEvent {
            target_id: 1,
            generation: 1,
            timestamp_ms: 1,
            payload: ping,
        });
        assert!(app.sessions[0].pings.is_empty());
        let _ = app.event(probe::ProbeEvent {
            target_id: 1,
            generation: 2,
            timestamp_ms: 1,
            payload: probe::ProbePayload::TraceComplete {
                route_id: 1,
                reached: true,
                max_hops: 30,
            },
        });
        assert!(!app.sessions[0].trace_complete);
        let _ = app.event(probe::ProbeEvent {
            target_id: 1,
            generation: 2,
            timestamp_ms: 1,
            payload: probe::ProbePayload::TraceComplete {
                route_id: 2,
                reached: false,
                max_hops: 30,
            },
        });
        assert!(app.sessions[0].trace_complete);
    }
    #[test]
    fn theme_changes_preserve_animation_and_compact_details_are_accessible() {
        let (mut app, _) = App::new();
        app.sessions = demo::sessions(1);
        app.config.window_width = 680.0;
        app.replay();
        let started = app.animations[&1].start;
        let _ = app.update(Message::Theme(iced::theme::Mode::Light));
        assert!(!app.dark);
        assert_eq!(app.animations[&1].start, started);
        let _ = app.update(Message::Select(0));
        assert!(app.modal == Some(Modal::Details));
    }
    #[test]
    fn demo_mode_changes_restore_live_single_and_multi_state() {
        let (mut app, _) = App::new();
        app.history.set_enabled(false);
        app.sessions = vec![Session {
            target: "group.example".into(),
            ..Default::default()
        }];
        app.retained_single = vec![Session {
            target: "single.example".into(),
            ..Default::default()
        }];
        app.multi = true;
        let _ = app.update(Message::Demo);
        let _ = app.update(Message::ToggleMulti);
        let _ = app.update(Message::Demo);
        assert!(app.multi);
        assert_eq!(app.sessions[0].target, "group.example");
        assert_eq!(app.retained_single[0].target, "single.example");
        assert!(!app.running);
    }
    #[test]
    fn geo_batches_reject_stale_results_and_bound_current_address_cache() {
        let (mut app, _) = App::new();
        app.history.set_enabled(false);
        app.geo_epoch = 7;
        app.geo_lookup_revision = 2;
        app.geo_lookup_busy = true;
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        app.sessions = vec![Session {
            target_id: 1,
            chosen_ip: Some(ip.to_string()),
            ..Default::default()
        }];
        let location = geo::GeoLocation {
            city: Some("Fictional test location".into()),
            latitude: Some(47.5),
            longitude: Some(19.0),
            ..Default::default()
        };
        app.apply_geo_lookup(6, 2, vec![(ip, location.clone())]);
        assert!(app.geo_lookup_busy);
        assert!(app.location_cache.is_empty());
        app.apply_geo_lookup(7, 1, vec![(ip, location.clone())]);
        assert!(!app.geo_lookup_busy);
        assert!(app.location_cache.is_empty());
        assert!(app.pending_geo.contains(&ip));
        app.apply_geo_lookup(7, 2, vec![(ip, location.clone())]);
        assert_eq!(
            app.sessions[0].endpoint.as_ref().unwrap().latitude,
            Some(47.5)
        );
        assert!(app.sessions[0].origin.is_none());
        let mut batch = Vec::new();
        let mut responses = Vec::new();
        for n in 0..600u32 {
            let address = IpAddr::V4(std::net::Ipv4Addr::from(0xc612_0000u32 + n));
            batch.push((address, location.clone()));
            responses.push(HopResponse {
                address: address.to_string(),
                ..Default::default()
            });
        }
        app.sessions[0].hops = vec![Hop {
            responses,
            ..Default::default()
        }];
        app.apply_geo_lookup(7, 2, batch);
        assert!(app.location_cache.len() <= GEO_LOCATION_LIMIT);
        app.sessions[0].errors.push(Diagnostic {
            code: "network_changed".into(),
            ..Default::default()
        });
        app.apply_geo_lookup(7, 2, vec![(ip, geo::GeoLocation::default())]);
        assert_eq!(
            app.sessions[0].endpoint.as_ref().unwrap().latitude,
            Some(47.5)
        );
    }
    #[test]
    fn retracing_replaces_presentation_without_reusing_its_clock() {
        let (mut app, _) = App::new();
        app.history.set_enabled(false);
        app.running = true;
        app.sessions = vec![Session {
            target_id: 1,
            generation: 2,
            ..Default::default()
        }];
        app.route_ids.insert(1, 2);
        app.replay();
        assert!(app.animations.contains_key(&1));
        let _ = app.event(probe::ProbeEvent {
            target_id: 1,
            generation: 2,
            timestamp_ms: 1,
            payload: probe::ProbePayload::TraceStarted { route_id: 2 },
        });
        assert!(!app.animations.contains_key(&1));
    }
    #[test]
    fn new_route_snapshots_respect_the_global_pause_state() {
        let (mut app, _) = App::new();
        app.history.set_enabled(false);
        app.running = true;
        app.sessions = vec![Session {
            target_id: 1,
            generation: 2,
            ..Default::default()
        }];
        app.route_ids.insert(1, 1);
        let _ = app.update(Message::Pause);
        let _ = app.event(probe::ProbeEvent {
            target_id: 1,
            generation: 2,
            timestamp_ms: 1,
            payload: probe::ProbePayload::TraceComplete {
                route_id: 1,
                reached: true,
                max_hops: 30,
            },
        });
        assert_eq!(
            app.animations[&1].smooth(Instant::now() + Duration::from_secs(20)),
            0.0
        );
        let _ = app.update(Message::Pause);
        assert!(app.animations[&1].paused.is_none());
    }
    #[test]
    fn stopping_retains_a_history_write_failure_status() {
        let blocked = std::env::temp_dir().join(format!(
            "neontrace-history-stop-{}-{}",
            std::process::id(),
            export::now_ms()
        ));
        std::fs::write(&blocked, b"not a directory").unwrap();
        let (mut app, _) = App::new();
        app.history = storage::HistoryStore::new(blocked.clone(), true);
        app.running = true;
        app.sessions = vec![Session {
            target: "example.net".into(),
            ..Default::default()
        }];
        app.stop();
        assert!(!app.running);
        assert!(app.status == K::StorageError);
        std::fs::remove_file(blocked).unwrap();
    }
}
