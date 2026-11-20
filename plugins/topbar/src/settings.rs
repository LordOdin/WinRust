use super::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Controls::*;

const HEIGHTS: [i32; 3] = [36, 40, 48];
const STYLE: i32 = 110;
const HEIGHT: i32 = 111;
const DISPLAYS: i32 = 114;
const TILE: i32 = 115;
const OPTIONS: i32 = 123;
const TAB: i32 = 124;
const TIMES: i32 = 125;
const HINT: i32 = 126;
const EDIT_TIME: u32 = WM_APP + 1;
const TIME_VALUE: i32 = 141;
const KEY: &str = r"Software\WinRust\Topbar";
const VALUE: &str = "LayoutV1";

#[derive(Clone, Copy)]
pub(super) struct Preferences {
    pub dock: bool,
    pub edge_index: usize,
    pub drag_bar: bool,
    pub auto_hide: bool,
    pub timed_shuffle: bool,
    pub left_align: bool,
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
        // Layouts saved before docking and palettes were separated inherit the old geometry.
        let legacy = i18n::style::legacy_style();
        let separate = bits & (1 << 18) != 0;
        Self {
            dock: if separate {
                bits & (1 << 15) != 0
            } else {
                legacy == i18n::style::Style::Dock
            },
            edge_index: if separate {
                ((bits >> 16) & 3) as usize
            } else if legacy == i18n::style::Style::Tiling {
                2
            } else {
                0
            },
            drag_bar: bits & (1 << 14) != 0,
            timed_shuffle: bits & (1 << 12) != 0,
            left_align: bits & (1 << 13) != 0,
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
            | ((self.auto_hide as u32) << 11)
            | ((self.timed_shuffle as u32) << 12)
            | ((self.left_align as u32) << 13)
            | ((self.drag_bar as u32) << 14)
            | ((self.dock as u32) << 15)
            | ((self.edge_index as u32) << 16)
            | (1 << 18);
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

type OptionRow = (&'static str, &'static str, fn(&mut Preferences) -> &mut bool, Option<Timing>);

fn options() -> [OptionRow; 13] {
    let behavior = t!("Панель", "Behavior");
    let icons = t!("Значки", "Icons");
    [
        (
            t!("Перетаскивать панель к краю экрана", "Drag taskbar to screen edges"),
            behavior,
            |p| &mut p.drag_bar,
            None,
        ),
        (
            t!("Выравнивать значки слева", "Left-align app icons"),
            icons,
            |p| &mut p.left_align,
            None,
        ),
        (
            t!("Перемешивать значки по таймеру", "Shuffle icons on a timer"),
            icons,
            |p| &mut p.timed_shuffle,
            Some(Timing::Shuffle),
        ),
        (
            t!("Перемешивать при загрузке Windows", "Shuffle at Windows startup"),
            icons,
            |p| &mut p.shuffle,
            None,
        ),
        (
            t!("Автоматически скрывать панель", "Auto-hide taskbar"),
            behavior,
            |p| &mut p.auto_hide,
            Some(Timing::AutoHide),
        ),
        (
            t!("Скрывать в полноэкранном режиме", "Hide in fullscreen"),
            behavior,
            |p| &mut p.hide_fullscreen,
            None,
        ),
        (
            t!("Использовать весь дополнительный экран", "Use full secondary display"),
            behavior,
            |p| &mut p.expand_secondary,
            None,
        ),
        (
            t!("Миниатюры только по щелчку", "Open previews only on click"),
            behavior,
            |p| &mut p.click_previews,
            None,
        ),
        (t!("Скрывать поиск", "Hide Search"), icons, |p| &mut p.hide_search, None),
        (t!("Скрывать Пуск", "Hide Start"), icons, |p| &mut p.hide_start, None),
        (
            t!("Сдвигать значки на 35 пикселей", "Drift icons up to 35 px"),
            "OLED",
            |p| &mut p.drift,
            Some(Timing::Drift),
        ),
        (
            t!("Область уведомлений при наведении", "Reveal tray on hover"),
            "OLED",
            |p| &mut p.hover_tray,
            None,
        ),
        (
            t!("Шестерёнка при наведении", "Reveal gear on hover"),
            "OLED",
            |p| &mut p.hover_gear,
            None,
        ),
    ]
}

pub(super) unsafe fn setup_table(list: HWND, columns: &[(&str, i32)], checks: bool) {
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

pub(super) unsafe fn insert_row(list: HWND, row: usize, columns: &[&str]) {
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
    SendDlgItemMessageW(dlg, STYLE, CB_SETCURSEL, prefs.dock as usize, 0);
    SendDlgItemMessageW(dlg, 131, CB_SETCURSEL, prefs.edge_index, 0);
    SendDlgItemMessageW(dlg, HEIGHT, CB_SETCURSEL, prefs.height_index, 0);
    for (i, (_, _, field, _)) in options().iter().enumerate() {
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
    let shuffle_changed = prefs.timed_shuffle != preferences().timed_shuffle;
    if let Err(err) = prefs.save() {
        if !dlg.is_null() {
            sync_controls(dlg);
        }
        MessageBoxW(
            dlg,
            wide(&err.to_string()).as_ptr(),
            wide(t!("Не удалось сохранить настройки", "Could not save settings")).as_ptr(),
            MB_ICONERROR,
        );
        return;
    }
    let bar = BAR.load(Relaxed) as HWND;
    if shuffle_changed {
        icon_order::schedule(bar);
    }
    if !prefs.expand_secondary {
        displays::restore();
    }
    previews::hide();
    HOT.set(NONE);
    refresh_buttons();
    place(bar);
    displays::reconcile(&displays::secondary_trays());
    fs_update(bar);
    if !dlg.is_null() {
        update_displays(dlg);
    }
}

pub(super) unsafe fn move_to(bar: HWND, index: usize) {
    if index == preferences().edge_index {
        return;
    }
    let dlg = DIALOG.get();
    let prefs = Preferences {
        edge_index: index,
        ..preferences()
    };
    fly::close_now();
    hide_tip(bar);
    ShowWindow(TRAYFLY.get(), SW_HIDE);
    apply(dlg, prefs);
    if !dlg.is_null() {
        sync_controls(dlg);
    }
}

unsafe fn show_page(dlg: HWND, page: isize) {
    ShowWindow(GetDlgItem(dlg, OPTIONS), if page == 0 { SW_SHOW } else { SW_HIDE });
    ShowWindow(GetDlgItem(dlg, TIMES), if page == 1 { SW_SHOW } else { SW_HIDE });
    for id in [1001, STYLE, 132, 131, 1002, HEIGHT, 1004, DISPLAYS, TILE, HINT] {
        ShowWindow(GetDlgItem(dlg,id), if page == 2 {SW_HIDE} else {SW_SHOW});
    }
    theme::mode_visible(dlg, page == 2);
    for id in (127..=130).chain(145..=151) {
        ShowWindow(GetDlgItem(dlg, id), if page == 2 { SW_SHOW } else { SW_HIDE });
    }
    let hint = if page == 2 {
        t!("Выберите цвет для редактирования.", "Select a color and choose Edit color.")
    } else if page == 1 {
        t!(
            "Дважды щёлкните строку или нажмите F2 для редактирования.",
            "Double-click a row or press F2 to edit its time."
        )
    } else {
        t!(
            "Дважды щёлкните время, чтобы изменить его.",
            "Double-click a time to edit it."
        )
    };
    SetDlgItemTextW(dlg, HINT, wide(hint).as_ptr());
}

// Open the editor after the list has finished dispatching its notification.
unsafe fn request_time_edit(dlg: HWND, row: i32) {
    if row >= 0 && (row as usize) < timing::ALL.len() {
        PostMessageW(dlg, EDIT_TIME, row as usize, 0);
    }
}

unsafe fn theme_brush(msg: u32, wp: WPARAM) -> Option<isize> {
    let field = match msg {
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC => false,
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => true,
        _ => return None,
    };
    let dc = wp as HDC;
    let bg = if field { Element::SettingsField.color() } else { Element::SettingsBg.color() };
    SetTextColor(dc, Element::SettingsText.color());
    SetBkColor(dc, bg);
    SetDCBrushColor(dc, bg);
    Some(GetStockObject(DC_BRUSH) as isize)
}

const DIALOG_FONT: *const u16 = windows_sys::w!("WinRustDialogFont");

unsafe fn dialog_font(dlg: HWND) {
    unsafe extern "system" fn set(child: HWND, font: LPARAM) -> i32 {
        SendMessageW(child, WM_SETFONT, font as usize, 0);
        1
    }
    let mut logfont: LOGFONTW = zeroed();
    let current = SendMessageW(dlg, WM_GETFONT, 0, 0);
    if current == 0 || GetObjectW(current as _, size_of::<LOGFONTW>() as i32, (&mut logfont as *mut LOGFONTW).cast()) == 0 { return; }
    let face = wide(ui_face());
    if logfont.lfFaceName.starts_with(&face) { return; }
    logfont.lfFaceName.fill(0);
    logfont.lfFaceName[..face.len()].copy_from_slice(&face);
    let font = CreateFontIndirectW(&logfont);
    if font.is_null() { return; }
    let old = GetPropW(dlg,DIALOG_FONT);
    if SetPropW(dlg,DIALOG_FONT,font) == 0 { DeleteObject(font); return; }
    SendMessageW(dlg, WM_SETFONT, font as usize, 0);
    EnumChildWindows(dlg, Some(set), font as isize);
    if !old.is_null() { DeleteObject(old); }
}

unsafe fn release_font(dlg: HWND) {
    let font = RemovePropW(dlg,DIALOG_FONT);
    if !font.is_null() { DeleteObject(font); }
}

unsafe fn restyle_dialog(dlg: HWND) {
    dialog_font(dlg);
    i18n::style::apply_frame(dlg);
    for id in [OPTIONS, TIMES, theme::LIST, DISPLAYS] {
        let list = GetDlgItem(dlg, id);
        if !list.is_null() {
            SendMessageW(list, LVM_SETBKCOLOR, 0, Element::SettingsList.color() as isize);
            SendMessageW(list, LVM_SETTEXTBKCOLOR, 0, Element::SettingsList.color() as isize);
            SendMessageW(list, LVM_SETTEXTCOLOR, 0, Element::SettingsRows.color() as isize);
        }
    }
    RedrawWindow(dlg, null(), null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
}

unsafe extern "system" fn time_editor(dlg: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    if let Some(brush) = theme_brush(msg, wp) { return brush; }
    if msg == i18n::style::msg() {
        i18n::style::reload();
        restyle_dialog(dlg);
        return 1;
    }
    match msg {
        WM_INITDIALOG => {
            let Some(&t) = timing::ALL.get(lp as usize) else { return 0 };
            SetWindowLongPtrW(dlg, GWLP_USERDATA, lp);
            SetWindowTextW(dlg, wide(t!("Изменить время", "Edit time")).as_ptr());
            SetDlgItemTextW(dlg, 140, wide(t.label()).as_ptr());
            SetDlgItemTextW(dlg, 142, wide(t.unit()).as_ptr());
            SetDlgItemTextW(dlg, IDOK, wide(t!("Сохранить", "Save")).as_ptr());
            SetDlgItemTextW(dlg, IDCANCEL, wide(t!("Отмена", "Cancel")).as_ptr());
            SendDlgItemMessageW(dlg, TIME_VALUE, EM_SETLIMITTEXT, 31, 0);
            SetDlgItemInt(dlg, TIME_VALUE, t.value(), 0);
            restyle_dialog(dlg);
            SetFocus(GetDlgItem(dlg, TIME_VALUE));
            SendDlgItemMessageW(dlg, TIME_VALUE, EM_SETSEL, 0, -1);
            return 0;
        }
        WM_COMMAND => match (wp & 0xffff) as i32 {
            IDOK => {
                let row = GetWindowLongPtrW(dlg, GWLP_USERDATA) as usize;
                let Some(&t) = timing::ALL.get(row) else { return 0 };
                let mut buffer = [0u16; 32];
                let len = GetDlgItemTextW(dlg, TIME_VALUE, buffer.as_mut_ptr(), buffer.len() as i32);
                let value = String::from_utf16_lossy(&buffer[..len as usize]).trim().parse::<u32>();
                let error = match value {
                    Ok(v) if (1..=t.maximum()).contains(&v) => t.save(v).err().map(|e| e.to_string()),
                    _ => Some(format!("1 – {} {}", t.maximum(), t.unit())),
                };
                if let Some(error) = error {
                    SetDlgItemTextW(dlg, 143, wide(&error).as_ptr());
                    SetFocus(GetDlgItem(dlg, TIME_VALUE));
                } else {
                    EndDialog(dlg, IDOK as isize);
                }
            }
            IDCANCEL => { EndDialog(dlg, IDCANCEL as isize); }
            _ => return 0,
        },
        WM_NCDESTROY => { release_font(dlg); return 0; }
        WM_CLOSE => { EndDialog(dlg, IDCANCEL as isize); }
        _ => return 0,
    }
    1
}

pub(super) unsafe fn set_cell(list: HWND, row: usize, column: i32, text: &str) {
    let mut value = wide(text);
    let item = LVITEMW { iSubItem: column, pszText: value.as_mut_ptr(), ..zeroed() };
    SendMessageW(list, LVM_SETITEMTEXTW, row, &item as *const _ as _);
}

unsafe fn edit_time(dlg: HWND, row: usize) {
    let Some(&t) = timing::ALL.get(row) else { return };
    if IsWindowEnabled(dlg) == 0 { return; }
    let result = DialogBoxParamW(GetModuleHandleW(null()), 102usize as _, dlg, Some(time_editor), row as isize);
    if result == -1 {
        SetDlgItemTextW(dlg, HINT, wide(&std::io::Error::last_os_error().to_string()).as_ptr());
        return;
    }
    if result != IDOK as isize { return; }
    if t == Timing::Shuffle {
        icon_order::schedule(BAR.load(Relaxed) as HWND);
    }
    set_cell(GetDlgItem(dlg, TIMES), row, 0, &t.value().to_string());
    for (i, option) in options().iter().enumerate() {
        if option.3 == Some(t) { set_cell(GetDlgItem(dlg, OPTIONS), i, 2, &t.display()); }
    }
    SetDlgItemTextW(dlg, HINT, wide(t!("Сохранено.", "Saved.")).as_ptr());
}

unsafe extern "system" fn dialog_proc(dlg: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    if let Some(brush) = theme_brush(msg, wp) { return brush; }
    if msg == i18n::style::msg() {
        i18n::style::reload();
        theme::sync(dlg);
        restyle_dialog(dlg);
        return 1;
    }
    match msg {
        WM_INITDIALOG => {
            DIALOG.set(dlg);
            DISPLAY_ROWS.take();
            for (kind, cx, cy) in [(ICON_SMALL, SM_CXSMICON, SM_CYSMICON), (ICON_BIG, SM_CXICON, SM_CYICON)] {
                let icon = LoadImageW(
                    GetModuleHandleW(null()),
                    1usize as _,
                    IMAGE_ICON,
                    GetSystemMetrics(cx),
                    GetSystemMetrics(cy),
                    LR_SHARED,
                );
                SendMessageW(dlg, WM_SETICON, kind as usize, icon as _);
            }
            if !i18n::english() {
                SetWindowTextW(dlg, wide("Настройки WinRust").as_ptr());
                for (id, text) in [
                    (1001, "Тип"),
                    (154, "Отключить WinRust..."), (127, "Палитра"), (145, "Шрифт"), (147, "Углы"), (149, "Цвета для"), (151, "По умолчанию"), (152, "Режим"),
                    (130, "Изменить цвет..."),
                    (132, "Край"),
                    (1002, "Высота панели"),
                    (1004, "Подключённые экраны"),
                    (TILE, "Разложить окна"),
                    (IDOK, "Закрыть"),
                ] {
                    SetDlgItemTextW(dlg, id, wide(text).as_ptr());
                }
            }
            for name in [t!("Панель", "Full-width bar"), t!("Плавающий док", "Floating dock")] {
                SendDlgItemMessageW(dlg, STYLE, CB_ADDSTRING, 0, wide(name).as_ptr() as _);
            }
            for name in [
                t!("Снизу", "Bottom"),
                t!("Слева", "Left"),
                t!("Сверху", "Top"),
                t!("Справа", "Right"),
            ] {
                SendDlgItemMessageW(dlg, 131, CB_ADDSTRING, 0, wide(name).as_ptr() as _);
            }
            for h in HEIGHTS {
                SendDlgItemMessageW(dlg, HEIGHT, CB_ADDSTRING, 0, wide(&format!("{h} px")).as_ptr() as isize);
            }
            let tab = GetDlgItem(dlg, TAB);
            for (i, label) in [t!("Настройки", "Options"), t!("Интервалы", "Timing"), t!("Тема", "Theme")]
                .iter()
                .enumerate()
            {
                let mut label = wide(label);
                let item = TCITEMW {
                    mask: TCIF_TEXT,
                    pszText: label.as_mut_ptr(),
                    ..zeroed()
                };
                SendMessageW(tab, TCM_INSERTITEMW, i, &item as *const _ as _);
            }
            let list = GetDlgItem(dlg, OPTIONS);
            setup_table(
                list,
                &[
                    (t!("Настройка", "Option"), 62),
                    (t!("Группа", "Group"), 19),
                    (t!("Время", "Time"), 19),
                ],
                true,
            );
            SYNCING.set(true);
            for (i, (label, group, _, time)) in options().iter().enumerate() {
                let value = time.map(Timing::display).unwrap_or_default();
                insert_row(list, i, &[label, group, &value]);
            }
            let times = GetDlgItem(dlg, TIMES);
            SendMessageW(times, CCM_SETUNICODEFORMAT, 1, 0);
            setup_table(
                times,
                &[
                    (t!("Время", "Time"), 18),
                    (t!("Ед.", "Unit"), 14),
                    (t!("Действие", "Behavior"), 68),
                ],
                false,
            );
            for (i, t) in timing::ALL.iter().enumerate() {
                insert_row(times, i, &[&t.value().to_string(), t.unit(), t.label()]);
            }
            theme::init(dlg);
            show_page(dlg, 0);
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
            restyle_dialog(dlg);
            update_displays(dlg);
            SetTimer(dlg, 1, 1000, None);
        }
        WM_TIMER => update_displays(dlg),
        EDIT_TIME => edit_time(dlg, wp),
        WM_NOTIFY => {
            let hdr = &*(lp as *const NMHDR);
            if hdr.idFrom == TAB as usize && hdr.code == TCN_SELCHANGE {
                show_page(dlg, SendMessageW(hdr.hwndFrom, TCM_GETCURSEL, 0, 0));
                return 0;
            }
            if hdr.idFrom == theme::LIST as usize {
                match hdr.code {
                    NM_CUSTOMDRAW => return theme::draw(dlg, lp),
                    LVN_ITEMCHANGED => theme::selection_changed(dlg),
                    NM_DBLCLK if (*(lp as *const NMITEMACTIVATE)).iItem >= 0 => { PostMessageW(dlg, WM_COMMAND, theme::EDIT as usize, 0); }
                    LVN_KEYDOWN if (*(lp as *const NMLVKEYDOWN)).wVKey == VK_F2 => { PostMessageW(dlg, WM_COMMAND, theme::EDIT as usize, 0); }
                    _ => {}
                }
                return 0;
            }
            if hdr.idFrom == TIMES as usize {
                match hdr.code {
                    NM_DBLCLK => request_time_edit(dlg, (*(lp as *const NMITEMACTIVATE)).iItem),
                    LVN_KEYDOWN if (*(lp as *const NMLVKEYDOWN)).wVKey == VK_F2 => {
                        let row = SendMessageW(hdr.hwndFrom, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as _) as i32;
                        request_time_edit(dlg, row);
                    }
                    _ => {}
                }
                return 0;
            }
            if hdr.idFrom == OPTIONS as usize && hdr.code == NM_DBLCLK {
                let cell = &*(lp as *const NMITEMACTIVATE);
                if cell.iSubItem == 2 {
                    if let Some(t) = options().get(cell.iItem as usize).and_then(|opt| opt.3) {
                        SendDlgItemMessageW(dlg, TAB, TCM_SETCURSEL, 1, 0);
                        show_page(dlg, 1);
                        request_time_edit(dlg, t as i32);
                    }
                }
                return 0;
            }
            if hdr.idFrom != OPTIONS as usize || hdr.code != LVN_ITEMCHANGED || SYNCING.get() {
                return 0;
            }
            let change = &*(lp as *const NMLISTVIEW);
            let old = (change.uOldState & LVIS_STATEIMAGEMASK) >> 12;
            let new = (change.uNewState & LVIS_STATEIMAGEMASK) >> 12;
            if old == new || old == 0 || !(1..=2).contains(&new) {
                return 0;
            }
            if let Some((_, _, field, _)) = options().get(change.iItem as usize) {
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
                    if index <= 1 {
                        apply(
                            dlg,
                            Preferences {
                                dock: index == 1,
                                ..preferences()
                            },
                        );
                    }
                }
                131 if notification == CBN_SELCHANGE => {
                    let index = SendDlgItemMessageW(dlg, 131, CB_GETCURSEL, 0, 0) as usize;
                    if index < 4 {
                        move_to(bar, index);
                    }
                }
                theme::PRESET | theme::FONT | theme::CORNERS | theme::GROUP | theme::MODE if notification == CBN_SELCHANGE => theme::select(dlg, id),
                theme::RESET => theme::reset(dlg),
                theme::EDIT => {
                    let row = SendDlgItemMessageW(dlg, theme::LIST, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as _) as i32;
                    theme::edit(dlg, row);
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
                154 => {
                    let path = std::env::current_exe().ok()
                        .and_then(|p| p.parent()?.parent().map(|d| d.join(r"WinRust\WinRust-setup.exe")));
                    let result = path.filter(|p| p.is_file()).map(|p| ShellExecuteW(dlg, windows_sys::w!("runas"), wide(&p.to_string_lossy()).as_ptr(),
                        windows_sys::w!("--windows-tools"), null(), SW_SHOWNORMAL) as isize);
                    if result.is_none_or(|v| v <= 32) {
                        MessageBoxW(dlg, wide(t!("Не удалось запустить WinRust Setup.", "Could not start WinRust Setup.")).as_ptr(), wide("WinRust").as_ptr(), MB_ICONERROR);
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
            release_font(dlg);
            DIALOG.set(null_mut());
            return 0;
        }
        _ => return 0,
    }
    1
}

pub(super) fn is_dialog(w: HWND) -> bool {
    !w.is_null() && DIALOG.get() == w
}

pub(super) unsafe fn open() {
    let mut dlg = DIALOG.get();
    if dlg.is_null() {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES,
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
    if dlg.is_null() {
        return false;
    }
    IsDialogMessageW(dlg, msg) != 0
}
