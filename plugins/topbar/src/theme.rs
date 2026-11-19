use super::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::{Controls::Dialogs::*, Controls::*};
use winrust_common::style::{self as legacy, Pal};

const KEY: &str = r"Software\WinRust\Topbar";
const CUSTOM: usize = 4;
pub(super) const LIST: i32 = 129;
pub(super) const PRESET: i32 = 128;
pub(super) const EDIT: i32 = 130;

#[derive(Clone, Copy)]
struct Theme {
    selected: usize,
    base: usize,
    custom: [u32; 7],
}

fn colors(p: &Pal) -> [u32; 7] {
    [p.bg, p.line, p.hover, p.active, p.accent, p.text, p.dim]
}

impl Theme {
    fn read() -> Self {
        let base = legacy::ALL.iter().position(|s| *s == legacy::style()).unwrap_or(0);
        let fallback = Self {
            selected: base,
            base,
            custom: colors(&legacy::PALS[base]),
        };
        let mut data = [0u32; 9];
        let mut size = size_of_val(&data) as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide("BarThemeV1").as_ptr(),
                RRF_RT_REG_BINARY,
                null_mut(),
                data.as_mut_ptr() as _,
                &mut size,
            )
        };
        if ok != ERROR_SUCCESS || size != 36 || data[0] > 4 || data[1] > 3 || data[2..].iter().any(|&c| c > 0xffffff) {
            return fallback;
        }
        Self {
            selected: data[0] as usize,
            base: data[1] as usize,
            custom: data[2..].try_into().unwrap(),
        }
    }

    fn colors(self) -> [u32; 7] {
        if self.selected == CUSTOM {
            self.custom
        } else {
            colors(&legacy::PALS[self.selected])
        }
    }

    unsafe fn save(self, dlg: HWND) {
        let mut data = [0u32; 9];
        data[0] = self.selected as u32;
        data[1] = self.base as u32;
        data[2..].copy_from_slice(&self.custom);
        let err = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            wide("BarThemeV1").as_ptr(),
            REG_BINARY,
            data.as_ptr() as _,
            size_of_val(&data) as u32,
        );
        if err == ERROR_SUCCESS {
            THEME.set(self);
            InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0);
        } else {
            MessageBoxW(
                dlg,
                wide(&std::io::Error::from_raw_os_error(err as i32).to_string()).as_ptr(),
                wide(t!("Ошибка сохранения", "Could not save theme")).as_ptr(),
                MB_ICONERROR,
            );
        }
        sync(dlg);
    }
}

thread_local! { static THEME:Cell<Theme>=Cell::new(Theme::read()); }
pub(super) fn pal() -> Pal {
    let [bg, line, hover, active, accent, text, dim] = THEME.get().colors();
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
pub(super) fn square() -> bool {
    let t = THEME.get();
    legacy::ALL[if t.selected == CUSTOM { t.base } else { t.selected }].square()
}
pub(super) fn ui_face() -> &'static str {
    if square() { "Consolas" } else { "Segoe UI" }
}

fn names() -> [&'static str; 7] {
    [
        t!("Фон", "Background"),
        t!("Граница", "Border"),
        t!("Наведение", "Hover"),
        t!("Активное окно", "Active window"),
        t!("Акцент", "Accent"),
        t!("Текст", "Text"),
        t!("Вторичный текст", "Secondary text"),
    ]
}

pub(super) unsafe fn init(dlg: HWND) {
    for label in [
        t!("Синий", "Windows blue"),
        t!("Терминал", "Terminal green"),
        t!("Графит", "Graphite violet"),
        t!("Янтарь", "Amber"),
        t!("Своя палитра", "Custom"),
    ] {
        SendDlgItemMessageW(dlg, PRESET, CB_ADDSTRING, 0, wide(label).as_ptr() as _);
    }
    let list = GetDlgItem(dlg, LIST);
    settings::setup_table(list, &[(t!("Элемент", "Element"), 58), (t!("Цвет", "Color"), 42)], false);
    for (i, label) in names().iter().enumerate() {
        settings::insert_row(list, i, &[label, ""]);
    }
    let item = LVITEMW {
        state: LVIS_SELECTED | LVIS_FOCUSED,
        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
        ..zeroed()
    };
    SendMessageW(list, LVM_SETITEMSTATE, 0, &item as *const _ as _);
    sync(dlg);
}

unsafe fn sync(dlg: HWND) {
    let t = THEME.get();
    SendDlgItemMessageW(dlg, PRESET, CB_SETCURSEL, t.selected, 0);
    for (i, c) in t.colors().into_iter().enumerate() {
        let mut value = wide(&format!("#{:02X}{:02X}{:02X}", c & 255, (c >> 8) & 255, (c >> 16) & 255));
        let sub = LVITEMW {
            iSubItem: 1,
            pszText: value.as_mut_ptr(),
            ..zeroed()
        };
        SendDlgItemMessageW(dlg, LIST, LVM_SETITEMTEXTW, i, &sub as *const _ as _);
    }
    InvalidateRect(GetDlgItem(dlg, LIST), null(), 1);
}

pub(super) unsafe fn select(dlg: HWND) {
    let selected = SendDlgItemMessageW(dlg, PRESET, CB_GETCURSEL, 0, 0) as usize;
    if selected <= CUSTOM {
        Theme { selected, ..THEME.get() }.save(dlg);
    }
}

pub(super) unsafe fn edit(dlg: HWND, row: i32) {
    if !(0..7).contains(&row) {
        return;
    }
    let mut t = THEME.get();
    let mut slots = [0xffffff; 16];
    slots[..7].copy_from_slice(&t.colors());
    let mut picker = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: dlg,
        rgbResult: slots[row as usize],
        lpCustColors: slots.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..zeroed()
    };
    if ChooseColorW(&mut picker) == 0 {
        return;
    }
    if t.selected != CUSTOM {
        t.base = t.selected;
        t.custom = t.colors();
        t.selected = CUSTOM;
    }
    t.custom[row as usize] = picker.rgbResult;
    t.save(dlg);
}

pub(super) unsafe fn draw(dlg: HWND, lp: LPARAM) -> isize {
    let cd = &mut *(lp as *mut NMLVCUSTOMDRAW);
    let result = match cd.nmcd.dwDrawStage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW,
        CDDS_ITEMPREPAINT => CDRF_NOTIFYSUBITEMDRAW,
        stage if stage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) && cd.iSubItem == 1 => {
            if let Some(&color) = THEME.get().colors().get(cd.nmcd.dwItemSpec) {
                cd.clrTextBk = color;
                let light = (color & 255) * 299 + ((color >> 8) & 255) * 587 + ((color >> 16) & 255) * 114;
                cd.clrText = if light > 128000 { 0 } else { 0xffffff };
            }
            CDRF_NEWFONT
        }
        _ => CDRF_DODEFAULT,
    };
    SetWindowLongPtrW(dlg, DWLP_MSGRESULT as i32, result as _);
    1
}
