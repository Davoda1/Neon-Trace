//! Recover a saved X11 position after a display disappears. Wayland owns placement.
use crate::storage::WindowPosition;
use iced::{Point, Size, window};
use std::ffi::c_void;

pub fn screen_size(handle: &dyn window::Window) -> Option<Size> {
    use window::raw_window_handle::RawDisplayHandle;
    let RawDisplayHandle::Xlib(display) = handle.display_handle().ok()?.as_raw() else {
        return None;
    };
    let connection = display.display?.as_ptr();
    // Xlib is already used by the backend; resolve its query functions without
    // introducing an unconditional X11 link for Wayland-only installations.
    unsafe {
        let library = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if library.is_null() {
            return None;
        }
        let width_fn = libc::dlsym(library, c"XDisplayWidth".as_ptr());
        let height_fn = libc::dlsym(library, c"XDisplayHeight".as_ptr());
        if width_fn.is_null() || height_fn.is_null() {
            libc::dlclose(library);
            return None;
        }
        type Dimension = unsafe extern "C" fn(*mut c_void, i32) -> i32;
        let width: Dimension = std::mem::transmute(width_fn);
        let height: Dimension = std::mem::transmute(height_fn);
        let size = Size::new(
            width(connection, display.screen) as f32,
            height(connection, display.screen) as f32,
        );
        libc::dlclose(library);
        (size.width > 0.0 && size.height > 0.0).then_some(size)
    }
}

pub fn recover_position(
    position: WindowPosition,
    size: Size,
    screen: Size,
    scale: f32,
) -> Option<Point> {
    if !position.valid() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let x = position.x as f32;
    let y = position.y as f32;
    // X11 root coordinates cover the whole virtual desktop, not just its first
    // monitor. Leave attached secondary displays and partly visible windows alone.
    let title_visible =
        x + 64.0 >= 0.0 && x < screen.width - 32.0 && y >= 0.0 && y < screen.height - 32.0;
    (!title_visible).then(|| {
        Point::new(
            ((screen.width / scale - size.width) / 2.0).max(0.0),
            ((screen.height / scale - size.height) / 2.0).max(0.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_x11_display_recovers_but_attached_secondary_is_kept() {
        let size = Size::new(1000., 700.);
        let screen = Size::new(3840., 1080.);
        let position = WindowPosition {
            x: 2100,
            y: 80,
            scale_factor: 1.,
        };
        assert!(recover_position(position, size, screen, 1.).is_none());
        let missing = WindowPosition {
            x: 9000,
            ..position
        };
        assert_eq!(
            recover_position(missing, size, Size::new(1920., 1080.), 1.),
            Some(Point::new(460., 190.))
        );
    }
}
