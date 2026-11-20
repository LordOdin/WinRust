//! An owned, keyboard-accessible folder browser. Hover shows it without taking focus.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use std::{
    collections::HashMap,
    ffi::OsString,
    mem::{size_of, zeroed},
    sync::{
        Arc,
        atomic::AtomicU64,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Graphics::Gdi::*,
    System::Registry::*,
    UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_BACK, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_TAB},
};

const DWELL: usize = 0x7241;
const WATCH: usize = 0x7242;
const BACK: usize = 1;
const UP: usize = 2;
const CLOSE: usize = 3;
const ITEMS: usize = 4;
const ACTIVATE: u32 = WM_APP + 8;
const CLASS: *const u16 = windows_sys::w!("WinRustFolderPopup");
type Request = (u64, PathBuf, bool);

struct Entry {
    name: OsString,
    folder: bool,
    size: u64,
    icon: i32,
}
struct Listing {
    entries: Vec<Entry>,
    folders: usize,
    note: String,
}

// Query the shared shell image list by file type on the reader thread.
fn type_icon(icons: &mut HashMap<OsString, i32>, name: &OsString, folder: bool) -> i32 {
    let ext = Path::new(name).extension().unwrap_or_default();
    let key = if folder { OsString::from("folder/") } else { ext.to_ascii_lowercase() };
    *icons.entry(key).or_insert_with(|| unsafe {
        let mut fi: SHFILEINFOW = zeroed();
        let sample = if folder {
            "folder".into()
        } else {
            format!("item.{}", ext.to_string_lossy())
        };
        SHGetFileInfoW(
            w(&sample).as_ptr(),
            if folder { 0x10 } else { 0x80 },
            &mut fi,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES,
        );
        fi.iIcon
    })
}

fn contents(path: &Path, hidden: bool, icons: &mut HashMap<OsString, i32>, current: impl Fn() -> bool) -> Option<Listing> {
    let mut entries = Vec::new();
    let mut folders = 0;
    let listing = fs::read_dir(path).and_then(|dir| {
        for item in dir {
            if !current() {
                break;
            }
            let entry = item?;
            let meta = entry.metadata().ok();
            if !hidden && meta.as_ref().is_some_and(|m| m.file_attributes() & 2 != 0) {
                continue;
            }
            let folder = meta.as_ref().map_or_else(|| entry.file_type().is_ok_and(|t| t.is_dir()), |m| m.is_dir());
            folders += folder as usize;
            let name = entry.file_name();
            let icon = type_icon(icons, &name, folder);
            entries.push(Entry {
                name,
                folder,
                size: meta.map_or(0, |m| m.len()),
                icon,
            });
        }
        Ok(())
    });
    if !current() {
        return None;
    }
    entries.sort_by_cached_key(|e| (!e.folder, e.name.to_string_lossy().to_lowercase()));
    let note = match listing {
        Err(e) => format!(
            "{}\n{e}",
            winrust_common::t!("Не удалось прочитать всё содержимое папки.", "Could not read the entire folder.")
        ),
        Ok(()) if entries.is_empty() => winrust_common::t!("Папка пуста", "This folder is empty").into(),
        _ => String::new(),
    };
    Some(Listing { entries, folders, note })
}

#[derive(Clone, Copy)]
struct Windows {
    source: HWND,
    popup: HWND,
    items: HWND,
}

struct Browser {
    windows: Windows,
    generation: Arc<AtomicU64>,
    send: Sender<Request>,
    results: Receiver<(u64, Listing)>,
    anchor: Option<(usize, PathBuf)>,
    away: Option<Instant>,
    dismiss_after: Duration,
    path: PathBuf,
    history: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    listing: Listing,
    fonts: [isize; 4],
}
thread_local! { static BROWSER: RefCell<Option<Browser>> = const {RefCell::new(None)}; }

fn windows() -> Option<Windows> {
    BROWSER.with_borrow(|b| b.as_ref().map(|b| b.windows))
}
fn font_set() -> [isize; 4] {
    [
        ui::font(14, 400, ui::ui_face()),
        ui::font(17, 600, ui::ui_face()),
        ui::font(12, 400, ui::ui_face()),
        ui::font(15, 400, "Segoe MDL2 Assets"),
    ]
}

pub unsafe fn register(inst: HINSTANCE) {
    RegisterClassW(&WNDCLASSW {
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(proc),
        hInstance: inst,
        hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
        lpszClassName: CLASS,
        ..zeroed()
    });
}

unsafe fn create(source: HWND) -> Option<()> {
    let inst = GetModuleHandleW(ptr::null());
    let popup = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        CLASS,
        windows_sys::w!("Folder browser"),
        WS_POPUP | WS_CLIPCHILDREN,
        0,
        0,
        0,
        0,
        GetAncestor(source, GA_ROOT),
        ptr::null_mut(),
        inst,
        ptr::null(),
    );
    if popup.is_null() {
        return None;
    }
    let items = CreateWindowExW(
        0,
        WC_LISTVIEWW,
        ptr::null(),
        WS_CHILD | WS_TABSTOP | LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_OWNERDATA | LVS_SHAREIMAGELISTS,
        0,
        0,
        0,
        0,
        popup,
        ITEMS as _,
        inst,
        ptr::null(),
    );
    let mut ok = !items.is_null();
    for (id, caption) in [
        (BACK, winrust_common::t!("Назад", "Back")),
        (UP, winrust_common::t!("Вверх", "Parent folder")),
        (CLOSE, winrust_common::t!("Закрыть", "Close")),
    ] {
        let button = CreateWindowExW(
            0,
            windows_sys::w!("BUTTON"),
            w(caption).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
            0,
            0,
            0,
            0,
            popup,
            id as _,
            inst,
            ptr::null(),
        );
        ok &= !button.is_null();
    }
    if !ok {
        DestroyWindow(popup);
        return None;
    }
    let generation = Arc::new(AtomicU64::new(0));
    let epoch = generation.clone();
    let (send, requests) = mpsc::channel::<Request>();
    let (done, results) = mpsc::channel();
    if std::thread::Builder::new()
        .name("folder browser".into())
        .spawn(move || {
            sh::init();
            let mut icons = HashMap::new();
            while let Ok(first) = requests.recv() {
                let (id, path, hidden) = requests.try_iter().last().unwrap_or(first);
                let current = || epoch.load(Relaxed) == id;
                if current()
                    && let Some(listing) = contents(&path, hidden, &mut icons, current)
                {
                    let _ = done.send((id, listing));
                }
            }
        })
        .is_err()
    {
        DestroyWindow(popup);
        return None;
    }
    let fonts = font_set();
    let handles = Windows { source, popup, items };
    BROWSER.replace(Some(Browser {
        windows: handles,
        generation,
        send,
        results,
        anchor: None,
        away: None,
        dismiss_after: Duration::ZERO,
        path: PathBuf::new(),
        history: vec![],
        forward: vec![],
        listing: Listing {
            entries: vec![],
            folders: 0,
            note: String::new(),
        },
        fonts,
    }));
    put(items, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as _);
    let images = put(source, LVM_GETIMAGELIST, LVSIL_SMALL as usize, 0);
    put(items, LVM_SETIMAGELIST, LVSIL_SMALL as usize, images);
    for (i, title) in ["Name", "Size"].iter().enumerate() {
        let mut title = w(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_FMT | LVCF_WIDTH,
            fmt: if i == 0 { LVCFMT_LEFT } else { LVCFMT_RIGHT },
            pszText: title.as_mut_ptr(),
            cx: 100,
            ..zeroed()
        };
        put(items, LVM_INSERTCOLUMNW, i, &col as *const _ as _);
    }
    restyle(handles);
    Some(())
}

unsafe fn restyle(h: Windows) {
    let p = ui::pal();
    ui::dark_title(h.popup);
    SetWindowTheme(h.items, windows_sys::w!("DarkMode_Explorer"), ptr::null());
    for (msg, color) in [
        (LVM_SETBKCOLOR, ui::listbg()),
        (LVM_SETTEXTBKCOLOR, ui::listbg()),
        (LVM_SETTEXTCOLOR, p.text),
    ] {
        put(h.items, msg, 0, color as _);
    }
    let font = BROWSER.with_borrow(|b| b.as_ref().unwrap().fonts[0]);
    put(h.items, WM_SETFONT, font as _, 1);
}

fn row_path(index: isize) -> Option<PathBuf> {
    ST.with(|cell| {
        let st = cell.try_borrow().ok()?;
        let row = st.rows.get(index as usize)?;
        row.dir.then(|| st.dir.join(&row.name))
    })
}

fn delay(name: *const u16, default: u32) -> u32 {
    let mut value = 0u32;
    let mut size = 4;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows_sys::w!(r"Software\WinRust\Topbar\Timing"),
            name,
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    if result == ERROR_SUCCESS && (1..=86_400_000).contains(&value) {
        value
    } else {
        default
    }
}

pub fn cancel(list: HWND) {
    let h = BROWSER.with_borrow_mut(|state| {
        let b = state.as_mut()?;
        if b.anchor.take().is_none() {
            return None;
        }
        b.generation.fetch_add(1, Relaxed);
        b.away = None;
        Some(b.windows)
    });
    if let Some(h) = h {
        unsafe {
            KillTimer(list, DWELL);
            KillTimer(list, WATCH);
            ShowWindow(h.popup, SW_HIDE);
        }
    }
}

pub unsafe fn moved(list: HWND, index: isize) {
    if windows().is_some_and(|h| IsWindowVisible(h.popup) != 0) {
        return;
    }
    let path = row_path(index);
    if windows().is_none() && (path.is_none() || create(list).is_none()) {
        return;
    }
    if BROWSER.with_borrow(|s| {
        s.as_ref()
            .and_then(|b| b.anchor.as_ref())
            .is_some_and(|(row, p)| *row == index as usize && Some(p) == path.as_ref())
    }) {
        return;
    }
    cancel(list);
    if let Some(path) = path {
        BROWSER.with_borrow_mut(|s| s.as_mut().unwrap().anchor = Some((index as usize, path)));
        SetTimer(
            list,
            DWELL,
            delay(windows_sys::w!("FolderPreview"), winrust_common::settings::FOLDER_PREVIEW_DELAY_MS),
            None,
        );
    }
}

unsafe fn hit(source: HWND, pt: POINT) -> isize {
    if WindowFromPoint(pt) != source {
        return -1;
    }
    let mut info: LVHITTESTINFO = zeroed();
    info.pt = pt;
    ScreenToClient(source, &mut info.pt);
    put(source, LVM_HITTEST, 0, &mut info as *mut _ as _)
}

unsafe fn position(h: Windows, pt: POINT) {
    let mut mon = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    if GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mon) == 0 {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut mon.rcWork as *mut RECT).cast(), 0);
    }
    let r = mon.rcWork;
    let (width, height) = (ui::sc(396).min(r.right - r.left), ui::sc(376).min(r.bottom - r.top));
    let gap = ui::sc(7);
    let x = if pt.x + gap + width <= r.right { pt.x + gap } else { pt.x - gap - width };
    let y = (pt.y - ui::sc(22)).min(r.bottom - height).max(r.top);
    SetWindowPos(h.popup, HWND_TOP, x.max(r.left), y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

enum Move {
    Start(PathBuf),
    Into(PathBuf),
    Back,
    Forward,
    Up,
}
unsafe fn navigate(h: Windows, action: Move) {
    let changed = BROWSER.with_borrow_mut(|s| {
        let b = s.as_mut().unwrap();
        let path = match action {
            Move::Start(path) => {
                b.history.clear();
                b.forward.clear();
                path
            }
            Move::Back => {
                let Some(path) = b.history.pop() else { return false };
                b.forward.push(b.path.clone());
                path
            }
            Move::Forward => {
                let Some(path) = b.forward.pop() else { return false };
                b.history.push(b.path.clone());
                path
            }
            action => {
                let path = match action {
                    Move::Into(path) => path,
                    Move::Up => match b.path.parent() {
                        Some(p) => p.to_owned(),
                        None => return false,
                    },
                    _ => unreachable!(),
                };
                b.history.push(b.path.clone());
                b.forward.clear();
                path
            }
        };
        let id = b.generation.fetch_add(1, Relaxed) + 1;
        b.path = path.clone();
        b.listing = Listing {
            entries: vec![],
            folders: 0,
            note: winrust_common::t!("Загрузка…", "Loading…").into(),
        };
        if b.send.send((id, path, SHOW_HIDDEN.load(Relaxed))).is_err() {
            b.listing.note = winrust_common::t!("Ошибка чтения папки", "Could not read folder").into();
        }
        true
    });
    if !changed {
        return;
    }
    let (back, up) = BROWSER.with_borrow(|s| {
        let b = s.as_ref().unwrap();
        (!b.history.is_empty(), b.path.parent().is_some())
    });
    EnableWindow(GetDlgItem(h.popup, BACK as _), back as i32);
    EnableWindow(GetDlgItem(h.popup, UP as _), up as i32);
    put(h.items, LVM_SETITEMCOUNT, 0, 0);
    ShowWindow(h.items, SW_HIDE);
    InvalidateRect(h.popup, ptr::null(), 0);
}

unsafe fn poll(h: Windows) {
    let count = BROWSER.with_borrow_mut(|s| {
        let b = s.as_mut().unwrap();
        let mut count = None;
        while let Ok((id, listing)) = b.results.try_recv() {
            if id == b.generation.load(Relaxed) {
                count = Some(listing.entries.len());
                b.listing = listing;
            }
        }
        count
    });
    if let Some(count) = count {
        put(h.items, LVM_SETITEMCOUNT, count, 0);
        if count > 0 {
            let first = LVITEMW {
                stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                state: LVIS_SELECTED | LVIS_FOCUSED,
                ..zeroed()
            };
            put(h.items, LVM_SETITEMSTATE, 0, &first as *const _ as _);
            put(h.items, LVM_ENSUREVISIBLE, 0, 0);
            ShowWindow(h.items, SW_SHOWNOACTIVATE);
            if GetForegroundWindow() == h.popup {
                SetFocus(h.items);
            }
        }
        InvalidateRect(h.popup, ptr::null(), 0);
    }
}

pub unsafe fn timer(list: HWND, id: usize) -> bool {
    if id != DWELL && id != WATCH {
        return false;
    }
    let Some(h) = windows() else { return true };
    let Some((index, path)) = BROWSER.with_borrow(|s| s.as_ref()?.anchor.clone()) else {
        return true;
    };
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let foreground = GetForegroundWindow();
    let at = hit(list, pt);
    let anchor = at == index as isize && row_path(at).as_ref() == Some(&path);
    if foreground != GetAncestor(list, GA_ROOT) && foreground != h.popup {
        cancel(list);
        return true;
    }
    if id == DWELL {
        KillTimer(list, DWELL);
        if !anchor {
            cancel(list);
            return true;
        }
        let grace = delay(
            windows_sys::w!("FolderPreviewDismiss"),
            winrust_common::settings::FOLDER_PREVIEW_DISMISS_MS,
        );
        BROWSER.with_borrow_mut(|s| s.as_mut().unwrap().dismiss_after = Duration::from_millis(grace as u64));
        navigate(h, Move::Start(path));
        position(h, pt);
        SetTimer(list, WATCH, 50, None);
    } else {
        let inside = GetAncestor(WindowFromPoint(pt), GA_ROOT) == h.popup;
        let expired = BROWSER.with_borrow_mut(|s| {
            let b = s.as_mut().unwrap();
            if inside || anchor || foreground == h.popup {
                b.away = None;
                false
            } else {
                b.away.get_or_insert_with(Instant::now).elapsed() >= b.dismiss_after
            }
        });
        if expired {
            cancel(list);
            if at >= 0 {
                moved(list, at);
            }
            return true;
        }
        poll(h);
    }
    true
}

unsafe fn open_item(h: Windows, index: isize) {
    let item = BROWSER.with_borrow(|s| {
        let b = s.as_ref()?;
        let e = b.listing.entries.get(index as usize)?;
        Some((b.path.join(&e.name), e.folder))
    });
    let Some((path, folder)) = item else { return };
    if folder {
        navigate(h, Move::Into(path));
    } else {
        cancel(h.source);
        use std::os::windows::ffi::OsStrExt;
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let result = ShellExecuteW(
            GetAncestor(h.source, GA_ROOT),
            ptr::null(),
            filename.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        ) as isize;
        if result <= 32 {
            MessageBoxW(
                GetAncestor(h.source, GA_ROOT),
                w(&path.to_string_lossy()).as_ptr(),
                w(winrust_common::t!("Не удалось открыть файл", "Could not open file")).as_ptr(),
                MB_ICONERROR,
            );
        }
    }
}

unsafe fn display_item(item: &mut LVITEMW) {
    BROWSER.with_borrow(|s| {
        let Some(b) = s.as_ref() else {
            return;
        };
        let Some(e) = b.listing.entries.get(item.iItem as usize) else {
            return;
        };
        if item.mask & LVIF_TEXT != 0 && item.cchTextMax > 0 && !item.pszText.is_null() {
            let text = if item.iSubItem == 0 {
                e.name.to_string_lossy().into_owned()
            } else if e.folder {
                String::new()
            } else {
                size_text(e.size)
            };
            let dest = std::slice::from_raw_parts_mut(item.pszText, item.cchTextMax as usize);
            let mut n = 0;
            for ch in text.encode_utf16().take(dest.len() - 1) {
                dest[n] = ch;
                n += 1;
            }
            dest[n] = 0;
        }
        if item.mask & LVIF_IMAGE != 0 {
            item.iImage = e.icon;
        }
    });
}

unsafe fn rect(x: i32, y: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right,
        bottom,
    }
}
unsafe fn fill(dc: HDC, r: RECT, color: u32) {
    SetDCBrushColor(dc, color);
    FillRect(dc, &r, GetStockObject(DC_BRUSH) as _);
}
unsafe fn text(dc: HDC, font: isize, color: u32, mut r: RECT, value: &str, flags: u32) {
    let old = SelectObject(dc, font as _);
    SetTextColor(dc, color);
    SetBkMode(dc, TRANSPARENT as i32);
    DrawTextW(dc, w(value).as_ptr(), -1, &mut r, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}

unsafe fn layout(h: Windows) {
    let mut r: RECT = zeroed();
    GetClientRect(h.popup, &mut r);
    let sc = ui::sc;
    let button = sc(28);
    for (id, x) in [(BACK, sc(8)), (UP, sc(38)), (CLOSE, r.right - sc(36))] {
        MoveWindow(GetDlgItem(h.popup, id as _), x, sc(14), button, button, 1);
    }
    let width = (r.right - 2).max(1);
    MoveWindow(h.items, 1, sc(62), width, (r.bottom - sc(92)).max(1), 1);
    let size = sc(73);
    put(
        h.items,
        LVM_SETCOLUMNWIDTH,
        0,
        (width - size - GetSystemMetrics(SM_CXVSCROLL) - sc(4)).max(1) as _,
    );
    put(h.items, LVM_SETCOLUMNWIDTH, 1, size as _);
}

unsafe fn paint(h: Windows) {
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(h.popup, &mut ps);
    let mut r: RECT = zeroed();
    GetClientRect(h.popup, &mut r);
    let sc = ui::sc;
    let p = ui::pal();
    fill(dc, r, p.line);
    fill(dc, rect(1, 1, r.right - 1, r.bottom - 1), p.bg);
    fill(dc, rect(1, sc(61), r.right - 1, r.bottom - sc(30)), ui::listbg());
    fill(dc, rect(1, sc(60), r.right - 1, sc(61)), p.line);
    fill(dc, rect(1, r.bottom - sc(30), r.right - 1, r.bottom - sc(30) + 1), p.line);
    let (path, n, dirs, note, fonts) = BROWSER.with_borrow(|s| {
        let b = s.as_ref().unwrap();
        (
            b.path.clone(),
            b.listing.entries.len(),
            b.listing.folders,
            b.listing.note.clone(),
            b.fonts,
        )
    });
    let title = label(&path);
    let left = sc(77);
    let right = r.right - sc(45);
    text(
        dc,
        fonts[1],
        p.text,
        rect(left, sc(6), right, sc(32)),
        &title,
        DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    text(
        dc,
        fonts[2],
        p.dim,
        rect(left, sc(32), right, sc(53)),
        &path.to_string_lossy(),
        DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS,
    );
    if n == 0 {
        let mut box_ = rect(sc(20), 0, r.right - sc(20), 0);
        let old = SelectObject(dc, fonts[0] as _);
        DrawTextW(dc, w(&note).as_ptr(), -1, &mut box_, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, old);
        let y = (r.bottom + sc(30) - (box_.bottom - box_.top)) / 2;
        text(
            dc,
            fonts[0],
            p.dim,
            rect(sc(20), y, r.right - sc(20), r.bottom - sc(35)),
            &note,
            DT_CENTER | DT_WORDBREAK,
        );
    }
    if n > 0 {
        let count = if !note.is_empty() {
            note.replace('\n', " ")
        } else if winrust_common::english() {
            format!(
                "{dirs} folder{}  ·  {} file{}",
                if dirs == 1 { "" } else { "s" },
                n - dirs,
                if n - dirs == 1 { "" } else { "s" }
            )
        } else {
            format!("Папок: {dirs}  ·  Файлов: {}", n - dirs)
        };
        text(
            dc,
            fonts[2],
            p.dim,
            rect(sc(12), r.bottom - sc(29), r.right / 2 + sc(12), r.bottom - 1),
            &count,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        text(
            dc,
            fonts[2],
            p.dim,
            rect(r.right / 2 + sc(14), r.bottom - sc(29), r.right - sc(12), r.bottom - 1),
            winrust_common::t!("Двойной щелчок — открыть", "Double-click to open"),
            DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS,
        );
    }
    EndPaint(h.popup, &ps);
}

unsafe fn paint_button(d: &DRAWITEMSTRUCT) {
    let p = ui::pal();
    let disabled = d.itemState & ODS_DISABLED != 0;
    let hot = d.itemState & (ODS_SELECTED | ODS_FOCUS | ODS_HOTLIGHT) != 0;
    fill(d.hDC, d.rcItem, if hot && !disabled { p.hover } else { p.bg });
    let glyph = match d.CtlID as usize {
        BACK => "\u{E72B}",
        UP => "\u{E74A}",
        _ => "\u{E8BB}",
    };
    let font = BROWSER.with_borrow(|s| s.as_ref().unwrap().fonts[3]);
    text(
        d.hDC,
        font,
        if disabled { ui::off() } else { p.text },
        d.rcItem,
        glyph,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );
}

pub unsafe fn key(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let Some(h) = windows() else { return false };
    if IsWindowVisible(h.popup) == 0 {
        return false;
    }
    let vk = msg.wParam as u16;
    if vk == VK_ESCAPE {
        cancel(h.source);
        return true;
    }
    if GetAncestor(msg.hwnd, GA_ROOT) != h.popup {
        return false;
    }
    match vk {
        VK_RETURN if msg.hwnd == h.items => {
            let index = put(h.items, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as _);
            open_item(h, index);
        }
        VK_BACK => navigate(h, Move::Back),
        VK_TAB => {
            let next = GetNextDlgTabItem(h.popup, GetFocus(), (GetKeyState(VK_SHIFT as i32) < 0) as i32);
            if !next.is_null() {
                SetFocus(next);
            }
        }
        _ => return false,
    }
    true
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(h) = windows().filter(|h| h.popup == hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(h);
            0
        }
        WM_SIZE => {
            layout(h);
            0
        }
        WM_DRAWITEM if lp != 0 => {
            paint_button(&*(lp as *const DRAWITEMSTRUCT));
            1
        }
        WM_CLOSE => {
            cancel(h.source);
            0
        }
        WM_COMMAND => {
            match wp & 0xffff {
                BACK => navigate(h, Move::Back),
                UP => navigate(h, Move::Up),
                CLOSE => cancel(h.source),
                _ => {}
            }
            0
        }
        WM_NAV => {
            match wp {
                1 => navigate(h, Move::Back),
                2 => navigate(h, Move::Forward),
                4 => navigate(h, Move::Up),
                _ => {}
            };
            0
        }
        WM_NOTIFY => {
            let hdr = &*(lp as *const NMHDR);
            if hdr.hwndFrom == h.items {
                match hdr.code {
                    LVN_GETDISPINFOW => display_item(&mut (*(lp as *mut NMLVDISPINFOW)).item),
                    NM_DBLCLK => {
                        let index = (*(lp as *const NMITEMACTIVATE)).iItem;
                        if index >= 0 {
                            let id = BROWSER.with_borrow(|s| s.as_ref().unwrap().generation.load(Relaxed));
                            PostMessageW(hwnd, ACTIVATE, index as _, id as isize);
                        }
                    }
                    _ => {}
                }
            }
            0
        }
        ACTIVATE => {
            if IsWindowVisible(hwnd) != 0 && BROWSER.with_borrow(|s| s.as_ref().unwrap().generation.load(Relaxed)) == lp as u64 {
                open_item(h, wp as isize);
            }
            0
        }
        m if m == winrust_common::style::msg() => {
            winrust_common::style::reload();
            let old = BROWSER.with_borrow_mut(|s| std::mem::replace(&mut s.as_mut().unwrap().fonts, font_set()));
            restyle(h);
            for font in old {
                DeleteObject(font as _);
            }
            RedrawWindow(hwnd, ptr::null(), ptr::null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub unsafe fn destroy(list: HWND) {
    cancel(list);
    if let Some(b) = BROWSER.take() {
        DestroyWindow(b.windows.popup);
        for font in b.fonts {
            DeleteObject(font as _);
        }
    }
}
