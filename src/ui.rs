//! Shared visual language for the native interface.
use iced::mouse;
use iced::widget::canvas::{Frame, Geometry, Path, Stroke};
use iced::widget::{button, canvas, container};
use iced::{Color, Font, Point, Rectangle, Renderer, Theme};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub panel: Color,
    pub line: Color,
    pub text: Color,
    pub muted: Color,
    pub blue: Color,
    pub green: Color,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: Color::from_rgb8(9, 11, 15),
            panel: Color::from_rgb8(17, 21, 27),
            line: Color::from_rgb8(37, 42, 51),
            text: Color::from_rgb8(238, 241, 245),
            muted: Color::from_rgb8(147, 157, 169),
            blue: Color::from_rgb8(118, 184, 255),
            green: Color::from_rgb8(180, 244, 202),
        }
    } else {
        Palette {
            bg: Color::from_rgb8(238, 240, 243),
            panel: Color::from_rgb8(250, 251, 253),
            line: Color::from_rgb8(209, 214, 222),
            text: Color::from_rgb8(27, 36, 50),
            muted: Color::from_rgb8(101, 113, 133),
            blue: Color::from_rgb8(47, 110, 167),
            green: Color::from_rgb8(35, 134, 80),
        }
    }
}

fn alpha(color: Color, a: f32) -> Color {
    Color { a, ..color }
}

pub fn bold() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..Font::with_name("DejaVu Sans")
    }
}

pub fn panel(dark: bool) -> container::Style {
    let p = palette(dark);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.panel.into()),
        border: iced::Border {
            color: p.line,
            width: 1.0,
            radius: 13.0.into(),
        },
        ..Default::default()
    }
}

pub fn card(dark: bool) -> container::Style {
    let p = palette(dark);
    container::Style {
        text_color: Some(p.text),
        background: Some(
            if dark {
                Color::from_rgb8(22, 28, 36)
            } else {
                Color::WHITE
            }
            .into(),
        ),
        border: iced::Border {
            color: p.line,
            width: 1.0,
            radius: 9.0.into(),
        },
        ..Default::default()
    }
}

fn surface(top: Color, bottom: Color) -> iced::Background {
    iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, top)
        .add_stop(1.0, bottom)
        .into()
}

fn lift(dark: bool, status: button::Status) -> iced::Shadow {
    if matches!(status, button::Status::Pressed | button::Status::Disabled) {
        return iced::Shadow::default();
    }
    iced::Shadow {
        color: Color::from_rgba8(0, 0, 0, if dark { 0.2 } else { 0.06 }),
        offset: iced::Vector::new(0.0, 1.0),
        blur_radius: 2.0,
    }
}

pub fn primary(dark: bool, status: button::Status) -> button::Style {
    let disabled = status == button::Status::Disabled;
    let (top, bottom) = match status {
        button::Status::Hovered => (
            Color::from_rgb8(212, 255, 227),
            Color::from_rgb8(184, 248, 206),
        ),
        button::Status::Pressed => (
            Color::from_rgb8(157, 222, 179),
            Color::from_rgb8(172, 233, 192),
        ),
        _ => (
            Color::from_rgb8(192, 249, 212),
            Color::from_rgb8(170, 232, 192),
        ),
    };
    button::Style {
        background: Some(surface(top, bottom).scale_alpha(if disabled { 0.28 } else { 1.0 })),
        text_color: if disabled {
            palette(dark).muted
        } else {
            Color::from_rgb8(12, 36, 24)
        },
        border: iced::Border {
            color: alpha(
                if dark {
                    top
                } else {
                    Color::from_rgb8(138, 198, 159)
                },
                if disabled { 0.3 } else { 1.0 },
            ),
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: lift(dark, status),
        ..Default::default()
    }
}

pub fn quiet(dark: bool, status: button::Status) -> button::Style {
    let p = palette(dark);
    let disabled = status == button::Status::Disabled;
    let (top, bottom) = match (dark, status) {
        (true, button::Status::Hovered) => {
            (Color::from_rgb8(38, 47, 60), Color::from_rgb8(27, 34, 45))
        }
        (true, button::Status::Pressed) => {
            (Color::from_rgb8(16, 22, 31), Color::from_rgb8(24, 31, 42))
        }
        (true, _) => (Color::from_rgb8(25, 31, 40), Color::from_rgb8(18, 23, 31)),
        (false, button::Status::Hovered) => (
            Color::from_rgb8(255, 255, 255),
            Color::from_rgb8(231, 239, 247),
        ),
        (false, button::Status::Pressed) => (
            Color::from_rgb8(219, 230, 241),
            Color::from_rgb8(234, 241, 248),
        ),
        (false, _) => (Color::WHITE, Color::from_rgb8(240, 244, 249)),
    };
    button::Style {
        background: Some(surface(top, bottom).scale_alpha(if disabled { 0.35 } else { 1.0 })),
        text_color: if disabled {
            alpha(p.muted, 0.55)
        } else {
            p.text
        },
        border: iced::Border {
            color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                alpha(p.blue, 0.65)
            } else if disabled {
                alpha(p.line, 0.6)
            } else if dark {
                Color::from_rgb8(48, 58, 71)
            } else {
                Color::from_rgb8(190, 202, 215)
            },
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: lift(dark, status),
        ..Default::default()
    }
}

pub fn selected(dark: bool, status: button::Status) -> button::Style {
    let p = palette(dark);
    let mut style = quiet(dark, status);
    if status == button::Status::Disabled {
        return style;
    }
    let pressed = status == button::Status::Pressed;
    let hovered = status == button::Status::Hovered;
    let (top, bottom) = if dark {
        (
            Color::from_rgb8(30, if hovered { 65 } else { 54 }, 45),
            Color::from_rgb8(19, if pressed { 35 } else { 42 }, 34),
        )
    } else {
        (
            Color::from_rgb8(237, 252, 241),
            Color::from_rgb8(211, if pressed { 230 } else { 239 }, 220),
        )
    };
    style.background = Some(surface(top, bottom));
    style.border.color = alpha(p.green, if hovered { 0.8 } else { 0.55 });
    style.text_color = p.green;
    style
}

pub fn stop(dark: bool, status: button::Status) -> button::Style {
    let mut style = quiet(dark, status);
    let hovered = status == button::Status::Hovered;
    style.background = Some(if dark {
        surface(
            Color::from_rgb8(if hovered { 91 } else { 73 }, 36, 43),
            Color::from_rgb8(53, 25, 32),
        )
    } else {
        surface(
            Color::from_rgb8(255, 241, 242),
            Color::from_rgb8(250, 222, 226),
        )
    });
    style.text_color = if dark {
        Color::from_rgb8(255, 184, 190)
    } else {
        Color::from_rgb8(148, 47, 60)
    };
    style.border.color = alpha(style.text_color, 0.5);
    style
}

#[derive(Clone, Copy)]
pub enum IconKind {
    History,
    Info,
    Export,
    Settings,
    Focus,
    Details,
    Play,
    Stop,
    Pause,
    Replay,
    Targets,
}

/// Small native vector controls stay crisp at every operating-system scale.
pub struct ActionIcon {
    pub kind: IconKind,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for ActionIcon {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.scale(bounds.width.min(bounds.height) / 20.0);
        let stroke = Stroke::default()
            .with_width(1.55)
            .with_color(self.color)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        let line = |frame: &mut Frame, points: &[(f32, f32)]| {
            frame.stroke(
                &Path::new(|path| {
                    for (index, &(x, y)) in points.iter().enumerate() {
                        if index == 0 {
                            path.move_to(Point::new(x, y));
                        } else {
                            path.line_to(Point::new(x, y));
                        }
                    }
                }),
                stroke,
            );
        };
        match self.kind {
            IconKind::Play => frame.fill(
                &Path::new(|path| {
                    path.move_to(Point::new(6., 3.5));
                    path.line_to(Point::new(16., 10.));
                    path.line_to(Point::new(6., 16.5));
                    path.close();
                }),
                self.color,
            ),
            IconKind::Stop => frame.fill(
                &Path::rectangle(Point::new(5., 5.), iced::Size::new(10., 10.)),
                self.color,
            ),
            IconKind::Pause => {
                frame.fill(
                    &Path::rectangle(Point::new(5., 4.), iced::Size::new(3.5, 12.)),
                    self.color,
                );
                frame.fill(
                    &Path::rectangle(Point::new(11.5, 4.), iced::Size::new(3.5, 12.)),
                    self.color,
                );
            }
            IconKind::History | IconKind::Replay => {
                let arc = Path::new(|path| {
                    for i in 0..=32 {
                        let angle = -2.5 + i as f32 / 32. * 5.5;
                        let p = Point::new(10. + 6.7 * angle.cos(), 10. + 6.7 * angle.sin());
                        if i == 0 {
                            path.move_to(p);
                        } else {
                            path.line_to(p);
                        }
                    }
                });
                frame.stroke(&arc, stroke);
                line(&mut frame, &[(3.8, 2.2), (4.5, 6.2), (8.5, 5.6)]);
                if matches!(self.kind, IconKind::History) {
                    line(&mut frame, &[(10., 6.), (10., 10.), (13., 12.)]);
                }
            }
            IconKind::Info => {
                frame.stroke(&Path::circle(Point::new(10., 10.), 7.), stroke);
                frame.fill(&Path::circle(Point::new(10., 6.), 0.9), self.color);
                line(&mut frame, &[(10., 9.), (10., 14.)]);
            }
            IconKind::Export => {
                line(&mut frame, &[(3., 12.), (3., 17.), (17., 17.), (17., 12.)]);
                line(&mut frame, &[(10., 2.), (10., 12.)]);
                line(&mut frame, &[(6.5, 8.5), (10., 12.), (13.5, 8.5)]);
            }
            IconKind::Settings => {
                for (y, x) in [(5., 7.), (10., 13.), (15., 8.)] {
                    line(&mut frame, &[(3., y), (x - 2., y)]);
                    line(&mut frame, &[(x + 2., y), (17., y)]);
                    frame.stroke(&Path::circle(Point::new(x, y), 2.), stroke);
                }
            }
            IconKind::Focus => {
                for points in [
                    [(3., 8.), (3., 3.), (8., 3.)],
                    [(12., 3.), (17., 3.), (17., 8.)],
                    [(17., 12.), (17., 17.), (12., 17.)],
                    [(8., 17.), (3., 17.), (3., 12.)],
                ] {
                    line(&mut frame, &points);
                }
            }
            IconKind::Details => {
                frame.stroke(
                    &Path::rectangle(Point::new(3., 3.), iced::Size::new(14., 14.)),
                    stroke,
                );
                line(&mut frame, &[(11., 3.), (11., 17.)]);
            }
            IconKind::Targets => {
                for (y, x) in [(5., 6.), (10., 6.), (15., 6.)] {
                    frame.fill(&Path::circle(Point::new(3., y), 1.2), self.color);
                    line(&mut frame, &[(x, y), (17., y)]);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Vector globe identity: deliberately independent of the map geometry.
pub struct BrandMark {
    pub dark: bool,
}

impl<Message> canvas::Program<Message> for BrandMark {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = palette(self.dark);
        let diameter = bounds.width.min(bounds.height);
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = diameter * 0.39;
        let stroke = |width, tint| Stroke::default().with_width(width).with_color(tint);
        frame.stroke(&Path::circle(center, radius), stroke(1.25, p.blue));
        let meridian = Path::new(|path| {
            for i in 0..=64 {
                let t = i as f32 / 64.0 * std::f32::consts::TAU;
                let point = Point::new(
                    center.x + radius * 0.47 * t.cos(),
                    center.y + radius * t.sin(),
                );
                if i == 0 {
                    path.move_to(point)
                } else {
                    path.line_to(point)
                }
            }
        });
        frame.stroke(&meridian, stroke(0.8, alpha(p.blue, 0.6)));
        for vertical in [-0.55_f32, 0.0, 0.55] {
            let half = radius * (1.0 - vertical * vertical).sqrt();
            let y = center.y + vertical * radius;
            let line = Path::line(
                Point::new(center.x - half, y),
                Point::new(center.x + half, y),
            );
            frame.stroke(&line, stroke(0.8, alpha(p.blue, 0.5)));
        }
        let route = Path::new(|path| {
            let start = Point::new(center.x - radius * 0.84, center.y + radius * 0.39);
            let control = Point::new(center.x - radius * 0.05, center.y - radius * 1.05);
            let end = Point::new(center.x + radius * 0.9, center.y - radius * 0.32);
            path.move_to(start);
            for i in 1..=28 {
                let t = i as f32 / 28.0;
                let k = 1.0 - t;
                path.line_to(Point::new(
                    k * k * start.x + 2.0 * k * t * control.x + t * t * end.x,
                    k * k * start.y + 2.0 * k * t * control.y + t * t * end.y,
                ));
            }
        });
        frame.stroke(&route, stroke(2.0, p.green));
        frame.fill(
            &Path::circle(
                Point::new(center.x + radius * 0.9, center.y - radius * 0.32),
                2.1,
            ),
            p.green,
        );
        vec![frame.into_geometry()]
    }
}

/// Latest endpoint observations. Missing replies break the line; they are never
/// interpolated or replaced with zero-millisecond measurements.
pub struct Sparkline {
    pub samples: Vec<Option<f64>>,
    pub dark: bool,
}

impl<Message> canvas::Program<Message> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = palette(self.dark);
        let w = bounds.width;
        let h = bounds.height;
        if w < 4.0 || h < 4.0 {
            return vec![frame.into_geometry()];
        }
        let left = 1.0;
        let right = w - 1.0;
        let top = 5.0;
        let bottom = (h - 5.0).max(top + 1.0);
        for ratio in [0.0_f32, 0.5, 1.0] {
            let y = top + (bottom - top) * ratio;
            frame.stroke(
                &Path::line(Point::new(left, y), Point::new(right, y)),
                Stroke::default()
                    .with_width(0.5)
                    .with_color(alpha(p.line, 0.8)),
            );
        }
        let samples = &self.samples[self.samples.len().saturating_sub(60)..];
        let values: Vec<f64> = samples
            .iter()
            .flatten()
            .copied()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .collect();
        if values.is_empty() {
            return vec![frame.into_geometry()];
        }
        let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // A stable padded vertical range keeps constant and near-constant RTTs
        // readable without claiming that the graph starts at zero.
        let padding = ((maximum - minimum) * 0.15).max(0.5);
        let low = (minimum - padding).max(0.0);
        let high = maximum + padding;
        let range = (high - low).max(0.5);
        let denominator = samples.len().saturating_sub(1).max(1) as f32;
        let position = |index: usize, value: f64| {
            Point::new(
                left + (right - left) * index as f32 / denominator,
                bottom - ((value - low) / range) as f32 * (bottom - top),
            )
        };
        let line = Path::new(|path| {
            let mut continuing = false;
            for (index, value) in samples.iter().enumerate() {
                if let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) {
                    let point = position(index, value);
                    if continuing {
                        path.line_to(point)
                    } else {
                        path.move_to(point)
                    }
                    continuing = true;
                } else {
                    continuing = false;
                }
            }
        });
        frame.stroke(&line, Stroke::default().with_width(1.7).with_color(p.green));
        // Individual points also make isolated replies adjacent to gaps visible.
        for (index, value) in samples.iter().enumerate() {
            if let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) {
                frame.fill(&Path::circle(position(index, value), 1.4), p.green);
            }
        }
        if let Some(Some(value)) = samples.last() {
            if value.is_finite() && *value >= 0.0 {
                let point = position(samples.len() - 1, *value);
                frame.fill(&Path::circle(point, 4.0), alpha(p.green, 0.13));
                frame.fill(&Path::circle(point, 2.2), p.green);
            }
        }
        vec![frame.into_geometry()]
    }
}
