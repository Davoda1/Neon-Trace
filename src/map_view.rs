//! Cached static atlas plus independent outbound route presentation.
use crate::map;
use iced::mouse;
use iced::widget::canvas::{
    self, Cache, Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
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
    camera: Cell<Option<(f32, map::MapPoint, bool)>>,
}

impl MapCache {
    pub fn clear(&self) {
        self.geography.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapEvent {
    Selected { route_id: u64, node_index: usize },
}

#[derive(Debug, Default)]
pub struct MapState {
    hovered: Option<(u64, usize)>,
    control_held: bool,
    manual: Option<Navigation>,
    drag_from: Option<Point>,
    reset_epoch: u64,
}

pub struct MapView<'a> {
    pub cache: &'a MapCache,
    pub routes: &'a [RouteVisual],
    pub selected_id: Option<u64>,
    pub selected_node: Option<(u64, usize)>,
    /// Current OS preference; this view exposes no theme control.
    pub dark: bool,
    /// Increment to restore route framing after Clear, a new session or Fit map.
    pub reset_epoch: u64,
}

const ARC_STEPS: usize = 64;
const MAX_ROUTE_ZOOM: f32 = 4.;
const MAX_MANUAL_ZOOM: f32 = 12.;

/// A world-space center survives window resizing without accumulating screen
/// offsets. Longitude wraps, while latitude never pans beyond the atlas.
#[derive(Debug, Clone, Copy)]
struct Navigation {
    center: map::MapPoint,
    zoom: f32,
}

impl Navigation {
    fn base_scale(size: Size) -> f32 {
        map::fit(size.width, size.height, 16.).0.max(0.001)
    }

    fn from_camera(scale: f32, offset: map::MapPoint, size: Size) -> Self {
        let mut navigation = Self {
            center: (
                (size.width * 0.5 - offset.0) / scale,
                (size.height * 0.5 - offset.1) / scale,
            ),
            zoom: scale / Self::base_scale(size),
        };
        navigation.constrain(size);
        navigation
    }

    fn constrain(&mut self, size: Size) {
        self.zoom = self.zoom.clamp(1., MAX_MANUAL_ZOOM);
        self.center.0 = self.center.0.rem_euclid(map::WIDTH);
        let half_height = size.height / (2. * Self::base_scale(size) * self.zoom);
        self.center.1 = if half_height * 2. >= map::HEIGHT {
            map::HEIGHT * 0.5
        } else {
            self.center.1.clamp(half_height, map::HEIGHT - half_height)
        };
    }

    fn camera(mut self, size: Size) -> (f32, map::MapPoint) {
        self.constrain(size);
        let scale = Self::base_scale(size) * self.zoom;
        (
            scale,
            (
                size.width * 0.5 - self.center.0 * scale,
                size.height * 0.5 - self.center.1 * scale,
            ),
        )
    }

    fn zoom_at(&mut self, size: Size, anchor: Point, factor: f32) {
        let (old_scale, old_offset) = self.camera(size);
        let world = (
            (anchor.x - old_offset.0) / old_scale,
            (anchor.y - old_offset.1) / old_scale,
        );
        self.zoom = (self.zoom * factor).clamp(1., MAX_MANUAL_ZOOM);
        let new_scale = Self::base_scale(size) * self.zoom;
        self.center = (
            world.0 - (anchor.x - size.width * 0.5) / new_scale,
            world.1 - (anchor.y - size.height * 0.5) / new_scale,
        );
        self.constrain(size);
    }

    fn pan(&mut self, size: Size, delta: iced::Vector) {
        let scale = self.camera(size).0;
        self.center.0 -= delta.x / scale;
        self.center.1 -= delta.y / scale;
        self.constrain(size);
    }
}

/// Atlas offsets intersecting the viewport. Repeating the same offsets for
/// coastlines, routes and hit testing keeps every wrapped marker interactive.
fn visible_offsets(
    size: Size,
    scale: f32,
    offset: map::MapPoint,
    wrap: bool,
) -> Vec<map::MapPoint> {
    if !wrap {
        return vec![offset];
    }
    let period = map::WIDTH * scale;
    let first = (-offset.0 / period).floor() as i32;
    let last = ((size.width - offset.0) / period).floor() as i32;
    (first..=last)
        .map(|copy| (offset.0 + copy as f32 * period, offset.1))
        .collect()
}

/// Keep the route and its arcs in view with context around it. The atlas itself
/// retains its original projection and one uniform scale; only the camera moves.
/// Multi-target views fit the union, so choosing a target never hides another.
fn route_fit(routes: &[RouteVisual], size: Size) -> (f32, map::MapPoint) {
    let world = map::fit(size.width, size.height, 16.);
    if world.0 <= 0. {
        return world;
    }
    let mut extent: Option<(f32, f32, f32, f32)> = None;
    let mut include = |point: map::MapPoint| {
        let (x, y) = point;
        // Arc clipping uses the same original atlas extent. Never move a marker
        // to an invented coordinate just to make it fit.
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let x = x.clamp(0., map::WIDTH);
        let y = y.clamp(0., map::HEIGHT);
        extent = Some(match extent {
            Some((left, top, right, bottom)) => {
                (left.min(x), top.min(y), right.max(x), bottom.max(y))
            }
            None => (x, y, x, y),
        });
    };
    for route in routes {
        for node in &route.nodes {
            if let Some(point) = map::project(node.lat, node.lon) {
                include(point);
            }
        }
        for pair in route.nodes.windows(2) {
            for (a, b) in projected_pieces(&pair[0], &pair[1]) {
                include(a);
                include(b);
                // A quadratic's sole possible interior y extremum covers the
                // lifted arc; endpoint bounds already cover its monotonic x.
                let control_y = curve_control(a, b).1;
                let denominator = a.1 - 2. * control_y + b.1;
                if denominator.abs() > f32::EPSILON {
                    let t = (a.1 - control_y) / denominator;
                    if (0.0..1.0).contains(&t) {
                        include(curve(a, b, t));
                    }
                }
            }
        }
    }
    let Some((left, top, right, bottom)) = extent else {
        return world;
    };
    // A little extra horizontal space is useful for location labels. Relative
    // padding remains usable in the minimum-size window.
    let padding_x = (size.width * 0.14).clamp(28., 112.);
    let padding_y = (size.height * 0.19).clamp(22., 72.);
    let scale = ((size.width - 2. * padding_x).max(1.) / (right - left).max(1.))
        .min((size.height - 2. * padding_y).max(1.) / (bottom - top).max(1.))
        .clamp(world.0, world.0 * MAX_ROUTE_ZOOM);
    let mut offset = (
        size.width * 0.5 - (left + right) * 0.5 * scale,
        size.height * 0.5 - (top + bottom) * 0.5 * scale,
    );
    // Limit empty ocean beyond the atlas while retaining room for markers and
    // labels at its edge. The location stays exact; only the camera is offset.
    for (value, viewport, atlas, padding) in [
        (&mut offset.0, size.width, map::WIDTH, padding_x),
        (&mut offset.1, size.height, map::HEIGHT, padding_y),
    ] {
        if atlas * scale >= viewport {
            *value = value.clamp(viewport - atlas * scale - padding, padding);
        } else {
            *value = (viewport - atlas * scale) * 0.5;
        }
    }
    (scale, offset)
}

fn projected_pieces(a: &MapNode, b: &MapNode) -> Vec<(map::MapPoint, map::MapPoint)> {
    map::split_at_dateline((a.lat, a.lon), (b.lat, b.lon))
        .into_iter()
        .filter_map(|(a, b)| {
            let a = map::project_unclipped(a.0, a.1)?;
            let b = map::project_unclipped(b.0, b.1)?;
            Some(((a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32)))
        })
        .collect()
}

fn arc_length(a: map::MapPoint, b: map::MapPoint) -> f32 {
    let mut previous = a;
    (1..=ARC_STEPS)
        .map(|step| {
            let point = curve(a, b, step as f32 / ARC_STEPS as f32);
            let length = (point.0 - previous.0).hypot(point.1 - previous.1);
            previous = point;
            length
        })
        .sum()
}

/// Convert an arc-distance fraction to its quadratic parameter so the head
/// moves at an even visual speed, including on steeply curved segments.
fn arc_parameter(a: map::MapPoint, b: map::MapPoint, fraction: f32) -> f32 {
    let fraction = fraction.clamp(0., 1.);
    let total = arc_length(a, b);
    if total <= f32::EPSILON || fraction == 1. {
        return fraction;
    }
    let target = total * fraction;
    let mut covered = 0.;
    let mut previous = a;
    for step in 1..=ARC_STEPS {
        let point = curve(a, b, step as f32 / ARC_STEPS as f32);
        let length = (point.0 - previous.0).hypot(point.1 - previous.1);
        if covered + length >= target && length > 0. {
            return ((step - 1) as f32 + (target - covered) / length) / ARC_STEPS as f32;
        }
        covered += length;
        previous = point;
    }
    1.
}

fn segment_weights(nodes: &[MapNode]) -> Vec<f32> {
    let mut weights: Vec<f32> = nodes
        .windows(2)
        .map(|pair| {
            projected_pieces(&pair[0], &pair[1])
                .into_iter()
                .map(|(a, b)| arc_length(a, b))
                .sum()
        })
        .collect();
    // Very short/coincident geolocation estimates should still receive a brief
    // visible stage. Beyond that floor, duration is proportional to arc length.
    let minimum = weights.iter().copied().fold(0_f32, f32::max) * 0.025;
    for weight in &mut weights {
        *weight = weight.max(minimum).max(0.001);
    }
    weights
}

fn weighted_progress(progress: f32, weights: &[f32], index: usize) -> f32 {
    let Some(&weight) = weights.get(index) else {
        return 0.;
    };
    if progress >= 1. {
        return 1.;
    }
    let total: f32 = weights.iter().sum();
    let before: f32 = weights[..index].iter().sum();
    ((progress.clamp(0., 1.) * total - before) / weight).clamp(0., 1.)
}

fn weighted_progresses(progress: f32, weights: &[f32]) -> Vec<f32> {
    let distance = progress.clamp(0., 1.) * weights.iter().sum::<f32>();
    let mut before = 0.;
    weights
        .iter()
        .map(|&weight| {
            let fraction = if progress >= 1. {
                1.
            } else {
                ((distance - before) / weight).clamp(0., 1.)
            };
            before += weight;
            fraction
        })
        .collect()
}

impl RouteVisual {
    /// The map and bottom journey bars share exactly the same distance timing.
    /// Compute the whole strip in one pass rather than resampling for every bar.
    pub fn segment_progresses(&self) -> Vec<f32> {
        weighted_progresses(self.progress, &segment_weights(&self.nodes))
    }

    #[cfg(test)]
    fn segment_progress(&self, index: usize) -> f32 {
        self.segment_progresses().get(index).copied().unwrap_or(0.)
    }

    #[cfg(test)]
    fn segment_completed(&self, index: usize) -> bool {
        self.segment_progress(index) >= 1.
    }
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
    frame.stroke(
        path,
        Stroke::default()
            .with_width(width)
            .with_color(tint)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

impl MapView<'_> {
    fn camera(&self, state: &MapState, size: Size) -> (f32, map::MapPoint, bool) {
        if state.reset_epoch == self.reset_epoch {
            if let Some(navigation) = state.manual {
                let (scale, offset) = navigation.camera(size);
                return (scale, offset, true);
            }
        }
        let (scale, offset) = route_fit(self.routes, size);
        (scale, offset, false)
    }

    fn node_at(
        &self,
        state: &MapState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<(u64, usize)> {
        if state.control_held || state.drag_from.is_some() {
            return None;
        }
        let position = cursor.position_in(bounds)?;
        let (scale, offset, wrap) = self.camera(state, bounds.size());
        visible_offsets(bounds.size(), scale, offset, wrap)
            .into_iter()
            .find_map(|offset| hit_node(self.routes, self.selected_id, position, scale, offset))
    }
}

impl canvas::Program<MapEvent> for MapView<'_> {
    type State = MapState;

    fn update(
        &self,
        state: &mut MapState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<MapEvent>> {
        if state.reset_epoch != self.reset_epoch {
            state.manual = None;
            state.drag_from = None;
            state.hovered = None;
            state.reset_epoch = self.reset_epoch;
        }
        match event {
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers))
            | canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed { modifiers, .. })
            | canvas::Event::Keyboard(iced::keyboard::Event::KeyReleased { modifiers, .. }) => {
                let control_held = modifiers.control();
                if state.control_held != control_held {
                    state.control_held = control_held;
                    if !control_held {
                        state.drag_from = None;
                    }
                    state.hovered = None;
                    return Some(canvas::Action::request_redraw());
                }
            }
            canvas::Event::Window(iced::window::Event::Unfocused) => {
                state.control_held = false;
                state.drag_from = None;
                state.hovered = None;
                return Some(canvas::Action::request_redraw());
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if state.control_held && cursor.is_over(bounds) =>
            {
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 100.,
                };
                if amount.is_finite() && amount != 0. {
                    let (scale, offset, _) = self.camera(state, bounds.size());
                    let mut navigation = state
                        .manual
                        .unwrap_or_else(|| Navigation::from_camera(scale, offset, bounds.size()));
                    navigation.zoom_at(
                        bounds.size(),
                        cursor.position_in(bounds).unwrap(),
                        1.18_f32.powf(amount.clamp(-16., 16.)),
                    );
                    state.manual = Some(navigation);
                    state.hovered = None;
                    return Some(canvas::Action::request_redraw().and_capture());
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if state.control_held && cursor.is_over(bounds) =>
            {
                let (scale, offset, _) = self.camera(state, bounds.size());
                state
                    .manual
                    .get_or_insert_with(|| Navigation::from_camera(scale, offset, bounds.size()));
                state.drag_from = cursor.position();
                state.hovered = None;
                return Some(canvas::Action::request_redraw().and_capture());
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(previous), Some(current)) = (state.drag_from, cursor.position()) {
                    if state.control_held {
                        if let Some(navigation) = state.manual.as_mut() {
                            navigation.pan(bounds.size(), current - previous);
                        }
                        state.drag_from = Some(current);
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag_from.take().is_some() {
                    return Some(canvas::Action::request_redraw().and_capture());
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => {
                state.drag_from = None;
            }
            _ => {}
        }
        let hovered = self.node_at(state, bounds, cursor);
        if matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            if let Some((route_id, node_index)) = hovered {
                state.hovered = hovered;
                return Some(
                    canvas::Action::publish(MapEvent::Selected {
                        route_id,
                        node_index,
                    })
                    .and_capture(),
                );
            }
        }
        if state.hovered != hovered {
            state.hovered = hovered;
            return Some(canvas::Action::request_redraw());
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &MapState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag_from.is_some() && state.control_held {
            mouse::Interaction::Grabbing
        } else if state.control_held && cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else if self.node_at(state, bounds, cursor).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        state: &MapState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if self.cache.dark.replace(Some(self.dark)) != Some(self.dark) {
            self.cache.clear();
        }
        let camera = self.camera(state, bounds.size());
        let (scale, offset, wrap) = camera;
        let copies = visible_offsets(bounds.size(), scale, offset, wrap);
        if self.cache.camera.replace(Some(camera)) != Some(camera) {
            self.cache.clear();
        }
        let background = self.cache.geography.draw(renderer, bounds.size(), |frame| {
            // Two gentle gradients provide ocean depth without a per-frame blur.
            let (edge, center) = if self.dark {
                (color(5, 11, 19, 1.), color(13, 26, 43, 1.))
            } else {
                (color(203, 218, 230, 1.), color(229, 237, 244, 1.))
            };
            frame.fill_rectangle(
                Point::ORIGIN,
                frame.size(),
                gradient::Linear::new(Point::ORIGIN, Point::new(frame.width(), 0.))
                    .add_stop(0., edge)
                    .add_stop(0.5, center)
                    .add_stop(1., edge),
            );
            let vertical = if self.dark {
                color(3, 7, 12, 0.46)
            } else {
                color(172, 195, 212, 0.20)
            };
            frame.fill_rectangle(
                Point::ORIGIN,
                frame.size(),
                gradient::Linear::new(Point::ORIGIN, Point::new(0., frame.height()))
                    .add_stop(0., vertical)
                    .add_stop(0.45, Color::TRANSPARENT)
                    .add_stop(1., vertical),
            );
            let land = if self.dark {
                color(20, 20, 20, 1.)
            } else {
                color(238, 238, 238, 1.)
            };
            let data = map::geometry();
            for &offset in &copies {
                for polygon in &data.land {
                    frame.fill(&path_for_lines(&polygon.rings, scale, offset, true), land);
                }
                let coast = path_for_lines(&data.coasts, scale, offset, false);
                // Rounded joins prevent the long miter spikes of broad coastal strokes.
                // Static geometry remains cached; the screen-space halo stays restrained.
                let halo_scale = (map::WIDTH * scale / 1000.0).clamp(0.5, 1.15);
                for (width, alpha) in [
                    (12., 0.016),
                    (8.5, 0.026),
                    (5.5, 0.046),
                    (3.2, 0.095),
                    (1.8, 0.25),
                ] {
                    stroke(
                        frame,
                        &coast,
                        width * halo_scale,
                        color(29, 122, 255, if self.dark { alpha } else { alpha * 0.62 }),
                    );
                }
                let borders = path_for_lines(&data.borders, scale, offset, false);
                stroke(
                    frame,
                    &borders,
                    0.55,
                    color(64, 139, 203, if self.dark { 0.34 } else { 0.42 }),
                );
                stroke(
                    frame,
                    &coast,
                    0.8,
                    color(84, 170, 255, if self.dark { 0.96 } else { 0.88 }),
                );
            }
        });
        // Recompute from current route geometry. A route snapshot can change while
        // the cursor stays still, so the last event's tuple is only redraw state.
        let hovered = self.node_at(state, bounds, cursor);
        let mut routes = Frame::new(renderer, bounds.size());
        let mut labels = Vec::new();
        let mut markers = Vec::new();
        for (ordinal, visual) in self.routes.iter().enumerate() {
            let opacity = if self.selected_id.is_none() || self.selected_id == Some(visual.id) {
                1.
            } else {
                0.82
            };
            for &offset in &copies {
                draw_route(
                    &mut routes,
                    visual,
                    scale,
                    offset,
                    self.dark,
                    opacity,
                    self.routes.len() > 1,
                    ordinal,
                    self.selected_node,
                    hovered,
                    &mut labels,
                    &mut markers,
                );
            }
        }
        // Shared origins in multi mode need one label, not one per target.
        let mut unique_labels: Vec<Label> = Vec::with_capacity(labels.len());
        for label in labels {
            if !unique_labels.iter().any(|other| {
                other.content == label.content
                    && (other.anchor.x - label.anchor.x).abs() < 1.
                    && (other.anchor.y - label.anchor.y).abs() < 1.
            }) {
                unique_labels.push(label);
            }
        }
        let mut labels = unique_labels;
        // Place selected labels first and reserve all marker footprints. This keeps
        // geography stable while avoiding text collisions around nearby cities.
        labels.sort_by_key(|label: &Label| !label.emphasized);
        let mut occupied: Vec<Rectangle> = markers
            .into_iter()
            .map(|point| Rectangle {
                x: point.x - 9.,
                y: point.y - 9.,
                width: 18.,
                height: 18.,
            })
            .collect();
        for label in &labels {
            draw_label(&mut routes, label, &mut occupied, self.dark);
        }
        if cursor.is_over(bounds) {
            if let Some((route_id, index)) = hovered {
                if let Some(visual) = self.routes.iter().find(|route| route.id == route_id) {
                    if let Some(node) = visual.nodes.get(index) {
                        if let Some(position) = cursor.position_in(bounds) {
                            draw_tooltip(
                                &mut routes,
                                position,
                                &node.label,
                                &visual.label,
                                self.dark,
                            );
                        }
                    }
                }
            }
        }
        vec![background, routes.into_geometry()]
    }
}

fn hit_node(
    routes: &[RouteVisual],
    selected: Option<u64>,
    cursor: Point,
    scale: f32,
    offset: map::MapPoint,
) -> Option<(u64, usize)> {
    let mut best: Option<(f32, (u64, usize))> = None;
    for route in routes {
        for (index, node) in route.nodes.iter().enumerate() {
            let Some((x, y)) = map::project(node.lat, node.lon) else {
                continue;
            };
            let distance = (cursor.x - offset.0 - x * scale).powi(2)
                + (cursor.y - offset.1 - y * scale).powi(2);
            if distance <= 13_f32.powi(2) {
                // At coincident locations keep inspection on the selected route.
                // Every route remains independently selectable from the target list.
                let score = distance - if selected == Some(route.id) { 0.5 } else { 0. };
                if best.as_ref().is_none_or(|(previous, _)| score < *previous) {
                    best = Some((score, (route.id, index)));
                }
            }
        }
    }
    best.map(|(_, node)| node)
}

struct Label {
    anchor: Point,
    content: String,
    emphasized: bool,
    opacity: f32,
}

fn overlap_area(a: Rectangle, b: Rectangle) -> f32 {
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    width.max(0.) * height.max(0.)
}

fn label_rectangle(anchor: Point, size: Size, bounds: Size, occupied: &[Rectangle]) -> Rectangle {
    let candidates = [
        (anchor.x + 13., anchor.y - size.height - 9.),
        (anchor.x + 13., anchor.y + 10.),
        (anchor.x - size.width - 13., anchor.y - size.height - 9.),
        (anchor.x - size.width - 13., anchor.y + 10.),
        (anchor.x - size.width / 2., anchor.y - size.height - 27.),
        (anchor.x - size.width / 2., anchor.y + 28.),
        (anchor.x + 25., anchor.y - size.height - 47.),
        (anchor.x - size.width - 25., anchor.y + 48.),
    ];
    candidates
        .into_iter()
        .enumerate()
        .map(|(order, (x, y))| {
            let rect = Rectangle {
                x: x.clamp(4., (bounds.width - size.width - 4.).max(4.)),
                y: y.clamp(4., (bounds.height - size.height - 4.).max(4.)),
                width: size.width,
                height: size.height,
            };
            let overlap: f32 = occupied
                .iter()
                .map(|other| overlap_area(rect, *other))
                .sum();
            (
                overlap * 100. + order as f32 + (rect.x - x).abs() + (rect.y - y).abs(),
                rect,
            )
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
        .1
}

fn draw_label(frame: &mut Frame, label: &Label, occupied: &mut Vec<Rectangle>, dark: bool) {
    let max_width = (frame.width() - 8.).clamp(60., 220.);
    let mut label_text = canvas::Text {
        content: label.content.clone(),
        size: iced::Pixels(12.),
        color: if dark {
            color(215, 231, 242, label.opacity)
        } else {
            color(30, 61, 83, label.opacity)
        },
        ..Default::default()
    };
    // Measure unwrapped glyphs first, then measure again at the bounded width.
    // The small extra allowance prevents fractional rounding from wrapping the
    // final letter of a short name into an otherwise unnecessary second line.
    let width = (text_size(&label_text).width.ceil() + 20.).clamp(40., max_width);
    label_text.max_width = width - 16.;
    let height = text_size(&label_text).height.ceil() + 10.;
    let rect = label_rectangle(
        label.anchor,
        Size::new(width, height),
        frame.size(),
        occupied,
    );
    occupied.push(Rectangle {
        x: rect.x - 3.,
        y: rect.y - 3.,
        width: rect.width + 6.,
        height: rect.height + 6.,
    });
    let end = Point::new(
        label.anchor.x.clamp(rect.x, rect.x + rect.width),
        label.anchor.y.clamp(rect.y, rect.y + rect.height),
    );
    stroke(
        frame,
        &Path::line(label.anchor, end),
        0.65,
        if dark {
            color(112, 146, 172, 0.48 * label.opacity)
        } else {
            color(57, 91, 113, 0.5 * label.opacity)
        },
    );
    let panel = Path::rounded_rectangle(Point::new(rect.x, rect.y), rect.size(), 5.0.into());
    frame.fill(
        &panel,
        if dark {
            color(9, 18, 29, 0.85)
        } else {
            color(234, 242, 247, 0.93)
        },
    );
    if label.emphasized {
        stroke(
            frame,
            &panel,
            0.8,
            if dark {
                color(123, 231, 160, 0.7)
            } else {
                color(31, 127, 77, 0.7)
            },
        );
    }
    label_text.position = Point::new(rect.x + 8., rect.y + 5.);
    frame.fill_text(label_text);
}

fn text_size(text: &canvas::Text) -> Size {
    use iced::advanced::text::Paragraph as _;
    // Measure using the same paragraph engine, font, shaping and wrapping as
    // Canvas text. Character counts cannot predict wrapped Cyrillic/long names.
    iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
        content: text.content.as_str(),
        bounds: Size::new(text.max_width, f32::INFINITY),
        size: text.size,
        line_height: text.line_height,
        font: text.font,
        align_x: text.align_x,
        align_y: text.align_y,
        shaping: text.shaping,
        wrapping: iced::advanced::text::Wrapping::default(),
    })
    .min_bounds()
}

fn text_height(text: &canvas::Text) -> f32 {
    text_size(text).height.ceil()
}

fn draw_tooltip(frame: &mut Frame, cursor: Point, location: &str, target: &str, dark: bool) {
    let width = (location.chars().count().max(target.chars().count()) as f32 * 7. + 26.)
        .clamp(125., (frame.width() - 16.).clamp(125., 310.));
    // These are data labels only; no untranslated interface text is inserted.
    let mut location_text = canvas::Text {
        content: location.into(),
        max_width: width - 24.,
        size: iced::Pixels(13.),
        color: if dark {
            color(235, 246, 250, 1.)
        } else {
            color(25, 50, 68, 1.)
        },
        ..Default::default()
    };
    let mut target_text = canvas::Text {
        content: target.into(),
        max_width: width - 24.,
        size: iced::Pixels(11.),
        color: if dark {
            color(147, 171, 189, 1.)
        } else {
            color(82, 112, 131, 1.)
        },
        ..Default::default()
    };
    let location_height = text_height(&location_text);
    let target_height = text_height(&target_text);
    let height = 20. + location_height + 6. + target_height;
    let x = (cursor.x + 20.).clamp(8., (frame.width() - width - 8.).max(8.));
    let y = (cursor.y + 24.).clamp(8., (frame.height() - height - 8.).max(8.));
    let panel = Path::rounded_rectangle(Point::new(x, y), Size::new(width, height), 7.0.into());
    frame.fill(
        &panel,
        if dark {
            color(18, 29, 42, 0.98)
        } else {
            color(246, 250, 252, 0.99)
        },
    );
    stroke(
        frame,
        &panel,
        1.,
        if dark {
            color(65, 99, 124, 1.)
        } else {
            color(144, 174, 194, 1.)
        },
    );
    location_text.position = Point::new(x + 12., y + 10.);
    target_text.position = Point::new(x + 12., y + 10. + location_height + 6.);
    frame.fill_text(location_text);
    frame.fill_text(target_text);
}

fn curve_control(a: map::MapPoint, b: map::MapPoint) -> map::MapPoint {
    let lift = ((a.0 - b.0).hypot(a.1 - b.1) * 0.25).min(180.);
    ((a.0 + b.0) * 0.5, a.1.min(b.1) - lift)
}

fn curve(a: map::MapPoint, b: map::MapPoint, t: f32) -> map::MapPoint {
    let control = curve_control(a, b);
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
        let steps = ARC_STEPS;
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
    ordinal: usize,
    selected: Option<(u64, usize)>,
    hovered: Option<(u64, usize)>,
    labels: &mut Vec<Label>,
    markers: &mut Vec<Point>,
) {
    let fractions = visual.segment_progresses();
    for (index, pair) in visual.nodes.windows(2).enumerate() {
        let fraction = fractions[index];
        let pieces = projected_pieces(&pair[0], &pair[1]);
        let piece_lengths: Vec<f32> = pieces
            .iter()
            .map(|&(a, b)| arc_length(a, b).max(0.001))
            .collect();
        for (piece_index, (a, b)) in pieces.into_iter().enumerate() {
            let f = weighted_progress(fraction, &piece_lengths, piece_index);
            let t = arc_parameter(a, b, f);
            let dashed = pair[1].unknown_before;
            let route_color = |alpha| {
                if dashed {
                    color(222, 190, 125, alpha)
                } else {
                    color(155, 255, 183, alpha)
                }
            };
            let trail_color = |alpha| {
                if dashed {
                    color(255, 229, 175, alpha)
                } else {
                    color(207, 255, 220, alpha)
                }
            };
            let ghost = arc_path(a, b, 0., 1., scale, offset, dashed);
            stroke(frame, &ghost, 0.85, route_color(0.20 * opacity));
            if f > 0. {
                let line = arc_path(a, b, 0., t, scale, offset, dashed);
                if !dark {
                    stroke(
                        frame,
                        &line,
                        3.0,
                        if dashed {
                            color(100, 70, 23, 0.8 * opacity)
                        } else {
                            color(6, 70, 43, 0.8 * opacity)
                        },
                    );
                }
                stroke(frame, &line, 5.0, route_color(0.075 * opacity));
                stroke(frame, &line, 1.6, route_color(0.98 * opacity));
                let trail_start = arc_parameter(a, b, (f - 0.075).max(0.));
                let trail = arc_path(a, b, trail_start, t, scale, offset, dashed);
                stroke(frame, &trail, 2.4, trail_color(0.85 * opacity));
            }
            if f > 0. && f < 1. {
                let p = curve(a, b, t);
                if (0.0..=map::WIDTH).contains(&p.0) && (0.0..=map::HEIGHT).contains(&p.1) {
                    let p = Point::new(offset.0 + p.0 * scale, offset.1 + p.1 * scale);
                    frame.fill(&Path::circle(p, 9.), route_color(0.06 * opacity));
                    frame.fill(&Path::circle(p, 5.5), route_color(0.19 * opacity));
                    frame.fill(&Path::circle(p, 2.5), trail_color(opacity));
                }
            }
        }
    }
    for (index, node) in visual.nodes.iter().enumerate() {
        let Some(p) = map::project(node.lat, node.lon) else {
            continue;
        };
        let p = Point::new(offset.0 + p.0 * scale, offset.1 + p.1 * scale);
        if !Rectangle::with_size(frame.size()).contains(p) {
            continue;
        }
        let arrived = index == 0 || fractions[index - 1] >= 1.;
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
        markers.push(p);
        let emphasized =
            selected == Some((visual.id, index)) || hovered == Some((visual.id, index));
        if emphasized {
            stroke(
                frame,
                &Path::circle(p, 10.),
                1.15,
                if dark {
                    color(179, 255, 204, 0.9)
                } else {
                    color(30, 135, 76, 0.95)
                },
            );
        }
        if multi && index + 1 == visual.nodes.len() {
            let badge = Point::new(p.x, p.y - 15.);
            frame.fill(
                &Path::circle(badge, 8.),
                if dark {
                    color(15, 38, 29, 1.)
                } else {
                    color(222, 245, 230, 1.)
                },
            );
            frame.fill_text(canvas::Text {
                content: (ordinal + 1).to_string(),
                position: Point::new(badge.x - 3.5, badge.y - 6.),
                color: if dark {
                    color(181, 250, 203, opacity)
                } else {
                    color(29, 110, 62, opacity)
                },
                size: iced::Pixels(10.),
                ..Default::default()
            });
        }
        if (opacity > 0.5 && (!multi || index == 0 || index + 1 == visual.nodes.len()))
            || emphasized
        {
            let label = if multi && index + 1 == visual.nodes.len() {
                format!("{} · {}", ordinal + 1, node.label)
            } else {
                node.label.clone()
            };
            if !label.is_empty() {
                labels.push(Label {
                    anchor: p,
                    content: label,
                    emphasized,
                    opacity: if emphasized { 1. } else { opacity },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route_at(points: &[(f64, f64)]) -> RouteVisual {
        RouteVisual {
            id: 1,
            label: "route".into(),
            progress: 0.,
            nodes: points
                .iter()
                .map(|&(lat, lon)| MapNode {
                    lat,
                    lon,
                    label: String::new(),
                    unknown_before: false,
                })
                .collect(),
        }
    }

    #[test]
    fn manual_zoom_anchors_cursor_and_has_finite_limits() {
        let size = Size::new(900., 500.);
        let mut navigation = Navigation {
            center: (1000., 430.),
            zoom: 3.,
        };
        let cursor = Point::new(320., 300.);
        let world_at = |navigation: Navigation| {
            let (scale, offset) = navigation.camera(size);
            ((cursor.x - offset.0) / scale, (cursor.y - offset.1) / scale)
        };
        let before = world_at(navigation);
        navigation.zoom_at(size, cursor, 1.18);
        let after = world_at(navigation);
        assert!((before.0 - after.0).abs() < 0.001);
        assert!((before.1 - after.1).abs() < 0.001);
        navigation.zoom_at(size, cursor, 1000.);
        assert_eq!(navigation.zoom, MAX_MANUAL_ZOOM);
        navigation.zoom_at(size, cursor, 0.00001);
        assert_eq!(navigation.zoom, 1.);
        assert!(navigation.center.0.is_finite() && navigation.center.1.is_finite());
    }

    #[test]
    fn manual_pan_wraps_horizontally_and_stops_at_vertical_edges() {
        let size = Size::new(900., 500.);
        let mut navigation = Navigation {
            center: (1000., 430.),
            zoom: 3.,
        };
        for delta in [
            iced::Vector::new(80_000., 90_000.),
            iced::Vector::new(-180_000., -180_000.),
        ] {
            navigation.pan(size, delta);
            assert!((0.0..map::WIDTH).contains(&navigation.center.0));
            let (scale, offset) = navigation.camera(size);
            assert!(offset.1 <= 0.001);
            assert!(offset.1 + map::HEIGHT * scale >= size.height - 0.001);
            let copies = visible_offsets(size, scale, offset, true);
            assert!(copies.first().unwrap().0 <= 0.);
            assert!(copies.last().unwrap().0 + map::WIDTH * scale >= size.width);
        }
        navigation.zoom = 1.;
        navigation.pan(size, iced::Vector::new(0., 80_000.));
        // At a full-world view, unused vertical canvas is centered and immovable.
        assert_eq!(navigation.center.1, map::HEIGHT * 0.5);
    }

    #[test]
    fn full_horizontal_period_returns_same_camera_and_wrapped_markers_are_interactive() {
        let size = Size::new(900., 500.);
        let mut navigation = Navigation {
            center: (50., 430.),
            zoom: 1.,
        };
        let first = navigation.camera(size);
        navigation.pan(size, iced::Vector::new(map::WIDTH * first.0, 0.));
        let second = navigation.camera(size);
        assert!((first.1.0 - second.1.0).abs() < 0.001);
        let cache = MapCache::default();
        let route = route_at(&[(0., 175.)]);
        let point = map::project(0., 175.).unwrap();
        let state = MapState {
            manual: Some(navigation),
            ..MapState::default()
        };
        let view = MapView {
            cache: &cache,
            routes: std::slice::from_ref(&route),
            selected_id: Some(1),
            selected_node: None,
            dark: true,
            reset_epoch: 0,
        };
        let (scale, offset, wrap) = view.camera(&state, size);
        let point = visible_offsets(size, scale, offset, wrap)
            .into_iter()
            .map(|offset| Point::new(offset.0 + point.0 * scale, offset.1 + point.1 * scale))
            .find(|point| Rectangle::with_size(size).contains(*point))
            .unwrap();
        assert_eq!(
            view.node_at(
                &state,
                Rectangle::with_size(size),
                mouse::Cursor::Available(point)
            ),
            Some((1, 0))
        );
    }

    #[test]
    fn wheel_requires_control_and_drag_does_not_select_a_marker() {
        use canvas::Program;
        let cache = MapCache::default();
        let route = route_at(&[(47.5, 19.04)]);
        let view = MapView {
            cache: &cache,
            routes: std::slice::from_ref(&route),
            selected_id: Some(1),
            selected_node: None,
            dark: true,
            reset_epoch: 0,
        };
        let bounds = Rectangle::with_size(Size::new(900., 500.));
        let (scale, offset) = route_fit(view.routes, bounds.size());
        let point = map::project(47.5, 19.04).unwrap();
        let cursor = mouse::Cursor::Available(Point::new(
            offset.0 + point.0 * scale,
            offset.1 + point.1 * scale,
        ));
        let wheel = canvas::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0., y: 1. },
        });
        let mut state = MapState::default();
        let _ = view.update(&mut state, &wheel, bounds, cursor);
        assert!(state.manual.is_none());
        let _ = view.update(
            &mut state,
            &canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                iced::keyboard::Modifiers::CTRL,
            )),
            bounds,
            cursor,
        );
        let action = view
            .update(&mut state, &wheel, bounds, cursor)
            .unwrap()
            .into_inner();
        assert_eq!(action.2, iced::event::Status::Captured);
        assert!(action.0.is_none() && state.manual.is_some());
        let action = view
            .update(
                &mut state,
                &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                cursor,
            )
            .unwrap()
            .into_inner();
        assert!(action.0.is_none() && state.drag_from.is_some());
        assert!(view.node_at(&state, bounds, cursor).is_none());
        let _ = view.update(
            &mut state,
            &canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                iced::keyboard::Modifiers::empty(),
            )),
            bounds,
            cursor,
        );
        assert!(!state.control_held && state.drag_from.is_none());
    }

    #[test]
    fn camera_reset_epoch_discards_manual_view_and_focus_loss_releases_drag() {
        use canvas::Program;
        let cache = MapCache::default();
        let route = route_at(&[(47.5, 19.04), (40.7, -74.)]);
        let view = MapView {
            cache: &cache,
            routes: std::slice::from_ref(&route),
            selected_id: Some(1),
            selected_node: None,
            dark: true,
            reset_epoch: 2,
        };
        let mut state = MapState {
            manual: Some(Navigation {
                center: (100., 200.),
                zoom: 10.,
            }),
            control_held: true,
            drag_from: Some(Point::new(10., 20.)),
            reset_epoch: 1,
            ..MapState::default()
        };
        let size = Size::new(900., 500.);
        let (scale, offset, wrapped) = view.camera(&state, size);
        assert!(!wrapped);
        assert_eq!((scale, offset), route_fit(view.routes, size));
        let _ = view.update(
            &mut state,
            &canvas::Event::Window(iced::window::Event::Unfocused),
            Rectangle::with_size(size),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(state.reset_epoch, 2);
        assert!(state.manual.is_none() && state.drag_from.is_none() && !state.control_held);
    }

    #[test]
    fn camera_fits_regional_route_and_keeps_curved_path_visible() {
        let size = Size::new(900., 500.);
        let route = route_at(&[(47.4979, 19.0402), (50.1109, 8.6821), (51.5074, -0.1278)]);
        let (scale, offset) = route_fit(std::slice::from_ref(&route), size);
        let world_scale = map::fit(size.width, size.height, 16.).0;
        assert!(scale > world_scale * 2.);
        assert!(scale <= world_scale * MAX_ROUTE_ZOOM);
        for pair in route.nodes.windows(2) {
            for (a, b) in projected_pieces(&pair[0], &pair[1]) {
                for step in 0..=64 {
                    let p = curve(a, b, step as f32 / 64.);
                    let x = offset.0 + p.0 * scale;
                    let y = offset.1 + p.1 * scale;
                    assert!((50.0..850.0).contains(&x), "x={x}");
                    assert!((40.0..460.0).contains(&y), "y={y}");
                }
            }
        }
    }

    #[test]
    fn camera_keeps_markers_inset_near_the_atlas_edge() {
        let size = Size::new(800., 350.);
        let route = route_at(&[(0., 174.), (0., 175.)]);
        let (scale, offset) = route_fit(std::slice::from_ref(&route), size);
        for node in &route.nodes {
            let p = map::project(node.lat, node.lon).unwrap();
            let x = offset.0 + p.0 * scale;
            let y = offset.1 + p.1 * scale;
            assert!((16.0..size.width - 16.).contains(&x));
            assert!((16.0..size.height - 16.).contains(&y));
        }
    }

    #[test]
    fn camera_fits_all_targets_and_preserves_dateline_seam() {
        let size = Size::new(1000., 500.);
        let routes = [
            route_at(&[(47.4979, 19.0402), (40.7128, -74.006)]),
            route_at(&[(47.4979, 19.0402), (35.0116, 135.7681)]),
            route_at(&[(30., 170.), (32., -165.)]),
        ];
        let (scale, offset) = route_fit(&routes, size);
        for route in &routes {
            for node in &route.nodes {
                let (x, y) = map::project(node.lat, node.lon).unwrap();
                assert!((0.0..size.width).contains(&(offset.0 + x * scale)));
                assert!((0.0..size.height).contains(&(offset.1 + y * scale)));
            }
        }
        let seam = projected_pieces(&routes[2].nodes[0], &routes[2].nodes[1]);
        assert_eq!(seam.len(), 2);
        assert!(seam[0].0.0 > 1500. && seam[1].1.0 < 500.);
    }

    #[test]
    fn camera_stays_stable_during_playback_and_handles_empty_or_coincident_routes() {
        let size = Size::new(800., 350.);
        assert_eq!(route_fit(&[], size), map::fit(size.width, size.height, 16.));
        let mut route = route_at(&[(47.5, 19.04), (47.5, 19.04)]);
        let first = route_fit(std::slice::from_ref(&route), size);
        route.progress = 0.82;
        assert_eq!(first, route_fit(std::slice::from_ref(&route), size));
        assert!(first.0.is_finite() && first.1.0.is_finite() && first.1.1.is_finite());
        let (x, y) = map::project(47.5, 19.04).unwrap();
        let cursor = Point::new(first.1.0 + x * first.0, first.1.1 + y * first.0);
        assert_eq!(
            hit_node(&[route], Some(1), cursor, first.0, first.1),
            Some((1, 0))
        );
    }

    #[test]
    fn shorter_jumps_finish_before_longer_jumps_and_bar_timing_matches() {
        let mut route = route_at(&[(47.4979, 19.0402), (50.1109, 8.6821), (40.7128, -74.006)]);
        let weights = segment_weights(&route.nodes);
        assert!(weights[1] > weights[0] * 5.);
        let first_end = weights[0] / weights.iter().sum::<f32>();
        route.progress = first_end * 0.5;
        assert!((route.segment_progress(0) - 0.5).abs() < 0.001);
        assert_eq!(route.segment_progress(1), 0.);
        assert!(!route.segment_completed(0));
        route.progress = first_end + 0.001;
        assert!(route.segment_completed(0));
        assert!(route.segment_progress(1) > 0. && route.segment_progress(1) < 0.01);
        route.progress = 1.;
        assert!(route.segment_completed(0) && route.segment_completed(1));
    }

    #[test]
    fn dateline_pieces_use_their_lengths_and_coincident_positions_remain_finite() {
        let route = route_at(&[(30., 179.), (30., -160.)]);
        let pieces = projected_pieces(&route.nodes[0], &route.nodes[1]);
        let weights: Vec<_> = pieces.iter().map(|&(a, b)| arc_length(a, b)).collect();
        assert!(weights[1] > weights[0] * 10.);
        assert_eq!(weighted_progress(0.5, &weights, 0), 1.);
        assert!(weighted_progress(0.5, &weights, 1) < 0.5);
        let mut same = route_at(&[(47.5, 19.04), (47.5, 19.04), (47.5, 19.04)]);
        same.progress = 0.25;
        assert_eq!(same.segment_progress(0), 0.5);
        assert_eq!(same.segment_progress(1), 0.);
        assert_eq!(arc_parameter((10., 20.), (10., 20.), 0.7), 0.7);
    }

    #[test]
    fn nearby_labels_avoid_each_other_and_stay_in_frame() {
        let bounds = Size::new(800., 400.);
        let first = label_rectangle(Point::new(430., 180.), Size::new(85., 25.), bounds, &[]);
        let second = label_rectangle(
            Point::new(440., 187.),
            Size::new(90., 25.),
            bounds,
            &[first],
        );
        assert_eq!(overlap_area(first, second), 0.);
        let edge = label_rectangle(
            Point::new(798., 398.),
            Size::new(140., 25.),
            bounds,
            &[first, second],
        );
        assert!(edge.x >= 4. && edge.y >= 4.);
        assert!(edge.x + edge.width <= 796. && edge.y + edge.height <= 396.);
    }

    #[test]
    fn overlapping_markers_prefer_selected_route_and_ignore_distant_clicks() {
        let routes = [3_u64, 8_u64]
            .into_iter()
            .map(|id| RouteVisual {
                id,
                label: format!("target-{id}"),
                progress: 0.5,
                nodes: vec![MapNode {
                    lat: 47.4979,
                    lon: 19.0402,
                    label: "Budapest".into(),
                    unknown_before: false,
                }],
            })
            .collect::<Vec<_>>();
        let (x, y) = map::project(47.4979, 19.0402).unwrap();
        let position = Point::new(x, y);
        assert_eq!(
            hit_node(&routes, Some(8), position, 1., (0., 0.)),
            Some((8, 0))
        );
        assert_eq!(
            hit_node(&routes, Some(3), position, 1., (0., 0.)),
            Some((3, 0))
        );
        assert_eq!(
            hit_node(&routes, Some(3), Point::new(x + 30., y), 1., (0., 0.)),
            None
        );
    }

    #[test]
    fn stationary_cursor_is_retested_after_route_snapshot_changes() {
        let mut route = RouteVisual {
            id: 3,
            label: "target".into(),
            progress: 0.5,
            nodes: vec![MapNode {
                lat: 47.4979,
                lon: 19.0402,
                label: "Budapest".into(),
                unknown_before: false,
            }],
        };
        let (x, y) = map::project(47.4979, 19.0402).unwrap();
        let cursor = Point::new(x, y);
        assert_eq!(
            hit_node(std::slice::from_ref(&route), Some(3), cursor, 1., (0., 0.)),
            Some((3, 0))
        );
        route.nodes[0].lat = 43.6532;
        route.nodes[0].lon = -79.3832;
        assert_eq!(hit_node(&[route], Some(3), cursor, 1., (0., 0.)), None);
    }

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
