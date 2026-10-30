//! Windows' OS preference is read independently from the window's paint theme.
//! Winit suppresses WM_SETTINGCHANGE theme events once Iced assigns a custom
//! window palette. Polling this small registry value preserves OS-only behavior.
#[cfg(windows)]
pub fn dark_preference() -> bool {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme".encode_utf16().chain(Some(0)).collect();
    let mut value = 0u32;
    let mut size = 4u32;
    let mut kind = 0u32;
    let status = unsafe {
        RegGetValueW(
            0x80000001u32 as i32 as isize,
            subkey.as_ptr(),
            name.as_ptr(),
            0x10,
            &mut kind,
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    status != 0 || size != 4 || value != 1
}
