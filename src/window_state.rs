//! Normal window bounds are independent of the current (possibly maximized) viewport.
use crate::storage::{Config, WindowPosition};
#[cfg(not(windows))]
use iced::Point;
use iced::{Size, Task, window};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod x11;

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub size: Size,
    pub position: Option<WindowPosition>,
    pub maximized: bool,
    pub minimized: bool,
    /// Native restore bounds remain available even while maximized/minimized.
    pub normal_bounds: Option<(Size, WindowPosition)>,
    pub restore_maximized: Option<bool>,
}

pub struct State {
    size: Size,
    position: Option<WindowPosition>,
    maximized: bool,
    revision: u64,
    changed_at: Option<Instant>,
    pending: bool,
    restoring: bool,
    closing: bool,
}

impl State {
    pub fn new(config: &Config) -> Self {
        Self {
            size: Size::new(config.window_width, config.window_height),
            position: config.window_position,
            maximized: config.window_maximized,
            revision: 0,
            changed_at: None,
            pending: false,
            restoring: true,
            closing: false,
        }
    }
    pub fn changed(&mut self) {
        if !self.closing {
            self.revision = self.revision.wrapping_add(1);
            self.changed_at = Some(Instant::now());
        }
    }
    pub fn restored(&mut self) {
        self.restoring = false;
        self.changed();
    }
    pub fn capture_due(&mut self, now: Instant) -> Option<u64> {
        if !self.restoring
            && !self.closing
            && !self.pending
            && self
                .changed_at
                .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_millis(250))
        {
            self.pending = true;
            Some(self.revision)
        } else {
            None
        }
    }
    pub fn close(&mut self) -> Option<u64> {
        if self.closing {
            return None;
        }
        self.closing = true;
        self.revision = self.revision.wrapping_add(1);
        Some(self.revision)
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn accept(&mut self, revision: u64, snapshot: Snapshot) -> bool {
        self.pending = false;
        if revision != self.revision {
            return false;
        }
        self.changed_at = None;
        // Native restore bounds capture a last-minute resize followed immediately
        // by maximize/minimize, before the debounced normal capture could run.
        if let Some((size, position)) = snapshot.normal_bounds
            && valid_size(size)
            && position.valid()
        {
            self.size = size;
            self.position = Some(position);
        }
        if let Some(maximized) = snapshot.restore_maximized {
            self.maximized = maximized;
        }
        // X11 can retain both maximized and minimized flags. This is positive
        // evidence even when maximize was followed by minimize before our debounce.
        // A false flag while minimized is ambiguous, so never use it to clear state.
        if snapshot.maximized {
            self.maximized = true;
        }
        // Portable minimize queries can report bogus bounds or clear maximized.
        // Keep the last confirmed visible state when no native restore state exists.
        if !snapshot.minimized {
            self.maximized = snapshot.maximized;
            if !snapshot.maximized && valid_size(snapshot.size) {
                self.size = snapshot.size;
                if snapshot.position.is_some_and(|p| p.valid()) {
                    self.position = snapshot.position;
                }
            }
        }
        true
    }
    pub fn saved_config(&self, current: &Config) -> Config {
        let mut saved = current.clone();
        saved.window_width = self.size.width;
        saved.window_height = self.size.height;
        saved.window_position = self.position;
        saved.window_maximized = self.maximized;
        saved
    }
}

fn valid_size(size: Size) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width >= 680.0 && size.height >= 540.0
}

pub fn initial_position(config: &Config) -> window::Position {
    // On Windows, restore physical desktop coordinates after the native window exists.
    // This avoids treating a secondary monitor's physical position as primary-monitor DIPs.
    #[cfg(windows)]
    {
        let _ = config;
        window::Position::Default
    }
    #[cfg(not(windows))]
    {
        config
            .window_position
            .filter(|p| p.valid())
            .map_or(window::Position::Default, |p| {
                window::Position::Specific(Point::new(
                    p.x as f32 / p.scale_factor,
                    p.y as f32 / p.scale_factor,
                ))
            })
    }
}

pub fn restore(id: window::Id, config: &Config) -> Task<()> {
    let maximized = config.window_maximized;
    #[cfg(windows)]
    let place = {
        let position = config.window_position;
        let size = Size::new(config.window_width, config.window_height);
        window::run(id, move |handle| native::restore(handle, position, size))
    };
    #[cfg(target_os = "linux")]
    let place = {
        let position = config.window_position;
        let size = Size::new(config.window_width, config.window_height);
        window::run(id, x11::screen_size).then(move |screen| match (screen, position) {
            (Some(screen), Some(position)) => window::scale_factor(id).then(move |scale| {
                match x11::recover_position(position, size, screen, scale) {
                    Some(point) => window::move_to(id, point).chain(Task::done(())),
                    None => Task::done(()),
                }
            }),
            _ => Task::done(()),
        })
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let place = Task::done(());
    place.then(move |_| window::maximize(id, maximized).chain(Task::done(())))
}

pub fn capture(id: window::Id) -> Task<Snapshot> {
    #[cfg(windows)]
    {
        window::run(id, native::capture).then(move |value| match value {
            Some(snapshot) => Task::done(snapshot),
            None => portable_capture(id),
        })
    }
    #[cfg(not(windows))]
    {
        portable_capture(id)
    }
}

fn portable_capture(id: window::Id) -> Task<Snapshot> {
    // A revision guard rejects results spanning a move/resize event. Windows uses a
    // single native callback instead, so minimized/maximized geometry is atomic.
    window::is_maximized(id).then(move |maximized| {
        window::is_minimized(id).then(move |minimized| {
            window::scale_factor(id).then(move |scale_factor| {
                window::position(id).then(move |position| {
                    window::size(id).map(move |size| Snapshot {
                        size,
                        position: position.map(|p| WindowPosition {
                            x: (p.x * scale_factor).round() as i32,
                            y: (p.y * scale_factor).round() as i32,
                            scale_factor,
                        }),
                        maximized,
                        minimized: minimized.unwrap_or(false),
                        normal_bounds: None,
                        restore_maximized: None,
                    })
                })
            })
        })
    })
}

#[cfg(any(windows, test))]
fn normal_bounds(
    workspace: (i32, i32, i32, i32),
    workspace_offset: (i32, i32),
    frame: (i32, i32),
    scale_factor: f32,
) -> (Size, WindowPosition) {
    (
        Size::new(
            (workspace.2 - workspace.0 - frame.0) as f32 / scale_factor,
            (workspace.3 - workspace.1 - frame.1) as f32 / scale_factor,
        ),
        WindowPosition {
            x: workspace.0.saturating_add(workspace_offset.0),
            y: workspace.1.saturating_add(workspace_offset.1),
            scale_factor,
        },
    )
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::c_void;
    type Handle = *mut c_void;
    const WS_MAXIMIZE: u32 = 0x0100_0000;
    const WS_MINIMIZE: u32 = 0x2000_0000;
    const SW_SHOWMAXIMIZED: u32 = 3;
    const WPF_RESTORETOMAXIMIZED: u32 = 2;
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct NativePoint {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Placement {
        length: u32,
        flags: u32,
        show_cmd: u32,
        min_position: NativePoint,
        max_position: NativePoint,
        normal_position: Rect,
    }
    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowRect(window: Handle, rect: *mut Rect) -> i32;
        fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
        fn GetWindowPlacement(window: Handle, placement: *mut Placement) -> i32;
        fn GetDpiForWindow(window: Handle) -> u32;
        fn GetWindowLongW(window: Handle, index: i32) -> i32;
        fn GetMenu(window: Handle) -> Handle;
        fn AdjustWindowRectExForDpi(
            rect: *mut Rect,
            style: u32,
            menu: i32,
            ex_style: u32,
            dpi: u32,
        ) -> i32;
        fn IsZoomed(window: Handle) -> i32;
        fn IsIconic(window: Handle) -> i32;
        fn MonitorFromRect(rect: *const Rect, flags: u32) -> Handle;
        fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
        fn SetWindowPos(
            window: Handle,
            after: Handle,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
    }
    fn hwnd(handle: &dyn window::Window) -> Option<Handle> {
        use window::raw_window_handle::RawWindowHandle;
        match handle.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(value) => Some(value.hwnd.get() as Handle),
            _ => None,
        }
    }
    fn monitor_info(rect: &Rect) -> Option<MonitorInfo> {
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        unsafe {
            let monitor = MonitorFromRect(rect, 2);
            (!monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0).then_some(info)
        }
    }
    fn normal_frame(hwnd: Handle, dpi: u32) -> Option<(i32, i32)> {
        let mut frame = Rect::default();
        unsafe {
            let style = GetWindowLongW(hwnd, -16) as u32 & !(WS_MAXIMIZE | WS_MINIMIZE);
            let ex_style = GetWindowLongW(hwnd, -20) as u32;
            (AdjustWindowRectExForDpi(
                &mut frame,
                style,
                i32::from(!GetMenu(hwnd).is_null()),
                ex_style,
                dpi,
            ) != 0)
                .then_some((frame.right - frame.left, frame.bottom - frame.top))
        }
    }
    pub fn capture(handle: &dyn window::Window) -> Option<Snapshot> {
        let hwnd = hwnd(handle)?;
        let mut outer = Rect::default();
        let mut client = Rect::default();
        // All calls run on the native UI thread while the window handle is borrowed.
        unsafe {
            if GetWindowRect(hwnd, &mut outer) == 0 || GetClientRect(hwnd, &mut client) == 0 {
                return None;
            }
            let dpi = GetDpiForWindow(hwnd).max(96);
            let scale_factor = dpi as f32 / 96.0;
            let maximized = IsZoomed(hwnd) != 0;
            let minimized = IsIconic(hwnd) != 0;
            let mut snapshot = Snapshot {
                size: Size::new(
                    (client.right - client.left) as f32 / scale_factor,
                    (client.bottom - client.top) as f32 / scale_factor,
                ),
                position: Some(WindowPosition {
                    x: outer.left,
                    y: outer.top,
                    scale_factor,
                }),
                maximized,
                minimized,
                normal_bounds: None,
                restore_maximized: None,
            };
            if maximized || minimized {
                let mut placement = Placement {
                    length: std::mem::size_of::<Placement>() as u32,
                    flags: 0,
                    show_cmd: 0,
                    min_position: NativePoint::default(),
                    max_position: NativePoint::default(),
                    normal_position: Rect::default(),
                };
                if GetWindowPlacement(hwnd, &mut placement) != 0 {
                    // A missing restore flag is not evidence of a normal window:
                    // some Windows versions return zero flags while minimized.
                    snapshot.restore_maximized = (placement.show_cmd == SW_SHOWMAXIMIZED
                        || placement.flags & WPF_RESTORETOMAXIMIZED != 0
                        || maximized)
                        .then_some(true);
                    let rect = placement.normal_position;
                    if let (Some(info), Some(frame)) =
                        (monitor_info(&rect), normal_frame(hwnd, dpi))
                    {
                        // WINDOWPLACEMENT is in workspace coordinates; SetWindowPos
                        // and our saved position use physical screen coordinates.
                        snapshot.normal_bounds = Some(normal_bounds(
                            (rect.left, rect.top, rect.right, rect.bottom),
                            (
                                info.work.left - info.monitor.left,
                                info.work.top - info.monitor.top,
                            ),
                            frame,
                            scale_factor,
                        ));
                    }
                }
            }
            Some(snapshot)
        }
    }
    pub fn restore(handle: &dyn window::Window, position: Option<WindowPosition>, size: Size) {
        let (Some(hwnd), Some(position)) = (hwnd(handle), position.filter(|p| p.valid())) else {
            return;
        };
        let proposed = Rect {
            left: position.x,
            top: position.y,
            right: position
                .x
                .saturating_add((size.width * position.scale_factor).round() as i32),
            bottom: position
                .y
                .saturating_add((size.height * position.scale_factor).round() as i32),
        };
        let Some(info) = monitor_info(&proposed) else {
            return;
        };
        unsafe {
            // Move onto the target monitor first. WM_DPICHANGED is synchronous;
            // its new DPI determines the saved logical client size, rather than
            // retaining the primary monitor's initial pixel dimensions.
            if SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                info.work.left,
                info.work.top,
                0,
                0,
                0x0001 | 0x0004 | 0x0008 | 0x0010,
            ) == 0
            {
                return;
            }
            let dpi = GetDpiForWindow(hwnd).max(96);
            let scale_factor = dpi as f32 / 96.0;
            let Some(frame) = normal_frame(hwnd, dpi) else {
                return;
            };
            let width = ((size.width * scale_factor).round() as i32 + frame.0)
                .min(info.work.right - info.work.left)
                .max(1);
            let height = ((size.height * scale_factor).round() as i32 + frame.1)
                .min(info.work.bottom - info.work.top)
                .max(1);
            let x = position.x.clamp(
                info.work.left,
                info.work.right.saturating_sub(width).max(info.work.left),
            );
            let y = position.y.clamp(
                info.work.top,
                info.work.bottom.saturating_sub(height).max(info.work.top),
            );
            let _ = SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                width,
                height,
                0x0004 | 0x0010,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn normal(width: f32, height: f32, x: i32, y: i32) -> Snapshot {
        Snapshot {
            size: Size::new(width, height),
            position: Some(WindowPosition {
                x,
                y,
                scale_factor: 1.5,
            }),
            maximized: false,
            minimized: false,
            normal_bounds: None,
            restore_maximized: None,
        }
    }
    #[test]
    fn maximized_and_minimized_bounds_never_replace_normal_geometry() {
        let config = Config::default();
        let mut state = State::new(&config);
        assert!(state.accept(0, normal(1000., 700., -1400, 80)));
        let mut max = normal(2560., 1440., -2560, 0);
        max.maximized = true;
        assert!(state.accept(0, max));
        let mut min = normal(0., 0., -32000, -32000);
        min.minimized = true;
        assert!(state.accept(0, min));
        let saved = state.saved_config(&config);
        assert_eq!((saved.window_width, saved.window_height), (1000., 700.));
        assert_eq!(saved.window_position.unwrap().x, -1400);
        assert!(saved.window_maximized);
    }
    #[test]
    fn minimized_capture_confirms_maximize_before_debounce_without_saving_its_size() {
        let config = Config::default();
        let mut state = State::new(&config);
        state.accept(0, normal(1120., 710., 233, 157));
        // X11 reports both flags after rapid maximize -> minimize -> close.
        let mut minimized = normal(1920., 1261., 0, 0);
        minimized.maximized = true;
        minimized.minimized = true;
        state.accept(0, minimized);
        let saved = state.saved_config(&config);
        assert!(saved.window_maximized);
        assert_eq!((saved.window_width, saved.window_height), (1120., 710.));
        assert_eq!(
            (
                saved.window_position.unwrap().x,
                saved.window_position.unwrap().y
            ),
            (233, 157)
        );
    }
    #[test]
    fn live_layout_dimensions_cannot_leak_into_saved_normal_bounds() {
        let mut config = Config::default();
        let state = State::new(&config);
        config.window_width = 3840.;
        config.window_height = 2160.;
        config.history_enabled = true;
        let saved = state.saved_config(&config);
        assert_eq!((saved.window_width, saved.window_height), (1280., 800.));
        assert!(saved.history_enabled);
    }
    #[test]
    fn stale_capture_cannot_overwrite_newer_geometry() {
        let mut state = State::new(&Config::default());
        state.restored();
        let revision = state
            .capture_due(Instant::now() + Duration::from_secs(1))
            .unwrap();
        state.changed();
        assert!(!state.accept(revision, normal(900., 600., 10, 20)));
        assert_eq!(state.saved_config(&Config::default()).window_width, 1280.);
        let revision = state.close().unwrap();
        assert!(state.close().is_none());
        assert!(state.accept(revision, normal(1100., 720., 50, 70)));
        assert_eq!(state.saved_config(&Config::default()).window_width, 1100.);
    }
    #[test]
    fn native_restore_bounds_preserve_rapid_resize_then_maximize_and_minimize() {
        let config = Config::default();
        let mut state = State::new(&config);
        let latest = normal(1100., 720., -1450, 90);
        let mut max = normal(2560., 1440., -2560, 0);
        max.maximized = true;
        max.normal_bounds = Some((latest.size, latest.position.unwrap()));
        max.restore_maximized = Some(true);
        state.accept(0, max);
        let saved = state.saved_config(&config);
        assert_eq!((saved.window_width, saved.window_height), (1100., 720.));
        assert_eq!(saved.window_position.unwrap().x, -1450);
        assert!(saved.window_maximized);
        let mut min = max;
        min.maximized = false;
        min.minimized = true;
        state.accept(0, min);
        assert!(state.saved_config(&config).window_maximized);
        min.restore_maximized = None;
        state.accept(0, min);
        assert!(state.saved_config(&config).window_maximized);
        state.accept(0, latest);
        assert!(!state.saved_config(&config).window_maximized);
    }
    #[test]
    fn native_normal_bounds_convert_top_left_workbars_without_dpi_drift() {
        let (size, position) = normal_bounds((80, 50, 1604, 1168), (60, 40), (24, 68), 1.5);
        assert_eq!(size, Size::new(1000., 700.));
        assert_eq!((position.x, position.y), (140, 90));
        let (size, position) = normal_bounds((-2000, 20, -476, 1138), (0, 40), (24, 68), 1.5);
        assert_eq!(size, Size::new(1000., 700.));
        assert_eq!((position.x, position.y), (-2000, 60));
    }
}
