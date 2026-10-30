//! Stable application-owned export schema. Measurements never contain animation values.
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const DATA_CREDITS: &str = "IP Geolocation by DB-IP — https://db-ip.com/ — CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Lite estimates have reduced coverage and accuracy. Map: Simplemaps, MIT.";
pub const APPROXIMATION_NOTE: &str = "Observed probe route · approximate locations. Curves are schematic, not physical cables. Hop RTT is measured from this device, not link delay. Continuous endpoint ping does not prove the last traced route is unchanged. Endpoint samples and statistics retain the latest 3600 probes per target; archived route snapshots retain the latest 16.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportBundle {
    pub schema_version: u32,
    pub app_version: String,
    pub exported_unix_ms: u64,
    pub demonstration: bool,
    pub notes: String,
    pub data_credits: String,
    pub sessions: Vec<Session>,
}
impl Default for ExportBundle {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            exported_unix_ms: now_ms(),
            demonstration: false,
            notes: APPROXIMATION_NOTE.into(),
            data_credits: DATA_CREDITS.into(),
            sessions: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub protocol: ProbeProtocol,
    pub target_id: u64,
    pub generation: u64,
    pub target: String,
    pub chosen_ip: Option<String>,
    pub resolved_candidates: Vec<String>,
    pub address_family: Option<String>,
    pub started_unix_ms: u64,
    pub ended_unix_ms: Option<u64>,
    pub route_timestamp_unix_ms: Option<u64>,
    pub trace_complete: bool,
    pub destination_reached: bool,
    pub demonstration: bool,
    pub origin: Option<GeoPoint>,
    pub endpoint: Option<GeoPoint>,
    pub hops: Vec<Hop>,
    pub route_snapshots: Vec<RouteSnapshot>,
    pub pings: Vec<PingSample>,
    pub tcp_checks: Vec<TcpSample>,
    pub errors: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeProtocol {
    #[default]
    Icmp,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeoPoint {
    pub ip: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub asn: Option<u32>,
    pub organization: Option<String>,
    pub provider: Option<String>,
    pub city_edition: Option<String>,
    pub asn_edition: Option<String>,
    pub estimated_network_origin: bool,
    pub stale: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteSnapshot {
    pub timestamp_unix_ms: u64,
    pub complete: bool,
    pub reached: bool,
    pub hops: Vec<Hop>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Hop {
    pub ttl: u8,
    pub observed_unix_ms: u64,
    /// Multiple responding interfaces at the same TTL are kept independently.
    pub responses: Vec<HopResponse>,
    pub nonresponses: u32,
    pub status: SampleStatus,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HopResponse {
    pub address: String,
    pub rtt_ms: Option<f64>,
    pub observed_unix_ms: u64,
    pub destination: bool,
    pub location: Option<GeoPoint>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleStatus {
    Reply,
    Timeout,
    Unavailable,
    Cancelled,
    #[default]
    Unknown,
    Error,
}
impl SampleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
            Self::Error => "error",
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PingSample {
    pub sequence: u64,
    pub observed_unix_ms: u64,
    pub status: SampleStatus,
    pub rtt_ms: Option<f64>,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TcpSample {
    pub observed_unix_ms: u64,
    pub port: u16,
    pub status: SampleStatus,
    pub connection_ms: Option<f64>,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Diagnostic {
    pub observed_unix_ms: u64,
    pub code: String,
    pub detail: String,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PingStatistics {
    pub sent: usize,
    pub replies: usize,
    pub nonresponses: usize,
    pub minimum_ms: Option<f64>,
    pub average_ms: Option<f64>,
    pub maximum_ms: Option<f64>,
    /// Mean absolute difference of consecutive successful RTTs. A missing sample breaks adjacency.
    pub jitter_ms: Option<f64>,
}
impl Session {
    pub fn statistics(&self) -> PingStatistics {
        let mut result = PingStatistics {
            sent: self.pings.len(),
            ..Default::default()
        };
        let mut sum = 0.0;
        let mut previous: Option<f64> = None;
        let mut differences = 0usize;
        let mut difference_sum = 0.0;
        for sample in &self.pings {
            let good = sample.rtt_ms.filter(|rtt| {
                sample.status == SampleStatus::Reply && rtt.is_finite() && *rtt >= 0.0
            });
            if let Some(rtt) = good {
                result.replies += 1;
                result.minimum_ms = Some(result.minimum_ms.map_or(rtt, |old| old.min(rtt)));
                result.maximum_ms = Some(result.maximum_ms.map_or(rtt, |old| old.max(rtt)));
                sum += rtt;
                if let Some(last) = previous {
                    difference_sum += (rtt - last).abs();
                    differences += 1;
                }
                previous = Some(rtt);
            } else {
                previous = None;
            }
        }
        result.nonresponses = result.sent - result.replies;
        if result.replies > 0 {
            result.average_ms = Some(sum / result.replies as f64);
        }
        if differences > 0 {
            result.jitter_ms = Some(difference_sum / differences as f64);
        }
        result
    }
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn write_json(path: &Path, bundle: &ExportBundle) -> io::Result<()> {
    crate::storage::atomic_write(path, |file| {
        serde_json::to_writer_pretty(&mut *file, bundle).map_err(io::Error::other)?;
        file.write_all(b"\n")
    })
}

/// One lossless long-form measurement table, with a row for every target, hop response,
/// silent hop, endpoint sample and TCP check. JSON retains richer nested metadata.
pub fn write_csv(path: &Path, bundle: &ExportBundle) -> io::Result<()> {
    crate::storage::atomic_write(path, |file| write_csv_to(file, bundle))
}
fn csv_row(file: &mut impl Write, cells: &[String]) -> io::Result<()> {
    for (index, cell) in cells.iter().enumerate() {
        if index != 0 {
            file.write_all(b",")?;
        }
        // Quoted cells also prevent newlines/commas from breaking row boundaries.
        file.write_all(b"\"")?;
        file.write_all(cell.replace('"', "\"\"").as_bytes())?;
        file.write_all(b"\"")?;
    }
    file.write_all(b"\r\n")
}
fn opt<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}
fn write_csv_to(file: &mut File, bundle: &ExportBundle) -> io::Result<()> {
    let names = [
        "schema_version",
        "exported_unix_ms",
        "demonstration",
        "row_kind",
        "target_id",
        "generation",
        "target",
        "chosen_ip",
        "address_family",
        "route_timestamp_unix_ms",
        "trace_complete",
        "destination_reached",
        "sequence_or_ttl",
        "observed_unix_ms",
        "address",
        "status",
        "rtt_or_connection_ms",
        "port",
        "country",
        "city",
        "latitude",
        "longitude",
        "asn",
        "organization",
        "provider",
        "city_edition",
        "asn_edition",
        "unknown_location",
        "detail",
        "data_credits",
        "approximation_note",
        "route_snapshot",
        "probe_protocol",
    ];
    csv_row(file, &names.map(str::to_owned))?;
    for session in &bundle.sessions {
        let base = |kind: &str| -> Vec<String> {
            let mut row = vec![String::new(); names.len()];
            row[0] = bundle.schema_version.to_string();
            row[1] = bundle.exported_unix_ms.to_string();
            row[2] = (bundle.demonstration || session.demonstration).to_string();
            row[3] = kind.into();
            row[4] = session.target_id.to_string();
            row[5] = session.generation.to_string();
            row[6] = session.target.clone();
            row[7] = session.chosen_ip.clone().unwrap_or_default();
            row[8] = session.address_family.clone().unwrap_or_default();
            row[9] = opt(session.route_timestamp_unix_ms);
            row[10] = session.trace_complete.to_string();
            row[11] = session.destination_reached.to_string();
            row[29] = bundle.data_credits.clone();
            row[30] = bundle.notes.clone();
            row[31] = "current".into();
            row[32] = "icmp".into();
            row
        };
        let location = |row: &mut Vec<String>, point: Option<&GeoPoint>| {
            row[27] = point
                .is_none_or(|p| p.latitude.is_none() || p.longitude.is_none())
                .to_string();
            if let Some(p) = point {
                row[18] = p.country.clone().unwrap_or_default();
                row[19] = p.city.clone().unwrap_or_default();
                row[20] = opt(p.latitude);
                row[21] = opt(p.longitude);
                row[22] = opt(p.asn);
                row[23] = p.organization.clone().unwrap_or_default();
                row[24] = p.provider.clone().unwrap_or_default();
                row[25] = p.city_edition.clone().unwrap_or_default();
                row[26] = p.asn_edition.clone().unwrap_or_default();
            }
        };
        let mut row = base("target");
        row[13] = session.started_unix_ms.to_string();
        location(&mut row, session.endpoint.as_ref());
        csv_row(file, &row)?;
        if let Some(origin) = &session.origin {
            let mut row = base("origin");
            row[14] = origin.ip.clone().unwrap_or_default();
            location(&mut row, Some(origin));
            csv_row(file, &row)?;
        }
        let snapshots =
            std::iter::once((
                session.hops.as_slice(),
                session.route_timestamp_unix_ms,
                session.trace_complete,
                session.destination_reached,
                "current".to_owned(),
            ))
            .chain(session.route_snapshots.iter().enumerate().map(
                |(index, snapshot)| {
                    (
                        snapshot.hops.as_slice(),
                        Some(snapshot.timestamp_unix_ms),
                        snapshot.complete,
                        snapshot.reached,
                        index.to_string(),
                    )
                },
            ));
        for (hops, timestamp, complete, reached, identity) in snapshots {
            let stamp = |row: &mut Vec<String>| {
                row[9] = opt(timestamp);
                row[10] = complete.to_string();
                row[11] = reached.to_string();
                row[31] = identity.clone();
            };
            if identity != "current" {
                let mut row = base("route_snapshot");
                stamp(&mut row);
                csv_row(file, &row)?;
            }
            for hop in hops {
                if hop.responses.is_empty() {
                    let mut row = base("hop");
                    stamp(&mut row);
                    row[12] = hop.ttl.to_string();
                    row[13] = hop.observed_unix_ms.to_string();
                    row[15] = hop.status.as_str().into();
                    row[27] = "true".into();
                    row[28] = format!("nonresponses={}", hop.nonresponses);
                    csv_row(file, &row)?;
                }
                for response in &hop.responses {
                    let mut row = base("hop");
                    stamp(&mut row);
                    row[12] = hop.ttl.to_string();
                    row[13] = response.observed_unix_ms.to_string();
                    row[14] = response.address.clone();
                    row[15] = "reply".into();
                    row[16] = opt(response.rtt_ms);
                    row[28] = format!(
                        "destination={}; nonresponses={}",
                        response.destination, hop.nonresponses
                    );
                    location(&mut row, response.location.as_ref());
                    csv_row(file, &row)?;
                }
            }
        }
        for ping in &session.pings {
            let mut row = base("ping");
            row[12] = ping.sequence.to_string();
            row[13] = ping.observed_unix_ms.to_string();
            row[15] = ping.status.as_str().into();
            row[16] = opt(ping.rtt_ms);
            row[28] = ping.detail.clone().unwrap_or_default();
            csv_row(file, &row)?;
        }
        for tcp in &session.tcp_checks {
            let mut row = base("tcp");
            row[13] = tcp.observed_unix_ms.to_string();
            row[15] = tcp.status.as_str().into();
            row[16] = opt(tcp.connection_ms);
            row[17] = tcp.port.to_string();
            row[28] = tcp.detail.clone().unwrap_or_default();
            csv_row(file, &row)?;
        }
        for diagnostic in &session.errors {
            let mut row = base("diagnostic");
            row[13] = diagnostic.observed_unix_ms.to_string();
            row[15] = diagnostic.code.clone();
            row[28] = diagnostic.detail.clone();
            csv_row(file, &row)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_invalid_samples_do_not_enter_averages() {
        let session = Session {
            pings: vec![
                PingSample {
                    status: SampleStatus::Reply,
                    rtt_ms: Some(10.0),
                    ..Default::default()
                },
                PingSample {
                    status: SampleStatus::Reply,
                    rtt_ms: Some(14.0),
                    ..Default::default()
                },
                PingSample {
                    status: SampleStatus::Timeout,
                    rtt_ms: Some(999.0),
                    ..Default::default()
                },
                PingSample {
                    status: SampleStatus::Reply,
                    rtt_ms: Some(20.0),
                    ..Default::default()
                },
                PingSample {
                    status: SampleStatus::Reply,
                    rtt_ms: Some(f64::NAN),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let stats = session.statistics();
        assert_eq!(stats.replies, 3);
        assert_eq!(stats.nonresponses, 2);
        assert_eq!(stats.average_ms, Some(44.0 / 3.0));
        assert_eq!(stats.jitter_ms, Some(4.0));
    }
    #[test]
    fn csv_quotes_fields_and_retains_silent_hops_and_alternates() {
        let root = std::env::temp_dir().join(format!(
            "neontrace-csv-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let bundle = ExportBundle {
            sessions: vec![
                Session {
                    target_id: 1,
                    target: "example,\"test\"".into(),
                    hops: vec![
                        Hop {
                            ttl: 1,
                            status: SampleStatus::Timeout,
                            ..Default::default()
                        },
                        Hop {
                            ttl: 2,
                            responses: vec![
                                HopResponse {
                                    address: "192.0.2.1".into(),
                                    ..Default::default()
                                },
                                HopResponse {
                                    address: "192.0.2.2".into(),
                                    ..Default::default()
                                },
                            ],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                Session {
                    target_id: 2,
                    target: "example.org".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        write_csv(&root.join("test.csv"), &bundle).unwrap();
        let text = std::fs::read_to_string(root.join("test.csv")).unwrap();
        assert!(text.contains("\"example,\"\"test\"\"\""));
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("192.0.2.1"));
        assert!(text.contains("192.0.2.2"));
        assert!(text.contains("\"timeout\""));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_routes_remain_separate_in_csv_and_json() {
        let root = std::env::temp_dir().join(format!(
            "neontrace-route-export-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let bundle = ExportBundle {
            sessions: vec![Session {
                route_timestamp_unix_ms: Some(2000),
                hops: vec![Hop {
                    ttl: 1,
                    responses: vec![HopResponse {
                        address: "192.0.2.2".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                route_snapshots: vec![RouteSnapshot {
                    timestamp_unix_ms: 1000,
                    complete: true,
                    reached: false,
                    hops: vec![Hop {
                        ttl: 1,
                        responses: vec![HopResponse {
                            address: "192.0.2.1".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        write_csv(&root.join("routes.csv"), &bundle).unwrap();
        let text = std::fs::read_to_string(root.join("routes.csv")).unwrap();
        let current = text
            .lines()
            .find(|line| line.contains("192.0.2.2"))
            .unwrap();
        let archived = text
            .lines()
            .find(|line| line.contains("192.0.2.1"))
            .unwrap();
        assert!(current.contains("\"2000\""));
        assert!(current.ends_with("\"current\",\"icmp\""));
        assert!(archived.contains("\"1000\""));
        assert!(archived.ends_with("\"0\",\"icmp\""));
        let restored: ExportBundle =
            serde_json::from_str(&serde_json::to_string(&bundle).unwrap()).unwrap();
        assert_eq!(
            restored.sessions[0].route_snapshots[0].hops[0].responses[0].address,
            "192.0.2.1"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn json_roundtrip_keeps_unknown_distinct_from_silence() {
        let original = ExportBundle {
            sessions: vec![Session {
                hops: vec![
                    Hop {
                        ttl: 1,
                        status: SampleStatus::Timeout,
                        ..Default::default()
                    },
                    Hop {
                        ttl: 2,
                        responses: vec![HopResponse {
                            address: "192.0.2.1".into(),
                            location: None,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let restored: ExportBundle =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(restored.schema_version, 1);
        assert!(restored.sessions[0].hops[0].responses.is_empty());
        assert_eq!(restored.sessions[0].hops[1].responses.len(), 1);
        assert!(restored.sessions[0].hops[1].responses[0].location.is_none());
    }
}
