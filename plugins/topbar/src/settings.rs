use super::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, CheckDlgButton, IsDlgButtonChecked};

const HEIGHTS: [i32; 3] = [36, 40, 48];
const STYLE: i32 = 110;
const HEIGHT: i32 = 111;
const FULL: i32 = 112;
const SECONDARY: i32 = 113;
const DISPLAYS: i32 = 114;
const TILE: i32 = 115;
const CLICK_PREVIEWS: i32 = 116;
const DRIFT: i32 = 117;
const SHUFFLE: i32 = 118;
const HIDE_SEARCH: i32 = 119;
const HIDE_START: i32 = 120;
const HOVER_TRAY: i32 = 121;
const HOVER_GEAR: i32 = 122;
const KEY: &str = r"Software\WinRust\Topbar";
const VALUE: &str = "LayoutV1";

#[derive(Clone, Copy)]
pub(super) struct Preferences {
    pub hide_fullscreen: bool,
    pub expand_secondary: bool,
    pub click_previews: bool,
    pub drift: bool,
    pub shuffle: bool,
    pub hide_search: bool,
    pub hide_start: bool,
    pub hover_tray: bool,
    pub hover_gear: bool,
    height_index: usize,
}

impl Preferences {
    pub fn height(self) -> i32 {
        HEIGHTS[self.height_index]
    }

    fn decode(bits: u32) -> Self {
        let index = ((bits >> 2) & 3) as usize;
        Self {
            hide_fullscreen: bits & 1 != 0,
            expand_secondary: bits & 2 != 0,
            click_previews: bits & (1 << 4) != 0,
            drift: bits & (1 << 5) != 0,
            shuffle: bits & (1 << 6) != 0,
            hide_search: bits & (1 << 7) != 0,
            hide_start: bits & (1 << 8) != 0,
            hover_tray: bits & (1 << 9) != 0,
            hover_gear: bits & (1 << 10) != 0,
            height_index: if index < HEIGHTS.len() { index } else { 1 },
        }
    }

    fn read() -> Self {
        let mut bits = 0u32;
        let mut len = 4;
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide(VALUE).as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                &mut bits as *mut _ as _,
                &mut len,
            )
        };
        Self::decode(if err == ERROR_SUCCESS { bits } else { 7 })
    }

    fn save(self) -> Result<(), std::io::Error> {
        let bits = self.hide_fullscreen as u32
            | ((self.expand_secondary as u32) << 1)
            | ((self.height_index as u32) << 2)
            | ((self.click_previews as u32) << 4)
            | ((self.drift as u32) << 5)
            | ((self.shuffle as u32) << 6)
            | ((self.hide_search as u32) << 7)
            | ((self.hide_start as u32) << 8)
            | ((self.hover_tray as u32) << 9)
            | ((self.hover_gear as u32) << 10);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide(VALUE).as_ptr(),
                REG_DWORD,
                &bits as *const _ as _,
                4,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        PREFS.set(self);
        Ok(())
    }
}

thread_local! {
    static PREFS: Cell<Preferences> = Cell::new(Preferences::read());
    static DIALOG: Cell<HWND> = const { Cell::new(null_mut()) };
    static DISPLAY_TEXT: RefCell<String> = RefCell::new(String::new());
}

pub(super) fn preferences() -> Preferences {
    PREFS.get()
}

unsafe fn display_text(dlg: HWND) {
    let text = displays::summary();
    if DISPLAY_TEXT.with_borrow(|old| *old != text) {
        SetDlgItemTextW(dlg, DISPLAYS, wide(&text).as_ptr());
        DISPLAY_TEXT.replace(text);
    }
}

unsafe fn sync_controls(dlg: HWND) {
    let prefs = preferences();
    let style_index = i18n::style::ALL.iter().position(|s| *s == style()).unwrap_or(0);
    SendDlgItemMessageW(dlg, STYLE, CB_SETCURSEL, style_index, 0);
    SendDlgItemMessageW(dlg, HEIGHT, CB_SETCURSEL, prefs.height_index, 0);
    for (id, checked) in [
        (FULL, prefs.hide_fullscreen),
        (SECONDARY, prefs.expand_secondary),
        (CLICK_PREVIEWS, prefs.click_previews),
        (DRIFT, prefs.drift),
        (SHUFFLE, prefs.shuffle),
        (HIDE_SEARCH, prefs.hide_search),
        (HIDE_START, prefs.hide_start),
        (HOVER_TRAY, prefs.hover_tray),
        (HOVER_GEAR, prefs.hover_gear),
    ] {
        CheckDlgButton(dlg, id, if checked { BST_CHECKED } else { BST_UNCHECKED });
    }
}

unsafe extern "system" fn dialog_proc(dlg: HWND, msg: u32, wp: WPARAM, _: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            DIALOG.set(dlg);
            DISPLAY_TEXT.take();
            if !i18n::english() {
                SetWindowTextW(dlg, wide("Настройки WinRust").as_ptr());
                for (id, text) in [
                    (1000, "Оформление"),
                    (1001, "Стиль"),
                    (1002, "Высота панели"),
                    (1003, "Поведение окон"),
                    (FULL, "Скрывать панель в полноэкранных приложениях"),
                    (SECONDARY, "Использовать всё место на экранах без панели WinRust"),
                    (1004, "Подключённые экраны"),
                    (CLICK_PREVIEWS, "Открывать миниатюры по щелчку"),
                    (DRIFT, "Сдвигать значки на 35 пикселей влево и вправо"),
                    (SHUFFLE, "Перемешивать значки при загрузке Windows"),
                    (HIDE_SEARCH, "Скрыть значок поиска"),
                    (HIDE_START, "Скрыть значок Пуска"),
                    (HOVER_TRAY, "Показывать область уведомлений при наведении"),
                    (HOVER_GEAR, "Показывать шестерёнку при наведении"),
                    (1005, "Значки"),
                    (1006, "Защита OLED"),
                    (TILE, "Разложить окна"),
                    (IDOK, "Закрыть"),
                ] {
                    SetDlgItemTextW(dlg, id, wide(text).as_ptr());
                }
            }
            for name in [
                t!("Обычный", "Normal"),
                t!("Терминал", "Terminal"),
                t!("Плавающий док", "Floating dock"),
                t!("Тайлинг", "Tiling"),
            ] {
                SendDlgItemMessageW(dlg, STYLE, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
            }
            for h in HEIGHTS {
                SendDlgItemMessageW(dlg, HEIGHT, CB_ADDSTRING, 0, wide(&format!("{h} px")).as_ptr() as isize);
            }
            sync_controls(dlg);
            display_text(dlg);
            SetTimer(dlg, 1, 1000, None);
        }
        WM_TIMER => display_text(dlg),
        WM_COMMAND => {
            let id = (wp & 0xffff) as i32;
            let notification = (wp >> 16) as u32;
            let bar = BAR.load(Relaxed) as HWND;
            match id {
                STYLE if notification == CBN_SELCHANGE => {
                    let index = SendDlgItemMessageW(dlg, STYLE, CB_GETCURSEL, 0, 0) as usize;
                    if let Some(&s) = i18n::style::ALL.get(index) {
                        i18n::style::set_style(s);
                        place(bar);
                    }
                }
                HEIGHT | FULL | SECONDARY | CLICK_PREVIEWS..=HOVER_GEAR
                    if (id == HEIGHT && notification == CBN_SELCHANGE) || (id != HEIGHT && notification == BN_CLICKED) =>
                {
                    let index = SendDlgItemMessageW(dlg, HEIGHT, CB_GETCURSEL, 0, 0) as usize;
                    if index >= HEIGHTS.len() {
                        return 1;
                    }
                    let prefs = Preferences {
                        click_previews: IsDlgButtonChecked(dlg, CLICK_PREVIEWS) == BST_CHECKED,
                        drift: IsDlgButtonChecked(dlg, DRIFT) == BST_CHECKED,
                        shuffle: IsDlgButtonChecked(dlg, SHUFFLE) == BST_CHECKED,
                        hide_search: IsDlgButtonChecked(dlg, HIDE_SEARCH) == BST_CHECKED,
                        hide_start: IsDlgButtonChecked(dlg, HIDE_START) == BST_CHECKED,
                        hover_tray: IsDlgButtonChecked(dlg, HOVER_TRAY) == BST_CHECKED,
                        hover_gear: IsDlgButtonChecked(dlg, HOVER_GEAR) == BST_CHECKED,
                        height_index: index,
                        hide_fullscreen: IsDlgButtonChecked(dlg, FULL) == BST_CHECKED,
                        expand_secondary: IsDlgButtonChecked(dlg, SECONDARY) == BST_CHECKED,
                    };
                    if let Err(err) = prefs.save() {
                        sync_controls(dlg);
                        MessageBoxW(
                            dlg,
                            wide(&err.to_string()).as_ptr(),
                            wide(t!("Не удалось сохранить настройки", "Could not save settings")).as_ptr(),
                            MB_ICONERROR,
                        );
                    } else {
                        if !prefs.expand_secondary {
                            displays::restore();
                        }
                        previews::hide();
                        HOT.set(NONE);
                        refresh_buttons();
                        place(bar);
                        displays::reconcile(&displays::secondary_trays());
                        fs_update(bar);
                    }
                }
                TILE => {
                    DestroyWindow(dlg);
                    tile_windows();
                }
                IDOK | IDCANCEL => {
                    DestroyWindow(dlg);
                }
                _ => return 0,
            }
        }
        WM_CLOSE => {
            DestroyWindow(dlg);
        }
        WM_NCDESTROY => {
            DIALOG.set(null_mut());
            return 0;
        }
        _ => return 0,
    }
    1
}

pub(super) unsafe fn open() {
    let mut dlg = DIALOG.get();
    if dlg.is_null() {
        dlg = CreateDialogParamW(GetModuleHandleW(null()), 101usize as _, null_mut(), Some(dialog_proc), 0);
        if dlg.is_null() {
            let err = std::io::Error::last_os_error();
            MessageBoxW(
                null_mut(),
                wide(&err.to_string()).as_ptr(),
                wide("WinRust Settings").as_ptr(),
                MB_ICONERROR,
            );
            return;
        }
    }
    ShowWindow(dlg, SW_RESTORE);
    SetForegroundWindow(dlg);
}

pub(super) unsafe fn handles(msg: &MSG) -> bool {
    let dlg = DIALOG.get();
    !dlg.is_null() && IsDialogMessageW(dlg, msg) != 0
}
