//! Cached static atlas plus independent outbound route presentation.
use crate::map;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme};
use std::cell::Cell;

#[derive(Clone, Debug)]
pub struct MapNode {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
    /// One or more observations between this node and its predecessor have no
    /// geographical position. The schematic bridge remains dashed.
    pub unknown_before: bool,
}

#[derive(Clone, Debug)]
pub struct RouteVisual {
    pub id: u64,
    pub label: String,
    pub nodes: Vec<MapNode>,
    /// Presentation fraction supplied by the fixed eight-second monotonic clock.
    /// Never an RTT or a network timeout fraction.
    pub progress: f32,
}

#[derive(Debug, Default)]
pub struct MapCache {
    geography: Cache,
    dark: Cell<Option<bool>>,
}

impl MapCache {
    pub fn clear(&self) {
        self.geography.clear();
    }
}

pub struct MapView<'a> {
    pub cache: &'a MapCache,
    pub routes: &'a [RouteVisual],
    pub selected_id: Option<u64>,
    /// Current OS preference; this view exposes no theme control.
    pub dark: bool,
}

fn color(r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color {
        a: alpha,
        ..Color::from_rgb8(r, g, b)
    }
}
fn path_for_lines(
    lines: &[Vec<map::MapPoint>],
    scale: f32,
    offset: map::MapPoint,
    close: bool,
) -> Path {
    Path::new(|p| {
        for line in lines {
            if let Some(&(x, y)) = line.first() {
                p.move_to(Point::new(offset.0 + x * scale, offset.1 + y * scale));
                for &(x, y) in &line[1..] {
                    p.line_to(Point::new(offset.0 + x * scale, offset.1 + y * scale));
                }
                if close {
                    p.close();
                }
            }
        }
    })
}
fn stroke(frame: &mut Frame, path: &Path, width: f32, tint: Color) {
    frame.stroke(path, Stroke::default().with_width(width).with_color(tint));
}

impl<Message> canvas::Program<Message> for MapView<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        if self.cache.dark.replace(Some(self.dark)) != Some(self.dark) {
            self.cache.clear();
        }
        let (scale, offset) = map::fit(bounds.width, bounds.height, 12.);
        let background = self.cache.geography.draw(renderer, bounds.size(), |frame| {
            // Distinct ocean and neutral land in both OS schemes.
            let ocean = if self.dark {
                Color::from_rgb8(8, 15, 25)
            } else {
                Color::from_rgb8(215, 226, 235)
            };
            let land = if self.dark {
                Color::from_rgb8(20, 20, 20)
            } else {
                Color::from_rgb8(238, 238, 238)
            };
            frame.fill_rectangle(Point::ORIGIN, frame.size(), ocean);
            let data = map::geometry();
            for polygon in &data.land {
                frame.fill(&path_for_lines(&polygon.rings, scale, offset, true), land);
            }
            let coast = path_for_lines(&data.coasts, scale, offset, false);
            // Small nested strokes form a cached halo; no per-frame full-map blur.
            let halo_scale = (map::WIDTH * scale / 1000.0).clamp(0.35, 1.0);
            for (width, alpha) in [(9., 0.025), (6., 0.045), (3.6, 0.085), (1.9, 0.23)] {
                stroke(
                    frame,
                    &coast,
                    width * halo_scale,
                    color(25, 122, 255, if self.dark { alpha } else { alpha * 0.65 }),
                );
            }
            let borders = path_for_lines(&data.borders, scale, offset, false);
            stroke(
                frame,
                &borders,
                0.55,
                color(63, 154, 232, if self.dark { 0.38 } else { 0.45 }),
            );
            stroke(
                frame,
                &coast,
                0.85,
                color(69, 161, 255, if self.dark { 0.98 } else { 0.9 }),
            );
        });
        let mut routes = Frame::new(renderer, bounds.size());
        for visual in self.routes {
            let opacity = if self.selected_id.is_none() || self.selected_id == Some(visual.id) {
                1.
            } else {
                0.33
            };
            draw_route(
                &mut routes,
                visual,
                scale,
                offset,
                self.dark,
                opacity,
                self.routes.len() > 1,
            );
        }
        vec![background, routes.into_geometry()]
    }
}

fn curve(a: map::MapPoint, b: map::MapPoint, t: f32) -> map::MapPoint {
    let lift = ((a.0 - b.0).abs() * 0.25).clamp(25., 180.);
    let control = ((a.0 + b.0) * 0.5, a.1.min(b.1) - lift);
    let k = 1. - t;
    (
        k * k * a.0 + 2. * k * t * control.0 + t * t * b.0,
        k * k * a.1 + 2. * k * t * control.1 + t * t * b.1,
    )
}

// Liang–Barsky clipping clips route geometry, never its geographical markers.
fn clip_segment(a: map::MapPoint, b: map::MapPoint) -> Option<(map::MapPoint, map::MapPoint)> {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let p = [-dx, dx, -dy, dy];
    let q = [a.0, map::WIDTH - a.0, a.1, map::HEIGHT - a.1];
    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    for i in 0..4 {
        if p[i].abs() < f32::EPSILON {
            if q[i] < 0. {
                return None;
            }
        } else {
            let r = q[i] / p[i];
            if p[i] < 0. {
                lo = lo.max(r);
            } else {
                hi = hi.min(r);
            }
            if lo > hi {
                return None;
            }
        }
    }
    Some((
        (a.0 + lo * dx, a.1 + lo * dy),
        (a.0 + hi * dx, a.1 + hi * dy),
    ))
}

fn arc_path(
    a: map::MapPoint,
    b: map::MapPoint,
    start: f32,
    end: f32,
    scale: f32,
    offset: map::MapPoint,
    dashed: bool,
) -> Path {
    Path::new(|builder| {
        let steps = 64usize;
        for i in 0..steps {
            if dashed && i % 4 >= 2 {
                continue;
            }
            let t0 = (i as f32 / steps as f32).max(start);
            let t1 = ((i + 1) as f32 / steps as f32).min(end);
            if t1 <= t0 {
                continue;
            }
            if let Some((p, q)) = clip_segment(curve(a, b, t0), curve(a, b, t1)) {
                builder.move_to(Point::new(offset.0 + p.0 * scale, offset.1 + p.1 * scale));
                builder.line_to(Point::new(offset.0 + q.0 * scale, offset.1 + q.1 * scale));
            }
        }
    })
}

fn draw_route(
    frame: &mut Frame,
    visual: &RouteVisual,
    scale: f32,
    offset: map::MapPoint,
    dark: bool,
    opacity: f32,
    multi: bool,
) {
    let phase = visual.progress.clamp(0., 1.);
    let count = visual.nodes.len().saturating_sub(1).max(1);
    for (index, pair) in visual.nodes.windows(2).enumerate() {
        let fraction = (phase * count as f32 - index as f32).clamp(0., 1.);
        let pieces = map::split_at_dateline((pair[0].lat, pair[0].lon), (pair[1].lat, pair[1].lon));
        let piece_count = pieces.len() as f32;
        for (piece_index, (start, end)) in pieces.into_iter().enumerate() {
            let Some(a) = map::project_unclipped(start.0, start.1) else {
                continue;
            };
            let Some(b) = map::project_unclipped(end.0, end.1) else {
                continue;
            };
            let a = (a.0 as f32, a.1 as f32);
            let b = (b.0 as f32, b.1 as f32);
            let f = (fraction * piece_count - piece_index as f32).clamp(0., 1.);
            let dashed = pair[1].unknown_before;
            let ghost = arc_path(a, b, 0., 1., scale, offset, dashed);
            stroke(frame, &ghost, 0.85, color(120, 246, 158, 0.16 * opacity));
            if f > 0. {
                let line = arc_path(a, b, 0., f, scale, offset, dashed);
                if !dark {
                    stroke(frame, &line, 3.0, color(6, 70, 43, 0.8 * opacity));
                }
                stroke(frame, &line, 5.0, color(120, 239, 151, 0.075 * opacity));
                stroke(frame, &line, 1.6, color(154, 255, 179, 0.98 * opacity));
                let trail = arc_path(a, b, (f - 0.075).max(0.), f, scale, offset, dashed);
                stroke(frame, &trail, 2.4, color(207, 255, 220, 0.85 * opacity));
            }
            if f > 0. && f < 1. {
                let p = curve(a, b, f);
                if (0.0..=map::WIDTH).contains(&p.0) && (0.0..=map::HEIGHT).contains(&p.1) {
                    let p = Point::new(offset.0 + p.0 * scale, offset.1 + p.1 * scale);
                    frame.fill(&Path::circle(p, 6.), color(120, 255, 163, 0.12 * opacity));
                    frame.fill(&Path::circle(p, 2.5), color(219, 255, 230, opacity));
                }
            }
        }
    }
    for (index, node) in visual.nodes.iter().enumerate() {
        let Some(p) = map::project(node.lat, node.lon) else {
            continue;
        };
        let p = Point::new(offset.0 + p.0 * scale, offset.1 + p.1 * scale);
        let arrived = index == 0 || phase * count as f32 >= index as f32;
        let alpha = opacity * if arrived { 1. } else { 0.45 };
        frame.fill(&Path::circle(p, 9.), color(120, 255, 162, 0.09 * alpha));
        frame.fill(
            &Path::circle(p, 4.0),
            if dark {
                Color::from_rgb8(11, 24, 32)
            } else {
                Color::from_rgb8(228, 238, 240)
            },
        );
        stroke(
            frame,
            &Path::circle(p, 4.0),
            1.0,
            color(164, 248, 187, alpha),
        );
        frame.fill(&Path::circle(p, 1.7), color(205, 255, 221, alpha));
        // Label only a selected route in concurrent mode, preserving readability.
        if opacity > 0.5 && (!multi || index == 0 || index + 1 == visual.nodes.len()) {
            let is_end = index + 1 == visual.nodes.len();
            let label = if multi && is_end {
                format!("{}. {}", visual.id, visual.label)
            } else {
                node.label.clone()
            };
            if label.is_empty() {
                continue;
            }
            let available = (frame.width() - p.x - 12.).max(0.);
            let put_left = available < 140.;
            let x = if put_left {
                (p.x - 175.).max(4.)
            } else {
                p.x + 10.
            };
            let y = (p.y + if index % 2 == 0 { 13. } else { -29. })
                .clamp(4., (frame.height() - 45.).max(4.));
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(x, y),
                max_width: if put_left {
                    (p.x - x - 8.).max(50.)
                } else {
                    available.max(50.)
                },
                color: if dark {
                    color(216, 231, 242, opacity)
                } else {
                    color(22, 57, 77, opacity)
                },
                size: iced::Pixels(11.),
                ..Default::default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schematic_arc_clips_at_frame_without_moving_markers() {
        assert_eq!(
            clip_segment((-10., 100.), (10., 100.)),
            Some(((0., 100.), (10., 100.)))
        );
        assert!(clip_segment((-10., -10.), (-20., -20.)).is_none());
        assert_eq!(
            clip_segment((100., 100.), (200., 200.)),
            Some(((100., 100.), (200., 200.)))
        );
    }
}
