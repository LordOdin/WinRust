//! Per-user theme shared by the WinRust programs. Open windows refresh on WinRustStyleChanged.
pub mod appearance;
use appearance::{Choice, Element};
use std::{
    cell::Cell,
    io,
    mem::size_of_val,
    ptr::null_mut,
    sync::atomic::{AtomicU32, Ordering::Relaxed},
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, PostMessageW, RegisterWindowMessageW};
use windows_sys::Win32::{Foundation::ERROR_SUCCESS, System::Registry::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Normal,
    Terminal, // monospace, green on black, square corners
    Dock,     // the bar is a floating pill in the middle
    Tiling,   // a thin status strip at the top, square corners, windows side by side
}

pub const ALL: [Style; 4] = [Style::Normal, Style::Terminal, Style::Dock, Style::Tiling];

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::Normal => "normal",
            Style::Terminal => "terminal",
            Style::Dock => "dock",
            Style::Tiling => "tiling",
        }
    }
    /// Unknown or empty text (no file yet, a hand edit) is the normal style.
    pub fn parse(s: &str) -> Style {
        ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim())).unwrap_or(Style::Normal)
    }
    /// Square corners everywhere.
    pub fn square(self) -> bool {
        matches!(self, Style::Terminal | Style::Tiling)
    }
    pub fn mono(self) -> bool {
        matches!(self, Style::Terminal | Style::Tiling)
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub selected: usize,
    pub base: usize,
    pub custom: [u32; 7],
}

fn colors(p: &Pal) -> [u32; 7] {
    [p.bg, p.line, p.hover, p.active, p.accent, p.text, p.dim]
}

impl Theme {
    fn read() -> Self {
        let mut data = [0u32; 9];
        let mut size = size_of_val(&data) as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows_sys::w!(r"Software\WinRust\Topbar"),
                windows_sys::w!("BarThemeV1"),
                RRF_RT_REG_BINARY,
                null_mut(),
                data.as_mut_ptr() as _,
                &mut size,
            )
        };
        if ok != ERROR_SUCCESS || size != 36 || data[0] > 4 || data[1] > 3 || data[2..].iter().any(|&c| c > 0xffffff) {
            let legacy = legacy_style();
            let base = ALL.iter().position(|&s| s == legacy).unwrap_or(0);
            return Self {
                selected: base,
                base,
                custom: colors(&PALS[base]),
            };
        }
        Self {
            selected: data[0] as usize,
            base: data[1] as usize,
            custom: data[2..].try_into().unwrap(),
        }
    }

    pub fn preset_colors(self) -> [u32; 7] { colors(&PALS[if self.selected == CUSTOM {self.base} else {self.selected}]) }

    pub fn colors(self) -> [u32; 7] {
        if self.selected == CUSTOM { self.custom } else { colors(&PALS[self.selected]) }
    }

    pub fn save(self) -> io::Result<()> {
        if self.selected > CUSTOM || self.base >= ALL.len() || self.custom.iter().any(|&color| color > 0xffffff) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid theme"));
        }
        let mut data = [0u32; 9];
        data[0] = self.selected as u32;
        data[1] = self.base as u32;
        data[2..].copy_from_slice(&self.custom);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                windows_sys::w!(r"Software\WinRust\Topbar"),
                windows_sys::w!("BarThemeV1"),
                REG_BINARY,
                data.as_ptr().cast(),
                size_of_val(&data) as u32,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        CACHE.set(Some((Instant::now(), self)));
        changed();
        Ok(())
    }
}

fn changed() {
    appearance::reload();
    unsafe { PostMessageW(HWND_BROADCAST, msg(), 0, 0) };
}

pub fn square() -> bool {
    match Choice::Corners.value() { 1 => false, 2 => true, _ => style().square() }
}

pub fn legacy_style() -> Style {
    let file = std::env::var_os("ProgramData").map(|p| std::path::PathBuf::from(p).join("WinRust").join("style.txt"));
    Style::parse(&file.and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default())
}

pub const CUSTOM: usize = ALL.len();
thread_local! { static CACHE: Cell<Option<(Instant, Theme)>> = const { Cell::new(None) }; }

pub fn reload() {
    CACHE.set(None);
    appearance::reload();
}

pub fn theme() -> Theme {
    if let Some((at, value)) = CACHE.get() {
        if at.elapsed() < Duration::from_secs(1) {
            return value;
        }
    }
    let value = Theme::read();
    CACHE.set(Some((Instant::now(), value)));
    value
}

pub fn style() -> Style {
    let t = theme();
    ALL[if t.selected == CUSTOM { t.base } else { t.selected }]
}

pub fn set_style(s: Style) {
    let _ = Theme {
        selected: ALL.iter().position(|&v| v == s).unwrap_or(0),
        ..theme()
    }
    .save();
}

/// The broadcast message a window gets when the style changed: call `reload()` and repaint.
pub fn msg() -> u32 {
    static M: AtomicU32 = AtomicU32::new(0);
    match M.load(Relaxed) {
        0 => {
            let n: Vec<u16> = "WinRustStyleChanged\0".encode_utf16().collect();
            let m = unsafe { RegisterWindowMessageW(n.as_ptr()) };
            M.store(m, Relaxed);
            m
        }
        m => m,
    }
}

/// Colours of one style, as COLORREF (0x00BBGGRR) like GDI wants them.
pub struct Pal {
    pub bg: u32,
    pub line: u32,
    pub hover: u32,
    pub active: u32,
    pub accent: u32,
    pub text: u32,
    pub dim: u32,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// In the order of `ALL`.
pub const PALS: [Pal; 4] = [
    // normal: Windows 11 blue
    Pal {
        bg: rgb(24, 30, 44),
        line: rgb(44, 52, 72),
        hover: rgb(40, 48, 68),
        active: rgb(52, 61, 84),
        accent: rgb(76, 194, 255),
        text: rgb(255, 255, 255),
        dim: rgb(150, 158, 175),
    },
    // terminal: green on black
    Pal {
        bg: rgb(10, 15, 10),
        line: rgb(40, 72, 40),
        hover: rgb(20, 40, 20),
        active: rgb(28, 56, 28),
        accent: rgb(91, 215, 91),
        text: rgb(207, 232, 207),
        dim: rgb(100, 145, 100),
    },
    // dock: graphite with violet
    Pal {
        bg: rgb(27, 29, 34),
        line: rgb(50, 53, 62),
        hover: rgb(42, 45, 53),
        active: rgb(52, 55, 66),
        accent: rgb(167, 139, 250),
        text: rgb(255, 255, 255),
        dim: rgb(154, 160, 170),
    },
    // tiling: dark with amber
    Pal {
        bg: rgb(17, 19, 23),
        line: rgb(40, 44, 52),
        hover: rgb(30, 34, 41),
        active: rgb(38, 43, 52),
        accent: rgb(229, 192, 123),
        text: rgb(215, 218, 224),
        dim: rgb(110, 117, 130),
    },
];

pub fn pal() -> Pal {
    let [bg, line, hover, active, accent, text, dim] = theme().colors();
    Pal {
        bg,
        line,
        hover,
        active,
        accent,
        text,
        dim,
    }
}

/// Apply the saved palette to a Win32 nonclient frame (unsupported DWM attributes are ignored).
pub unsafe fn apply_frame(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute};
    for (attr, color) in [(DWMWA_WINDOW_CORNER_PREFERENCE, if square() {1} else {2}), (DWMWA_CAPTION_COLOR, Element::FrameBg.color()), (DWMWA_TEXT_COLOR, Element::FrameText.color()), (DWMWA_BORDER_COLOR, Element::FrameLine.color())] {
        unsafe {
            DwmSetWindowAttribute(hwnd, attr as u32, (&color as *const u32).cast(), 4);
        }
    }
}

/// The UI font of the current style.
pub fn ui_face() -> &'static str {
    match Choice::Font.value() { 1 => "Segoe UI", 2 => "Consolas", _ if style().mono() => "Consolas", _ => "Segoe UI" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_junk_is_normal() {
        for s in ALL {
            assert_eq!(Style::parse(s.name()), s);
        }
        assert_eq!(Style::parse(" Tiling\r\n"), Style::Tiling);
        assert_eq!(Style::parse(""), Style::Normal);
        assert_eq!(Style::parse("neon"), Style::Normal);
    }
}
