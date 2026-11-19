use super::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Controls::*;

const HEIGHTS: [i32; 3] = [36, 40, 48];
const STYLE: i32 = 110;
const HEIGHT: i32 = 111;
const DISPLAYS: i32 = 114;
const TILE: i32 = 115;
const OPTIONS: i32 = 123;
const KEY: &str = r"Software\WinRust\Topbar";
const VALUE: &str = "LayoutV1";

#[derive(Clone, Copy)]
pub(super) struct Preferences {
    pub auto_hide: bool,
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
            auto_hide: bits & (1 << 11) != 0,
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
            | ((self.hover_gear as u32) << 10)
            | ((self.auto_hide as u32) << 11);
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
    static DISPLAY_ROWS: RefCell<Vec<[String; 4]>> = const { RefCell::new(Vec::new()) };
    static SYNCING: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn preferences() -> Preferences {
    PREFS.get()
}

type OptionRow = (&'static str, &'static str, fn(&mut Preferences) -> &mut bool);

fn options() -> [OptionRow; 10] {
    let behavior = t!("Панель", "Behavior");
    let icons = t!("Значки", "Icons");
    [
        (
            t!("Автоматически скрывать панель", "Auto-hide taskbar"),
            behavior,
            |p| &mut p.auto_hide,
        ),
        (
            t!("Скрывать в полноэкранном режиме", "Hide in fullscreen"),
            behavior,
            |p| &mut p.hide_fullscreen,
        ),
        (
            t!("Использовать весь дополнительный экран", "Use full secondary display"),
            behavior,
            |p| &mut p.expand_secondary,
        ),
        (
            t!("Открывать миниатюры по щелчку", "Click to open previews"),
            behavior,
            |p| &mut p.click_previews,
        ),
        (
            t!("Перемешивать при загрузке Windows", "Shuffle at Windows startup"),
            icons,
            |p| &mut p.shuffle,
        ),
        (t!("Скрывать поиск", "Hide Search"), icons, |p| {
            &mut p.hide_search
        }),
        (t!("Скрывать Пуск", "Hide Start"), icons, |p| &mut p.hide_start),
        (
            t!("Сдвигать значки на 35 пикселей", "Drift icons up to 35 px"),
            "OLED",
            |p| &mut p.drift,
        ),
        (
            t!("Область уведомлений при наведении", "Reveal tray on hover"),
            "OLED",
            |p| &mut p.hover_tray,
        ),
        (
            t!("Шестерёнка при наведении", "Reveal gear on hover"),
            "OLED",
            |p| &mut p.hover_gear,
        ),
    ]
}

unsafe fn setup_table(list: HWND, columns: &[(&str, i32)], checks: bool) {
    let styles = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | if checks { LVS_EX_CHECKBOXES } else { 0 };
    SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, styles as isize);
    let mut bounds: RECT = zeroed();
    GetClientRect(list, &mut bounds);
    let width = (bounds.right - GetSystemMetrics(SM_CXVSCROLL) - 2).max(1);
    for (i, &(name, percent)) in columns.iter().enumerate() {
        let mut text = wide(name);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            pszText: text.as_mut_ptr(),
            cx: width * percent / 100,
            ..zeroed()
        };
        SendMessageW(list, LVM_INSERTCOLUMNW, i, &col as *const _ as isize);
    }
}

unsafe fn insert_row(list: HWND, row: usize, columns: &[&str]) {
    for (i, value) in columns.iter().enumerate() {
        let mut text = wide(value);
        let item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: row as i32,
            iSubItem: i as i32,
            pszText: text.as_mut_ptr(),
            ..zeroed()
        };
        let msg = if i == 0 { LVM_INSERTITEMW } else { LVM_SETITEMTEXTW };
        SendMessageW(list, msg, row, &item as *const _ as isize);
    }
}

unsafe fn update_displays(dlg: HWND) {
    let rows = displays::rows();
    if DISPLAY_ROWS.with_borrow(|old| *old == rows) {
        return;
    }
    let list = GetDlgItem(dlg, DISPLAYS);
    SendMessageW(list, WM_SETREDRAW, 0, 0);
    SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    for (i, row) in rows.iter().enumerate() {
        insert_row(list, i, &row.each_ref().map(String::as_str));
    }
    SendMessageW(list, WM_SETREDRAW, 1, 0);
    InvalidateRect(list, null(), 1);
    DISPLAY_ROWS.replace(rows);
}

unsafe fn sync_controls(dlg: HWND) {
    SYNCING.set(true);
    let mut prefs = preferences();
    let style_index = i18n::style::ALL.iter().position(|s| *s == style()).unwrap_or(0);
    SendDlgItemMessageW(dlg, STYLE, CB_SETCURSEL, style_index, 0);
    SendDlgItemMessageW(dlg, HEIGHT, CB_SETCURSEL, prefs.height_index, 0);
    for (i, (_, _, field)) in options().iter().enumerate() {
        let state = if *field(&mut prefs) { 2 } else { 1 };
        let item = LVITEMW {
            stateMask: LVIS_STATEIMAGEMASK,
            state: state << 12,
            ..zeroed()
        };
        SendDlgItemMessageW(dlg, OPTIONS, LVM_SETITEMSTATE, i, &item as *const _ as isize);
    }
    SYNCING.set(false);
}

unsafe fn apply(dlg: HWND, prefs: Preferences) {
    if let Err(err) = prefs.save() {
        sync_controls(dlg);
        MessageBoxW(
            dlg,
            wide(&err.to_string()).as_ptr(),
            wide(t!("Не удалось сохранить настройки", "Could not save settings")).as_ptr(),
            MB_ICONERROR,
        );
        return;
    }
    let bar = BAR.load(Relaxed) as HWND;
    if !prefs.expand_secondary {
        displays::restore();
    }
    previews::hide();
    HOT.set(NONE);
    refresh_buttons();
    place(bar);
    displays::reconcile(&displays::secondary_trays());
    fs_update(bar);
    update_displays(dlg);
}

unsafe extern "system" fn dialog_proc(dlg: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            DIALOG.set(dlg);
            DISPLAY_ROWS.take();
            if !i18n::english() {
                SetWindowTextW(dlg, wide("Настройки WinRust").as_ptr());
                for (id, text) in [
                    (1001, "Стиль"),
                    (1002, "Высота панели"),
                    (1003, "Настройки панели"),
                    (1004, "Подключённые экраны"),
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
            let list = GetDlgItem(dlg, OPTIONS);
            setup_table(list, &[(t!("Настройка", "Option"), 72), (t!("Группа", "Group"), 28)], true);
            SYNCING.set(true);
            for (i, (label, group, _)) in options().iter().enumerate() {
                insert_row(list, i, &[label, group]);
            }
            sync_controls(dlg);
            setup_table(
                GetDlgItem(dlg, DISPLAYS),
                &[
                    (t!("Экран", "Display"), 23),
                    (t!("Назначение", "Role"), 24),
                    (t!("Разрешение", "Resolution"), 26),
                    (t!("Рабочая область", "Usable area"), 27),
                ],
                false,
            );
            update_displays(dlg);
            SetTimer(dlg, 1, 1000, None);
        }
        WM_TIMER => update_displays(dlg),
        WM_NOTIFY => {
            let hdr = &*(lp as *const NMHDR);
            if hdr.idFrom != OPTIONS as usize || hdr.code != LVN_ITEMCHANGED || SYNCING.get() {
                return 0;
            }
            let change = &*(lp as *const NMLISTVIEW);
            let old = (change.uOldState & LVIS_STATEIMAGEMASK) >> 12;
            let new = (change.uNewState & LVIS_STATEIMAGEMASK) >> 12;
            if old == new || old == 0 || !(1..=2).contains(&new) {
                return 0;
            }
            if let Some((_, _, field)) = options().get(change.iItem as usize) {
                let mut prefs = preferences();
                *field(&mut prefs) = new == 2;
                apply(dlg, prefs);
            }
            return 0;
        }
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
                HEIGHT if notification == CBN_SELCHANGE => {
                    let index = SendDlgItemMessageW(dlg, HEIGHT, CB_GETCURSEL, 0, 0) as usize;
                    if index < HEIGHTS.len() {
                        apply(
                            dlg,
                            Preferences {
                                height_index: index,
                                ..preferences()
                            },
                        );
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
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        });
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
