//! Embedded offline observations. These values are illustrative and never sent
//! to a resolver, socket, discovery service or geolocation provider.
use crate::export::{
    self, Diagnostic, GeoPoint, Hop, HopResponse, PingSample, SampleStatus, Session, TcpSample,
};

const FICTIONAL: &str = "Fictional";
const SAMPLE_RTT: [Option<f64>; 24] = [
    Some(110.8),
    Some(112.1),
    Some(111.5),
    Some(113.4),
    Some(112.0),
    Some(109.9),
    Some(110.3),
    Some(112.6),
    Some(114.1),
    None,
    Some(112.7),
    Some(111.8),
    Some(112.3),
    Some(115.1),
    Some(113.2),
    Some(111.1),
    Some(110.6),
    Some(112.8),
    Some(113.7),
    Some(112.2),
    Some(111.9),
    Some(113.0),
    Some(112.6),
    Some(112.4),
];

#[derive(Clone, Copy)]
struct Place {
    city: &'static str,
    country: &'static str,
    latitude: f64,
    longitude: f64,
}
const BUDAPEST: Place = Place {
    city: "Budapest",
    country: "HU",
    latitude: 47.4979,
    longitude: 19.0402,
};
const FRANKFURT: Place = Place {
    city: "Frankfurt",
    country: "DE",
    latitude: 50.1109,
    longitude: 8.6821,
};
const NEW_YORK: Place = Place {
    city: "New York",
    country: "US",
    latitude: 40.7128,
    longitude: -74.0060,
};
const DESTINATIONS: [Place; 10] = [
    Place {
        city: "Toronto",
        country: "CA",
        latitude: 43.6532,
        longitude: -79.3832,
    },
    Place {
        city: "London",
        country: "GB",
        latitude: 51.5074,
        longitude: -0.1278,
    },
    Place {
        city: "Reykjavík",
        country: "IS",
        latitude: 64.1466,
        longitude: -21.9426,
    },
    Place {
        city: "Tokyo",
        country: "JP",
        latitude: 35.6762,
        longitude: 139.6503,
    },
    Place {
        city: "Singapore",
        country: "SG",
        latitude: 1.3521,
        longitude: 103.8198,
    },
    Place {
        city: "Sydney",
        country: "AU",
        latitude: -33.8688,
        longitude: 151.2093,
    },
    Place {
        city: "Cape Town",
        country: "ZA",
        latitude: -33.9249,
        longitude: 18.4241,
    },
    Place {
        city: "São Paulo",
        country: "BR",
        latitude: -23.5505,
        longitude: -46.6333,
    },
    Place {
        city: "San Francisco",
        country: "US",
        latitude: 37.7749,
        longitude: -122.4194,
    },
    Place {
        city: "Auckland",
        country: "NZ",
        latitude: -36.8509,
        longitude: 174.7645,
    },
];

fn point(place: Place, address: &str, asn: u32, origin: bool) -> GeoPoint {
    GeoPoint {
        ip: Some(address.into()),
        country: Some(place.country.into()),
        city: Some(place.city.into()),
        latitude: Some(place.latitude),
        longitude: Some(place.longitude),
        // RFC 5398 documentation ASN range. These are not actual providers.
        asn: Some(asn),
        organization: Some("Fictional demonstration network".into()),
        provider: Some(FICTIONAL.into()),
        city_edition: Some("demonstration".into()),
        asn_edition: Some("demonstration".into()),
        estimated_network_origin: origin,
        stale: false,
    }
}

fn responding_hop(
    ttl: u8,
    address: String,
    rtt: f64,
    place: Option<Place>,
    destination: bool,
    at: u64,
) -> Hop {
    let location = place.map(|place| point(place, &address, 64496 + u32::from(ttl), false));
    Hop {
        ttl,
        observed_unix_ms: at,
        nonresponses: 0,
        status: SampleStatus::Reply,
        responses: vec![HopResponse {
            address,
            rtt_ms: Some(rtt),
            observed_unix_ms: at,
            destination,
            location,
        }],
    }
}

/// Produce 1–10 independent, explicitly fictional sessions. The default route
/// is the approved Budapest → Frankfurt → New York → Toronto example. A silent
/// hop and a responding interface without a location are deliberately distinct.
/// Out-of-range caller values are clamped to the supported presentation count.
/// This function performs no network operations and requires no database files.
pub fn sessions(count: usize) -> Vec<Session> {
    let started = export::now_ms().saturating_sub(SAMPLE_RTT.len() as u64 * 1_000);
    (0..count.clamp(1, 10)).map(|index| {
        let destination = DESTINATIONS[index];
        let endpoint_address = format!("203.0.113.{}", index + 10);
        let endpoint = point(destination, &endpoint_address, 64510, false);
        let origin = point(BUDAPEST, "192.0.2.1", 64496, true);
        // Preserve the approved transatlantic default; the other demo routes
        // use a shared Frankfurt stage and their own clearly fictional endpoint.
        let transit = if index == 0 || index == 8 { NEW_YORK } else { FRANKFURT };
        let offset = index as f64 * 3.0;
        let mut hops = vec![
            responding_hop(1, "192.0.2.2".into(), 1.2, Some(BUDAPEST), false, started + 100),
            Hop { ttl: 2, observed_unix_ms: started + 1_200, responses: vec![], nonresponses: 1, status: SampleStatus::Timeout },
            responding_hop(3, "198.51.100.1".into(), 18.9, Some(FRANKFURT), false, started + 1_400),
            responding_hop(4, "198.51.100.2".into(), 27.3, None, false, started + 1_600),
            responding_hop(5, "198.51.100.3".into(), 96.1 + offset, Some(transit), false, started + 1_800),
            responding_hop(6, endpoint_address.clone(), 112.4 + offset, Some(destination), true, started + 2_000),
        ];
        // An alternate interface is retained as an independent observation,
        // not stitched into another claimed route.
        if index == 2 {
            hops[2].responses.push(HopResponse {
                address: "198.51.100.4".into(), rtt_ms: Some(19.7), observed_unix_ms: started + 1_410,
                destination: false, location: Some(point(FRANKFURT, "198.51.100.4", 64500, false)),
            });
        }
        let pings = SAMPLE_RTT.iter().enumerate().map(|(sequence, &rtt)| PingSample {
            sequence: sequence as u64 + 1, observed_unix_ms: started + sequence as u64 * 1_000,
            status: if rtt.is_some() { SampleStatus::Reply } else { SampleStatus::Timeout },
            rtt_ms: rtt.map(|value| value + offset), detail: Some("Fictional offline demonstration sample".into()),
        }).collect();
        Session {
            protocol: Default::default(),
            route_snapshots: Vec::new(),
            target_id: index as u64 + 1, generation: 0,
            target: format!("demo{}.example.net", index + 1), chosen_ip: Some(endpoint_address.clone()),
            resolved_candidates: vec![endpoint_address], address_family: Some("IPv4".into()),
            started_unix_ms: started, ended_unix_ms: Some(started + SAMPLE_RTT.len() as u64 * 1_000),
            route_timestamp_unix_ms: Some(started + 2_000), trace_complete: true, destination_reached: true,
            demonstration: true, origin: Some(origin), endpoint: Some(endpoint), hops, pings,
            tcp_checks: vec![TcpSample {
                observed_unix_ms: started + 20_000, port: 443, status: SampleStatus::Reply,
                connection_ms: Some(116.7 + offset), detail: Some("Fictional connection timing; no connection was made".into()),
            }],
            errors: vec![Diagnostic {
                observed_unix_ms: started, code: "demonstration".into(),
                detail: "Fictional offline observations. No DNS, ping, TCP, public-IP discovery or database download was performed.".into(),
            }],
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, net::Ipv4Addr};
    fn documentation_address(value: &str) -> bool {
        let Ok(ip) = value.parse::<Ipv4Addr>() else {
            return false;
        };
        let [a, b, c, _] = ip.octets();
        matches!((a, b, c), (192, 0, 2) | (198, 51, 100) | (203, 0, 113))
    }
    #[test]
    fn one_through_ten_sessions_have_independent_ids_and_demo_provenance() {
        for count in 1..=10 {
            let result = sessions(count);
            assert_eq!(result.len(), count);
            assert_eq!(
                result
                    .iter()
                    .map(|s| s.target_id)
                    .collect::<BTreeSet<_>>()
                    .len(),
                count
            );
            for session in result {
                assert!(session.demonstration);
                assert!(session.target.ends_with(".example.net"));
                assert!(session.errors.iter().any(|e| e.code == "demonstration"));
                for point in session.origin.iter().chain(session.endpoint.iter()).chain(
                    session
                        .hops
                        .iter()
                        .flat_map(|h| h.responses.iter().filter_map(|r| r.location.as_ref())),
                ) {
                    assert_eq!(point.provider.as_deref(), Some(FICTIONAL));
                    assert!(point.asn.is_some_and(|asn| (64496..=64511).contains(&asn)));
                    assert!(documentation_address(point.ip.as_deref().unwrap()));
                }
                for response in session.hops.iter().flat_map(|h| &h.responses) {
                    assert!(documentation_address(&response.address));
                }
                assert!(documentation_address(session.chosen_ip.as_deref().unwrap()));
            }
        }
        assert_eq!(sessions(0).len(), 1);
        assert_eq!(sessions(11).len(), 10);
    }
    #[test]
    fn default_has_four_places_and_preserves_silence_and_unknown_location() {
        let default = sessions(1).remove(0);
        let cities: BTreeSet<_> = default
            .origin
            .iter()
            .chain(
                default
                    .hops
                    .iter()
                    .flat_map(|h| h.responses.iter().filter_map(|r| r.location.as_ref())),
            )
            .filter_map(|p| p.city.as_deref())
            .collect();
        assert_eq!(
            cities,
            BTreeSet::from(["Budapest", "Frankfurt", "New York", "Toronto"])
        );
        assert!(
            default
                .hops
                .iter()
                .any(|h| h.responses.is_empty() && h.status == SampleStatus::Timeout)
        );
        assert!(
            default
                .hops
                .iter()
                .flat_map(|h| &h.responses)
                .any(|r| r.location.is_none())
        );
        let stats = default.statistics();
        assert_eq!(stats.sent, 24);
        assert_eq!(stats.replies, 23);
        assert_eq!(stats.nonresponses, 1);
        assert_eq!(default.pings.last().unwrap().rtt_ms, Some(112.4));
    }
}
