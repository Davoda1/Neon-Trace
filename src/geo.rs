//! Local DB-IP geolocation. Network requests occur only in explicit live-use jobs.
//!
//! Data: IP Geolocation by DB-IP (https://db-ip.com), CC BY 4.0.
//! Source pages are inspected for their current links; no monthly URL is guessed.
use flate2::read::GzDecoder;
use maxminddb::{Mmap, Reader};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

pub const DATA_CREDIT: &str = "IP Geolocation by DB-IP · https://db-ip.com · CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/)";
const PAGE_LIMIT: usize = 2 * 1024 * 1024;
const MAX_COMPRESSED: u64 = 512 * 1024 * 1024;
const MAX_UNCOMPRESSED: u64 = 1024 * 1024 * 1024;
const SPACE_MARGIN: u64 = 16 * 1024 * 1024;
const RETRY_BACKOFF: u64 = 5 * 60;
static INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub type CancelFlag = Arc<AtomicBool>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetKind {
    City,
    Asn,
}
impl DatasetKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::City => "city",
            Self::Asn => "asn",
        }
    }
    fn page(self) -> &'static str {
        match self {
            Self::City => "https://db-ip.com/db/download/ip-to-city-lite",
            Self::Asn => "https://db-ip.com/db/download/ip-to-asn-lite",
        }
    }
    fn default_size(self) -> u64 {
        match self {
            Self::City => 200_000_000,
            Self::Asn => 40_000_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEdition {
    pub kind: DatasetKind,
    pub provider: String,
    pub edition: String,
    pub installed_unix: u64,
    pub build_epoch: u64,
    pub database_type: String,
    pub file: String,
    pub source: String,
    pub license: String,
}

/// All fields are optional: a country estimate never becomes an invented city.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeoLocation {
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub asn: Option<u32>,
    pub organization: Option<String>,
    pub city_edition: Option<String>,
    pub asn_edition: Option<String>,
    pub provider: Option<String>,
}
impl GeoLocation {
    pub fn coordinates(&self) -> Option<(f64, f64)> {
        Some((self.latitude?, self.longitude?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    V4,
    V6,
    Any,
}
#[derive(Debug, Clone)]
pub struct OriginEstimate {
    pub address: IpAddr,
    pub discovered_unix: u64,
    pub location: GeoLocation,
}

/// UI translates these stable status kinds and interpolates counts/errors.
#[derive(Debug, Clone)]
pub enum GeoEvent {
    Checking,
    Downloading {
        kind: DatasetKind,
        received: u64,
        total: Option<u64>,
    },
    Validating {
        kind: DatasetKind,
    },
    Installed {
        edition: DatasetEdition,
    },
    Ready {
        editions: Vec<DatasetEdition>,
    },
    Failed {
        code: &'static str,
        detail: String,
        usable_cache: bool,
    },
    Cancelled,
}
#[derive(Debug, Clone)]
pub struct GeoError {
    pub code: &'static str,
    pub detail: String,
}
impl std::fmt::Display for GeoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}
impl std::error::Error for GeoError {}
fn err(code: &'static str, detail: impl ToString) -> GeoError {
    GeoError {
        code,
        detail: detail.to_string(),
    }
}
fn ioerr(e: impl ToString) -> GeoError {
    err("geo_io", e)
}
fn cancelled(cancel: &AtomicBool) -> Result<(), GeoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(err("cancelled", "cancelled"))
    } else {
        Ok(())
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn month(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .map(|d| d.format("%Y-%m").to_string())
        .unwrap_or_default()
}
fn emit(tx: &Option<SyncSender<GeoEvent>>, event: GeoEvent) {
    // Progress must never block a download or accumulate an unbounded UI queue.
    if let Some(tx) = tx {
        let _ = tx.try_send(event);
    }
}

struct Database {
    reader: Reader<Mmap>,
    edition: DatasetEdition,
}
#[derive(Default)]
struct GeoCache {
    city: Option<Database>,
    asn: Option<Database>,
}
struct ServiceInner {
    directory: PathBuf,
    cache: RwLock<GeoCache>,
    refreshing: AtomicBool,
    last_attempt: AtomicU64,
    checked: AtomicU64,
}
#[derive(Clone)]
pub struct GeoService {
    inner: Arc<ServiceInner>,
}
impl std::fmt::Debug for GeoService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoService")
            .field("editions", &self.editions())
            .finish()
    }
}

impl GeoService {
    /// Does no networking. Call from a worker when cache metadata is large/slow.
    pub fn open(directory: PathBuf) -> Result<Self, GeoError> {
        fs::create_dir_all(&directory).map_err(ioerr)?;
        cleanup_abandoned_staging(&directory);
        let cache = GeoCache {
            city: load_cached(&directory, DatasetKind::City),
            asn: load_cached(&directory, DatasetKind::Asn),
        };
        let checked = fs::read_dir(&directory)
            .map_err(ioerr)?
            .filter_map(Result::ok)
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|s| s.strip_prefix("checked-"))
                    .and_then(|s| s.strip_suffix(".json"))
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0);
        Ok(Self {
            inner: Arc::new(ServiceInner {
                directory,
                cache: RwLock::new(cache),
                refreshing: AtomicBool::new(false),
                last_attempt: AtomicU64::new(0),
                checked: AtomicU64::new(checked),
            }),
        })
    }
    pub fn editions(&self) -> Vec<DatasetEdition> {
        let Ok(cache) = self.inner.cache.read() else {
            return vec![];
        };
        [cache.city.as_ref(), cache.asn.as_ref()]
            .into_iter()
            .flatten()
            .map(|d| d.edition.clone())
            .collect()
    }
    pub fn ready(&self) -> bool {
        self.inner
            .cache
            .read()
            .map(|c| c.city.is_some() && c.asn.is_some())
            .unwrap_or(false)
    }
    /// Intended to be checked while live measurement is active, never by a service.
    pub fn refresh_due(&self) -> bool {
        let t = now();
        let last = self.inner.last_attempt.load(Ordering::Relaxed);
        if last > 0 && t.saturating_sub(last) < RETRY_BACKOFF {
            return false;
        }
        !self.ready() || month(self.inner.checked.load(Ordering::Relaxed)) != month(t)
    }
    /// Synchronous, local-only. Use in a worker for batches of route observations.
    pub fn lookup(&self, address: IpAddr, locale: &str) -> GeoLocation {
        let Ok(cache) = self.inner.cache.read() else {
            return GeoLocation::default();
        };
        let mut location = GeoLocation::default();
        if !public_address(address) {
            return location;
        }
        if let Some(db) = &cache.city {
            if let Ok(record) = db.reader.lookup::<Value>(address) {
                location = city_record(&record, locale);
                location.city_edition = Some(db.edition.edition.clone());
                location.provider = Some("DB-IP Lite".into());
            }
        }
        if let Some(db) = &cache.asn {
            if let Ok(record) = db.reader.lookup::<Value>(address) {
                location.asn = record
                    .get("autonomous_system_number")
                    .or_else(|| record.pointer("/traits/autonomous_system_number"))
                    .or_else(|| record.get("asn"))
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok());
                location.organization = text_value(
                    record
                        .get("autonomous_system_organization")
                        .or_else(|| record.pointer("/traits/autonomous_system_organization"))
                        .or_else(|| record.get("as_organization")),
                );
                location.asn_edition = Some(db.edition.edition.clone());
                location.provider = Some("DB-IP Lite".into());
            }
        }
        location
    }
    /// Discover an egress address for the active family, without transmitting targets.
    /// A failed IPv6 request remains recoverable; no different-family position is substituted.
    pub async fn discover_origin(
        &self,
        family: AddressFamily,
        cancel: CancelFlag,
    ) -> Result<OriginEstimate, GeoError> {
        let endpoint = match family {
            AddressFamily::V4 => "https://api.ipify.org?format=json",
            AddressFamily::V6 => "https://api6.ipify.org?format=json",
            AddressFamily::Any => "https://api64.ipify.org?format=json",
        };
        let client = http_client()?;
        let mut last_error = err("origin_unavailable", "No public egress address available");
        for attempt in 0..2 {
            cancelled(&cancel)?;
            let request = async {
                let mut response = client
                    .get(endpoint)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                    .map_err(|_| err("origin_unavailable", "Public-IP discovery failed"))?
                    .error_for_status()
                    .map_err(|_| {
                        err(
                            "origin_unavailable",
                            "Public-IP discovery returned an error",
                        )
                    })?;
                let body = read_bounded(&mut response, 4096, &cancel).await?;
                let value: Value = serde_json::from_slice(&body)
                    .map_err(|_| err("origin_invalid", "Malformed public-IP response"))?;
                let address: IpAddr = value
                    .get("ip")
                    .and_then(Value::as_str)
                    .ok_or_else(|| err("origin_invalid", "Missing public address"))?
                    .parse()
                    .map_err(|_| err("origin_invalid", "Invalid public address"))?;
                if !public_address(address)
                    || matches!(
                        (family, address),
                        (AddressFamily::V4, IpAddr::V6(_)) | (AddressFamily::V6, IpAddr::V4(_))
                    )
                {
                    return Err(err(
                        "origin_invalid",
                        "Public address does not match the requested family",
                    ));
                }
                Ok(OriginEstimate {
                    address,
                    discovered_unix: now(),
                    location: self.lookup(address, "en"),
                })
            };
            match tokio::select! { result = request => result, _ = wait_cancel(cancel.clone()) => Err(err("cancelled", "cancelled")) }
            {
                Ok(origin) => return Ok(origin),
                Err(e) if e.code == "cancelled" => return Err(e),
                Err(e) => last_error = e,
            }
            if attempt == 0 {
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(2)) => {}, _ = wait_cancel(cancel.clone()) => return Err(err("cancelled", "cancelled")) }
            }
        }
        Err(last_error)
    }
    /// Run on Tokio, outside the GUI thread. At most one update per service is active.
    /// `force` is the explicit Retry/Refresh action; ordinary calls honor monthly checks/backoff.
    pub async fn refresh(
        &self,
        force: bool,
        cancel: CancelFlag,
        progress: Option<SyncSender<GeoEvent>>,
    ) -> Result<Vec<DatasetEdition>, GeoError> {
        if !force && !self.refresh_due() {
            return Ok(self.editions());
        }
        if self
            .inner
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return Ok(self.editions());
        }
        let _guard = RefreshGuard(self.inner.clone());
        self.inner.last_attempt.store(now(), Ordering::Relaxed);
        emit(&progress, GeoEvent::Checking);
        let result = self.refresh_inner(cancel.clone(), &progress).await;
        match &result {
            Ok(editions) => emit(
                &progress,
                GeoEvent::Ready {
                    editions: editions.clone(),
                },
            ),
            Err(e) if e.code == "cancelled" => emit(&progress, GeoEvent::Cancelled),
            Err(e) => emit(
                &progress,
                GeoEvent::Failed {
                    code: e.code,
                    detail: e.detail.clone(),
                    usable_cache: self.ready(),
                },
            ),
        }
        result
    }
    async fn refresh_inner(
        &self,
        cancel: CancelFlag,
        progress: &Option<SyncSender<GeoEvent>>,
    ) -> Result<Vec<DatasetEdition>, GeoError> {
        let client = http_client()?;
        for kind in [DatasetKind::City, DatasetKind::Asn] {
            cancelled(&cancel)?;
            let mut response = tokio::select! {
                result = client.get(kind.page()).timeout(Duration::from_secs(20)).send() => result.map_err(|_| err("geo_download_unavailable", "DB-IP download page unavailable"))?,
                _ = wait_cancel(cancel.clone()) => return Err(err("cancelled", "cancelled")),
            }.error_for_status().map_err(|_| err("geo_download_unavailable", "DB-IP download page returned an error"))?;
            let html = read_bounded(&mut response, PAGE_LIMIT, &cancel).await?;
            let html = std::str::from_utf8(&html)
                .map_err(|_| err("geo_invalid_page", "DB-IP page was not UTF-8"))?;
            let published = discover_download(html, kind)?;
            if self
                .editions()
                .iter()
                .any(|d| d.kind == kind && d.edition == published.edition)
            {
                continue;
            }
            let stage_id = format!(
                "{}-{}-{}-{}",
                kind.key(),
                now(),
                std::process::id(),
                INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            let compressed = self.inner.directory.join(format!(".stage-{stage_id}.gz"));
            let expanded = self.inner.directory.join(format!(".stage-{stage_id}.mmdb"));
            let _cleanup = StageGuard(vec![compressed.clone(), expanded.clone()]);
            download(&client, &published, &compressed, &cancel, progress).await?;
            cancelled(&cancel)?;
            emit(progress, GeoEvent::Validating { kind });
            let compressed_job = compressed.clone();
            let expanded_job = expanded.clone();
            let cancel_job = cancel.clone();
            let published_job = published.clone();
            let validated = tokio::task::spawn_blocking(move || {
                decompress(&compressed_job, &expanded_job, &cancel_job)?;
                cancelled(&cancel_job)?;
                let reader = open_validated(&expanded_job, kind)?;
                let metadata = &reader.metadata;
                let edition = DatasetEdition {
                    kind,
                    provider: "DB-IP Lite".into(),
                    edition: published_job.edition,
                    installed_unix: now(),
                    build_epoch: metadata.build_epoch,
                    database_type: metadata.database_type.clone(),
                    file: String::new(),
                    source: published_job.url.to_string(),
                    license: "CC BY 4.0".into(),
                };
                drop(reader); // Windows must release the mapping before moving the file.
                Ok::<_, GeoError>(edition)
            })
            .await
            .map_err(|_| err("geo_worker_failed", "Database validation worker failed"))??;
            cancelled(&cancel)?;
            let mut edition = validated;
            let name = format!(
                "{}-{}-{}-{}-{}.mmdb",
                kind.key(),
                edition.edition,
                edition.installed_unix,
                std::process::id(),
                INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            let installed = self.inner.directory.join(&name);
            fs::rename(&expanded, &installed).map_err(ioerr)?; // New unique path: atomic on both supported systems.
            edition.file = name;
            let db = match open_validated(&installed, kind) {
                Ok(reader) => Database {
                    reader,
                    edition: edition.clone(),
                },
                Err(e) => {
                    let _ = fs::remove_file(installed);
                    return Err(e);
                }
            };
            // Publish metadata last. An interrupted installation leaves the previous
            // generation intact and does not advertise an unfinished replacement.
            let metadata_path = installed.with_extension("json");
            if let Err(e) = write_new_json(&metadata_path, &edition) {
                drop(db);
                let _ = fs::remove_file(&installed);
                return Err(e);
            }
            {
                let mut cache = self
                    .inner
                    .cache
                    .write()
                    .map_err(|_| err("geo_lock", "Geolocation cache unavailable"))?;
                match kind {
                    DatasetKind::City => cache.city = Some(db),
                    DatasetKind::Asn => cache.asn = Some(db),
                }
            }
            prune_generations(&self.inner.directory, kind);
            emit(progress, GeoEvent::Installed { edition });
        }
        let checked = now();
        let check_path = self.inner.directory.join(format!("checked-{checked}.json"));
        if !check_path.exists() {
            write_new_json(
                &check_path,
                &serde_json::json!({"checked_unix": checked, "provider": "DB-IP"}),
            )?;
        }
        self.inner.checked.store(checked, Ordering::Relaxed);
        prune_checks(&self.inner.directory);
        Ok(self.editions())
    }
}
struct RefreshGuard(Arc<ServiceInner>);
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        self.0.refreshing.store(false, Ordering::Release);
    }
}
struct StageGuard(Vec<PathBuf>);
impl Drop for StageGuard {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
async fn wait_cancel(cancel: CancelFlag) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn http_client() -> Result<Client, GeoError> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(240))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or("");
            if attempt.previous().len() < 4
                && attempt.url().scheme() == "https"
                && matches!(
                    host,
                    "db-ip.com"
                        | "download.db-ip.com"
                        | "api.ipify.org"
                        | "api6.ipify.org"
                        | "api64.ipify.org"
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .user_agent(concat!("NeonTrace/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| err("geo_http", "HTTPS client could not be initialized"))
}
async fn read_bounded(
    response: &mut reqwest::Response,
    limit: usize,
    cancel: &CancelFlag,
) -> Result<Vec<u8>, GeoError> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(err("geo_size_limit", "Response exceeds its size limit"));
    }
    let mut bytes = Vec::new();
    loop {
        let next = tokio::select! { r = response.chunk() => r.map_err(|_| err("geo_read", "Network response interrupted"))?, _ = wait_cancel(cancel.clone()) => return Err(err("cancelled", "cancelled")) };
        let Some(chunk) = next else {
            break;
        };
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(err("geo_size_limit", "Response exceeds its size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Clone, Debug)]
struct PublishedDatabase {
    kind: DatasetKind,
    url: Url,
    edition: String,
    uncompressed_hint: Option<u64>,
}
/// Restrict discovered links to the publisher's HTTPS free-MMDB host and current kind.
fn discover_download(html: &str, kind: DatasetKind) -> Result<PublishedDatabase, GeoError> {
    let prefix = format!("/free/dbip-{}-lite-", kind.key());
    let mut remaining = html;
    while let Some(i) = remaining.find("href") {
        remaining = &remaining[i + 4..];
        let candidate = remaining.trim_start();
        let Some(candidate) = candidate.strip_prefix('=') else {
            continue;
        };
        let candidate = candidate.trim_start();
        let Some(quote @ ('\'' | '"')) = candidate.chars().next() else {
            continue;
        };
        let quoted = &candidate[1..];
        let Some(end) = quoted.find(quote) else {
            continue;
        };
        let value = quoted[..end].replace("&amp;", "&");
        let Ok(url) = Url::parse(&value) else {
            continue;
        };
        if url.scheme() != "https"
            || url.host_str() != Some("download.db-ip.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            continue;
        }
        let Some(edition) = url
            .path()
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".mmdb.gz"))
        else {
            continue;
        };
        if !valid_edition(edition) {
            continue;
        }
        let edition = edition.to_string();
        let before = html.split(&value).next().unwrap_or(html);
        return Ok(PublishedDatabase {
            kind,
            url,
            edition,
            uncompressed_hint: advertised_size(before),
        });
    }
    Err(err(
        "geo_link_changed",
        "No current official DB-IP Lite MMDB link was found",
    ))
}
fn valid_edition(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 7
        || bytes[4] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    s[5..].parse::<u8>().is_ok_and(|m| (1..=12).contains(&m))
}
fn advertised_size(html: &str) -> Option<u64> {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text.replace("&nbsp;", " ");
    let tail = text.rsplit("File size").next()?;
    let mut tokens = tail.split_whitespace();
    let amount: f64 = tokens.next()?.parse().ok()?;
    let scale = match tokens.next()? {
        "GB" | "GiB" => 1_073_741_824.0,
        "MB" | "MiB" => 1_048_576.0,
        "KB" | "KiB" => 1024.0,
        "B" => 1.0,
        _ => return None,
    };
    if amount.is_finite() && amount > 0.0 {
        Some((amount * scale).ceil() as u64)
    } else {
        None
    }
}
fn require_space(directory: &Path, required: u64) -> Result<(), GeoError> {
    let available = fs2::available_space(directory).map_err(ioerr)?;
    if available < required {
        return Err(err(
            "geo_disk_space",
            format!("{} bytes required; {} bytes available", required, available),
        ));
    }
    Ok(())
}
async fn download(
    client: &Client,
    published: &PublishedDatabase,
    destination: &Path,
    cancel: &CancelFlag,
    progress: &Option<SyncSender<GeoEvent>>,
) -> Result<(), GeoError> {
    let mut response = tokio::select! {
        r = client.get(published.url.clone()).send() => r.map_err(|_| err("geo_download_unavailable", "DB-IP database download unavailable"))?,
        _ = wait_cancel(cancel.clone()) => return Err(err("cancelled", "cancelled")),
    }.error_for_status().map_err(|_| err("geo_download_unavailable", "DB-IP database download returned an error"))?;
    let total = response.content_length();
    if total.is_some_and(|n| n > MAX_COMPRESSED) {
        return Err(err(
            "geo_size_limit",
            "Compressed database exceeds size limit",
        ));
    }
    let directory = destination
        .parent()
        .ok_or_else(|| err("geo_io", "Missing cache directory"))?;
    let estimated = published
        .uncompressed_hint
        .unwrap_or_else(|| published.kind.default_size());
    require_space(
        directory,
        total
            .unwrap_or(MAX_COMPRESSED)
            .saturating_add(estimated + estimated / 5)
            .saturating_add(SPACE_MARGIN),
    )?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await
        .map_err(ioerr)?;
    let mut received = 0u64;
    let mut next_report = 0u64;
    loop {
        let chunk = tokio::select! { r = response.chunk() => r.map_err(|_| err("geo_read", "DB-IP database download interrupted"))?, _ = wait_cancel(cancel.clone()) => return Err(err("cancelled", "cancelled")) };
        let Some(chunk) = chunk else {
            break;
        };
        received = received.saturating_add(chunk.len() as u64);
        if received > MAX_COMPRESSED {
            return Err(err(
                "geo_size_limit",
                "Compressed database exceeds size limit",
            ));
        }
        file.write_all(&chunk).await.map_err(ioerr)?;
        if received >= next_report {
            emit(
                progress,
                GeoEvent::Downloading {
                    kind: published.kind,
                    received,
                    total,
                },
            );
            next_report = received + 512 * 1024;
        }
    }
    if total.is_some_and(|n| n != received) || received == 0 {
        return Err(err("geo_read", "Incomplete DB-IP database download"));
    }
    file.sync_all().await.map_err(ioerr)?;
    emit(
        progress,
        GeoEvent::Downloading {
            kind: published.kind,
            received,
            total,
        },
    );
    Ok(())
}
fn decompress(compressed: &Path, destination: &Path, cancel: &AtomicBool) -> Result<(), GeoError> {
    let input = File::open(compressed).map_err(ioerr)?;
    let mut gzip = GzDecoder::new(input);
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(ioerr)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut expanded = 0u64;
    let mut next_space_check = 0u64;
    loop {
        cancelled(cancel)?;
        let n = gzip.read(&mut buffer).map_err(|_| {
            err(
                "geo_corrupt_archive",
                "Invalid or interrupted gzip database",
            )
        })?;
        if n == 0 {
            break;
        }
        expanded += n as u64;
        if expanded > MAX_UNCOMPRESSED {
            return Err(err(
                "geo_size_limit",
                "Expanded database exceeds size limit",
            ));
        }
        if expanded >= next_space_check {
            require_space(destination.parent().unwrap_or(Path::new(".")), SPACE_MARGIN)?;
            next_space_check = expanded + 16 * 1024 * 1024;
        }
        output.write_all(&buffer[..n]).map_err(ioerr)?;
    }
    if expanded == 0 {
        return Err(err("geo_corrupt_archive", "Empty database archive"));
    }
    output.sync_all().map_err(ioerr)?;
    Ok(())
}
fn open_validated(path: &Path, kind: DatasetKind) -> Result<Reader<Mmap>, GeoError> {
    let reader = Reader::open_mmap(path)
        .map_err(|_| err("geo_invalid_database", "Invalid MMDB structure"))?;
    let metadata = &reader.metadata;
    if metadata.binary_format_major_version != 2
        || !matches!(metadata.ip_version, 4 | 6)
        || metadata.node_count == 0
        || !metadata
            .database_type
            .to_ascii_lowercase()
            .contains(kind.key())
    {
        return Err(err(
            "geo_invalid_database",
            "MMDB is not the expected City/ASN edition",
        ));
    }
    let mut expected_fields = false;
    for ip in [
        IpAddr::from([8, 8, 8, 8]),
        IpAddr::from([1, 1, 1, 1]),
        "2001:4860:4860::8888".parse().unwrap(),
    ] {
        if let Ok(record) = reader.lookup::<Value>(ip) {
            expected_fields |= match kind {
                DatasetKind::City => {
                    record.get("country").is_some()
                        && record
                            .pointer("/location/latitude")
                            .and_then(Value::as_f64)
                            .is_some()
                        && record
                            .pointer("/location/longitude")
                            .and_then(Value::as_f64)
                            .is_some()
                }
                DatasetKind::Asn => record
                    .get("autonomous_system_number")
                    .or_else(|| record.pointer("/traits/autonomous_system_number"))
                    .or_else(|| record.get("asn"))
                    .and_then(Value::as_u64)
                    .is_some(),
            };
        }
    }
    if !expected_fields {
        return Err(err(
            "geo_invalid_database",
            "MMDB does not contain the expected record fields",
        ));
    }
    Ok(reader)
}
fn load_cached(directory: &Path, kind: DatasetKind) -> Option<Database> {
    let mut candidates: Vec<_> = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() > 64 * 1024 {
                return None;
            }
            let bytes = fs::read(path).ok()?;
            let edition: DatasetEdition = serde_json::from_slice(&bytes).ok()?;
            if edition.kind != kind
                || Path::new(&edition.file).components().count() != 1
                || !edition.file.ends_with(".mmdb")
            {
                return None;
            }
            Some(edition)
        })
        .collect();
    candidates.sort_by_key(|e| std::cmp::Reverse(e.installed_unix));
    for edition in candidates {
        if let Ok(reader) = open_validated(&directory.join(&edition.file), kind) {
            return Some(Database { reader, edition });
        }
    }
    None
}
fn write_new_json(path: &Path, value: &impl Serialize) -> Result<(), GeoError> {
    let staging = path.with_extension("json.pending");
    let _cleanup = StageGuard(vec![staging.clone()]);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staging)
        .map_err(ioerr)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(ioerr)?;
    file.sync_all().map_err(ioerr)?;
    drop(file);
    fs::rename(staging, path).map_err(ioerr)
}
fn prune_generations(directory: &Path, kind: DatasetKind) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut generations: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path();
            if p.extension()?.to_str()? != "json" {
                return None;
            }
            let metadata = fs::symlink_metadata(&p).ok()?;
            if !metadata.is_file() || metadata.len() > 64 * 1024 {
                return None;
            }
            let edition: DatasetEdition = serde_json::from_slice(&fs::read(&p).ok()?).ok()?;
            if edition.kind == kind && Path::new(&edition.file).components().count() == 1 {
                Some((edition.installed_unix, p, directory.join(edition.file)))
            } else {
                None
            }
        })
        .collect();
    generations.sort_by_key(|e| std::cmp::Reverse(e.0));
    // Keep the last good generation as recovery, with a bounded cache.
    for (_, metadata, db) in generations.into_iter().skip(2) {
        let _ = fs::remove_file(db);
        let _ = fs::remove_file(metadata);
    }
}
fn cleanup_abandoned_staging(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let abandoned_candidate = name.starts_with(".stage-")
            || name.ends_with(".json.pending")
            || (name.ends_with(".mmdb")
                && (name.starts_with("city-") || name.starts_with("asn-"))
                && !path.with_extension("json").exists());
        if !abandoned_candidate {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let old = metadata
            .modified()
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 60 * 60));
        if old {
            let _ = fs::remove_file(path);
        }
    }
}
fn prune_checks(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut checks: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|s| s.starts_with("checked-") && s.ends_with(".json"))
        })
        .map(|e| e.path())
        .collect();
    checks.sort();
    let remove = checks.len().saturating_sub(2);
    for p in checks.into_iter().take(remove) {
        let _ = fs::remove_file(p);
    }
}
fn text_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
fn city_record(value: &Value, locale: &str) -> GeoLocation {
    let localized = |section: &str| {
        text_value(value.pointer(&format!("/{section}/names/{locale}")))
            .or_else(|| text_value(value.pointer(&format!("/{section}/names/en"))))
    };
    let latitude = value
        .pointer("/location/latitude")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v));
    let longitude = value
        .pointer("/location/longitude")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (-180.0..=180.0).contains(v));
    // Coordinates without a city are coarse and are deliberately left unlocated.
    let city = localized("city");
    let has_city = city.is_some();
    GeoLocation {
        country_code: text_value(value.pointer("/country/iso_code")),
        country: localized("country"),
        city,
        latitude: if has_city && longitude.is_some() {
            latitude
        } else {
            None
        },
        longitude: if has_city && latitude.is_some() {
            longitude
        } else {
            None
        },
        ..Default::default()
    }
}
/// Exclude local/private/documentation/multicast addresses from public geolocation.
pub fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(a) => {
            let [a, b, c, _] = a.octets();
            !matches!(a, 0 | 10 | 127 | 224..=255)
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2))))
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            !a.is_loopback()
                && !a.is_unspecified()
                && (s[0] & 0xfe00) != 0xfc00
                && (s[0] & 0xffc0) != 0xfe80
                && (s[0] & 0xff00) != 0xff00
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && a.to_ipv4_mapped()
                    .map(|v4| public_address(IpAddr::V4(v4)))
                    .unwrap_or(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_link_discovery_rejects_wrong_hosts_kinds_and_invalid_editions() {
        let html = r#"<a href='https://evil.example/free/dbip-city-lite-2026-10.mmdb.gz'>bad</a><a href='https://download.db-ip.com/free/dbip-asn-lite-2026-10.mmdb.gz'>wrong kind</a><a href='https://download.db-ip.com/free/dbip-city-lite-2026-13.mmdb.gz'>bad month</a><div>File size <span>121.1 MB</span></div><a href='https://download.db-ip.com/free/dbip-city-lite-2026-10.mmdb.gz'>Download</a>"#;
        let result = discover_download(html, DatasetKind::City).unwrap();
        assert_eq!(result.edition, "2026-10");
        assert_eq!(result.url.host_str(), Some("download.db-ip.com"));
        assert!(result.uncompressed_hint.unwrap() > 121_000_000);
        assert!(discover_download("<a href='http://download.db-ip.com/free/dbip-city-lite-2026-10.mmdb.gz'>unsafe</a>", DatasetKind::City).is_err());
    }
    #[test]
    fn country_only_or_invalid_coordinates_remain_unlocated() {
        let value = serde_json::json!({"country":{"iso_code":"HU","names":{"en":"Hungary"}},"location":{"latitude":47.5,"longitude":19.0}});
        let loc = city_record(&value, "en");
        assert_eq!(loc.country_code.as_deref(), Some("HU"));
        assert!(loc.coordinates().is_none());
        let value = serde_json::json!({"city":{"names":{"en":"Example"}},"location":{"latitude":91.0,"longitude":19.0}});
        assert!(city_record(&value, "en").coordinates().is_none());
    }
    #[test]
    fn localized_fields_fallback_without_fabricating_values() {
        let value = serde_json::json!({"city":{"names":{"en":"Example City","ru":"Пример"}},"location":{"latitude":47.5,"longitude":19.0}});
        assert_eq!(city_record(&value, "ru").city.as_deref(), Some("Пример"));
        assert_eq!(
            city_record(&value, "hu").city.as_deref(),
            Some("Example City")
        );
        assert_eq!(city_record(&value, "de").coordinates(), Some((47.5, 19.0)));
        assert!(city_record(&value, "en").asn.is_none());
    }
    #[test]
    fn local_and_documentation_addresses_are_never_geolocated() {
        for a in [
            "127.0.0.1",
            "10.2.3.4",
            "100.64.1.1",
            "192.168.1.1",
            "192.0.2.1",
            "198.51.100.2",
            "203.0.113.5",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "ff02::1",
            "::ffff:192.168.1.2",
        ] {
            assert!(!public_address(a.parse().unwrap()), "{a}");
        }
        for a in ["8.8.8.8", "1.1.1.1", "2001:4860:4860::8888"] {
            assert!(public_address(a.parse().unwrap()), "{a}");
        }
    }
    #[test]
    fn bad_or_cancelled_archive_preserves_existing_cache() {
        let dir = std::env::temp_dir().join(format!(
            "neontrace-geo-test-{}-{}",
            std::process::id(),
            INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("last-good.mmdb");
        fs::write(&old, b"untouched").unwrap();
        let bad = dir.join("bad.gz");
        fs::write(&bad, b"not gzip").unwrap();
        let output = dir.join("staging.mmdb");
        assert_eq!(
            decompress(&bad, &output, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            "geo_corrupt_archive"
        );
        assert_eq!(fs::read(&old).unwrap(), b"untouched");
        let _ = fs::remove_file(&output);
        assert_eq!(
            decompress(&bad, &output, &AtomicBool::new(true))
                .unwrap_err()
                .code,
            "cancelled"
        );
        assert_eq!(fs::read(&old).unwrap(), b"untouched");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn no_network_on_open_and_monthly_backoff_is_bounded() {
        let dir = std::env::temp_dir().join(format!(
            "neontrace-geo-open-{}-{}",
            std::process::id(),
            INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let service = GeoService::open(dir.clone()).unwrap();
        assert!(!service.ready());
        assert!(service.refresh_due());
        service.inner.last_attempt.store(now(), Ordering::Relaxed);
        assert!(!service.refresh_due());
        assert!(
            service
                .lookup("192.0.2.1".parse().unwrap(), "en")
                .coordinates()
                .is_none()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
