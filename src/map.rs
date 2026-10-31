//! Approved Simplemaps geography and its asset-specific Robinson calibration.
//!
//! Geometry is loaded once; callers cache static Canvas geometry per size/theme.
//! All coordinates use the original 2000 × 857 SVG coordinate system.
use std::sync::OnceLock;

pub const WIDTH: f32 = 2000.0;
pub const HEIGHT: f32 = 857.0;
pub const SOURCE_URL: &str = "https://simplemaps.com/resources/svg-world";
pub const MAP_NOTICE: &str = include_str!("../assets/SIMPLEMAPS-LICENSE.txt");
pub type MapPoint = (f32, f32);

#[derive(Debug)]
pub struct Polygon {
    #[cfg(test)]
    pub country: String,
    /// First ring is the exterior. Further rings retain source holes.
    pub rings: Vec<Vec<MapPoint>>,
}

#[derive(Debug)]
pub struct Geometry {
    /// The original 470 polygon parts, in original order.
    pub land: Vec<Polygon>,
    /// Boundary of the complete land union; shared borders cannot glow twice.
    pub coasts: Vec<Vec<MapPoint>>,
    /// Internal boundaries, topologically unioned so each line is drawn once.
    pub borders: Vec<Vec<MapPoint>>,
}

pub fn geometry() -> &'static Geometry {
    static MAP: OnceLock<Geometry> = OnceLock::new();
    MAP.get_or_init(|| decode(include_bytes!("../assets/world-geometry.bin")))
}

fn decode(mut bytes: &[u8]) -> Geometry {
    fn take<'a>(b: &mut &'a [u8], n: usize) -> &'a [u8] {
        let (head, tail) = b.split_at(n);
        *b = tail;
        head
    }
    fn n(b: &mut &[u8]) -> usize {
        u32::from_le_bytes(take(b, 4).try_into().unwrap()) as usize
    }
    fn line(b: &mut &[u8]) -> Vec<MapPoint> {
        let count = n(b);
        (0..count)
            .map(|_| {
                let x = f32::from_le_bytes(take(b, 4).try_into().unwrap());
                let y = f32::from_le_bytes(take(b, 4).try_into().unwrap());
                (x, y)
            })
            .collect()
    }
    fn lines(b: &mut &[u8]) -> Vec<Vec<MapPoint>> {
        let count = n(b);
        (0..count).map(|_| line(b)).collect()
    }
    assert_eq!(take(&mut bytes, 4), b"NTG1");
    let count = n(&mut bytes);
    let land = (0..count)
        .map(|_| {
            let length = n(&mut bytes);
            let name = take(&mut bytes, length);
            #[cfg(test)]
            let country = String::from_utf8(name.to_vec()).unwrap();
            #[cfg(not(test))]
            let _ = name;
            Polygon {
                #[cfg(test)]
                country,
                rings: lines(&mut bytes),
            }
        })
        .collect();
    let coasts = lines(&mut bytes);
    let borders = lines(&mut bytes);
    assert!(bytes.is_empty(), "unexpected geography data");
    Geometry {
        land,
        coasts,
        borders,
    }
}

// Robinson's published five-degree coefficient tables. Linear interpolation is
// the convention used to calibrate this particular supplied SVG; do not replace
// it with a different projection library's spline implementation.
const X: [f64; 19] = [
    1.0, 0.9986, 0.9954, 0.9900, 0.9822, 0.9730, 0.9600, 0.9427, 0.9216, 0.8962, 0.8679, 0.8350,
    0.7986, 0.7597, 0.7186, 0.6732, 0.6213, 0.5722, 0.5322,
];
const Y: [f64; 19] = [
    0.0, 0.0620, 0.1240, 0.1860, 0.2480, 0.3100, 0.3720, 0.4340, 0.4958, 0.5571, 0.6176, 0.6769,
    0.7346, 0.7903, 0.8435, 0.8936, 0.9394, 0.9761, 1.0,
];

/// Project an approximate geographical location onto the selected SVG.
///
/// Invalid coordinates and locations outside its supported extent remain
/// unlocated. Antarctica is absent from this asset. Nothing is clamped to an
/// invented border position. Longitude ±180 remains a distinct map seam.
pub fn project(latitude: f64, longitude: f64) -> Option<MapPoint> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-60.0..=84.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return None;
    }
    let (x, y) = project_unclipped(latitude, longitude)?;
    if !(0.0..=WIDTH as f64).contains(&x) || !(0.0..=HEIGHT as f64).contains(&y) {
        return None;
    }
    Some((x as f32, y as f32))
}

/// Rendering-only projection for clipped seam arcs. Never use an off-frame
/// result as a location marker; [`project`] performs the visibility check.
pub fn project_unclipped(latitude: f64, longitude: f64) -> Option<(f64, f64)> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-60.0..=84.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return None;
    }
    let band = latitude.abs() / 5.0;
    let i = (band.floor() as usize).min(17);
    let t = band - i as f64;
    let xx = X[i] + (X[i + 1] - X[i]) * t;
    let yy = Y[i] + (Y[i + 1] - Y[i]) * t;
    let x = 983.6 + 331.57069 * longitude.to_radians() * xx;
    let y = 501.35 - 515.98 * latitude.signum() * yy;
    Some((x, y))
}

/// A direct SVG arc between these longitudes would span the dateline seam.
/// Callers must omit that arc or split it into edge-clipped portions.
pub fn crosses_dateline(a_longitude: f64, b_longitude: f64) -> bool {
    (a_longitude - b_longitude).abs() > 180.0
}

/// Split a geographic segment at ±180°. Interpolate latitude for the purely
/// schematic edge connection. This is presentation geometry, not a cable path.
pub fn split_at_dateline(a: (f64, f64), b: (f64, f64)) -> Vec<((f64, f64), (f64, f64))> {
    if !crosses_dateline(a.1, b.1) {
        return vec![(a, b)];
    }
    let adjusted = if b.1 > a.1 { b.1 - 360.0 } else { b.1 + 360.0 };
    let seam = if adjusted > a.1 { 180.0 } else { -180.0 };
    let t = (seam - a.1) / (adjusted - a.1);
    let latitude = a.0 + (b.0 - a.0) * t;
    vec![(a, (latitude, seam)), ((latitude, -seam), b)]
}

/// Uniform full-world fit with modest glow padding. No map cropping/distortion.
pub fn fit(width: f32, height: f32, padding: f32) -> (f32, MapPoint) {
    let available_w = (width - padding * 2.0).max(0.0);
    let available_h = (height - padding * 2.0).max(0.0);
    let scale = (available_w / WIDTH).min(available_h / HEIGHT);
    (
        scale,
        (
            (width - WIDTH * scale) / 2.0,
            (height - HEIGHT * scale) / 2.0,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn in_ring(p: MapPoint, ring: &[MapPoint]) -> bool {
        let mut inside = false;
        for (a, b) in ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
        {
            if ((a.1 > p.1) != (b.1 > p.1)) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
                inside = !inside;
            }
        }
        inside
    }
    fn country_contains(name: &str, lat: f64, lon: f64) -> bool {
        let p = project(lat, lon).unwrap();
        geometry()
            .land
            .iter()
            .filter(|part| part.country == name)
            .any(|part| part.rings.iter().filter(|ring| in_ring(p, ring)).count() % 2 == 1)
    }
    #[test]
    fn original_geography_and_holes_survive() {
        let g = geometry();
        assert_eq!(g.land.len(), 470);
        assert_eq!(
            g.land
                .iter()
                .map(|p| &p.country)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            220
        );
        assert_eq!(g.land.iter().map(|p| p.rings.len()).sum::<usize>(), 471);
        assert!(
            g.land
                .iter()
                .any(|p| p.country == "South Africa" && p.rings.len() == 2)
        );
        assert!(!g.coasts.is_empty() && !g.borders.is_empty());
    }
    #[test]
    fn calibration_matches_supplied_controls() {
        let controls = [
            (47.4979, 19.0402, 1080.7906, 198.3023),
            (50.1109, 8.6821, 1027.1695, 182.0021),
            (40.7128, -74.006, 590.4546, 241.0180),
            (43.6532, -79.3832, 568.7511, 222.4173),
            (35.0116, 135.7681, 1724.2315, 277.3407),
            (-23.5505, -46.6333, 720.2997, 652.0297),
            (51.5074, -0.1278, 982.9655, 173.4562),
            (48.8566, 2.3522, 995.5021, 189.8194),
            (52.3676, 4.9041, 1007.7889, 168.1922),
            (59.3293, 18.0686, 1067.6146, 126.3047),
            (-23.698, 133.8807, 1739.3050, 652.9734),
        ];
        for (lat, lon, x, y) in controls {
            let p = project(lat, lon).unwrap();
            assert!((p.0 - x).abs() < 0.001 && (p.1 - y).abs() < 0.001);
        }
    }
    #[test]
    fn locations_land_in_expected_source_polygons() {
        for (country, lat, lon) in [
            ("Hungary", 47.4979, 19.0402),
            ("Germany", 50.1109, 8.6821),
            ("United States", 40.7128, -74.006),
            ("Canada", 43.6532, -79.3832),
            ("Japan", 35.0116, 135.7681),
            ("Brazil", -23.5505, -46.6333),
            ("United Kingdom", 51.5074, -0.1278),
            ("France", 48.8566, 2.3522),
            ("Netherlands", 52.3676, 4.9041),
            ("Sweden", 59.3293, 18.0686),
            ("Australia", -23.698, 133.8807),
            ("China", 39.9042, 116.4074),
            ("South Africa", -26.2041, 28.0473),
            ("Argentina", -34.6037, -58.3816),
            ("Russian Federation", 55.7558, 37.6173),
        ] {
            assert!(country_contains(country, lat, lon), "{country}");
        }
    }
    #[test]
    fn unsupported_and_nonfinite_positions_are_not_clamped() {
        for (lat, lon) in [
            (-80., 0.),
            (90., 0.),
            (0., 181.),
            (f64::NAN, 0.),
            (0., f64::INFINITY),
        ] {
            assert!(project(lat, lon).is_none());
        }
        assert!(project(0., 180.).is_none()); // Outside this asset's actual frame.
    }
    #[test]
    fn dateline_splits_never_cross_the_whole_map() {
        let pieces = split_at_dateline((-17., 179.), (-14., -172.));
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].1.1, 180.);
        assert_eq!(pieces[1].0.1, -180.);
        for (a, b) in pieces {
            assert!(!crosses_dateline(a.1, b.1));
        }
        assert_eq!(split_at_dateline((48., 19.), (43., -79.)).len(), 1);
    }
    #[test]
    fn uniform_fit_keeps_whole_map_visible() {
        let (s, (x, y)) = fit(420., 280., 10.);
        assert!(s > 0. && x >= 10. && y >= 10.);
        assert!(x + WIDTH * s <= 410.001 && y + HEIGHT * s <= 270.001);
    }
}
