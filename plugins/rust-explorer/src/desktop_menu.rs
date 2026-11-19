#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use std::cell::Cell;
use windows::Win32::UI::{Controls::IImageList, Shell::SHGetImageList as image_list};
use windows::core::Interface;
use windows_sys::Win32::System::Registry::*;

thread_local! {
    static VIEW: Cell<u32> = Cell::new(read());
    static REARRANGE: Cell<bool> = const { Cell::new(false) };
    static IMAGES: RefCell<Option<IImageList>> = const { RefCell::new(None) };
}

const KEY: &str = r"Software\WinRust\Desktop";
const AUTO: u32 = 4;
const HIDDEN: u32 = 8;
const DESCENDING: u32 = 64;
const FREE_POSITION: u32 = 128;

fn read() -> u32 {
    let mut value = 0u32;
    let mut len = 4;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w(KEY).as_ptr(),
            w("ViewV1").as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            &mut value as *mut _ as _,
            &mut len,
        )
    };
    if err == 0 && value & 3 < 3 { value } else { 1 }
}

fn set(h: HWND, bits: u32) {
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            w(KEY).as_ptr(),
            w("ViewV1").as_ptr(),
            REG_DWORD,
            &bits as *const _ as _,
            4,
        )
    };
    if err != 0 {
        error(h, &std::io::Error::from_raw_os_error(err as i32).to_string());
        return;
    }
    VIEW.set(bits);
}
fn error(h: HWND, msg: &str) {
    unsafe {
        MessageBoxW(h, w(msg).as_ptr(), w("Desktop").as_ptr(), MB_ICONERROR);
    }
}
pub(super) fn auto_arrange() -> bool {
    VIEW.get() & AUTO != 0
}
pub(super) fn free_position() -> bool {
    VIEW.get() & FREE_POSITION != 0 && !auto_arrange()
}
pub(super) fn arrange() -> bool {
    REARRANGE.replace(false) || auto_arrange()
}

pub(super) unsafe fn layout_icons(list: HWND) {
    let mut work: RECT = std::mem::zeroed();
    if !list.is_null() && SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0) != 0 {
        let y = work.top + desk_top();
        MoveWindow(
            list,
            work.left,
            y,
            (work.right - work.left).max(0),
            (work.bottom - y).max(0),
            1,
        );
    }
}

pub(super) unsafe fn init(list: HWND) {
    let bits = VIEW.get();
    SORT_COL.store(((bits >> 4) & 3) as usize, Relaxed);
    SORT_ASC.store(bits & DESCENDING == 0, Relaxed);
    let size = (bits & 3) as usize;
    let sizes = [(SHIL_SMALL, 72, 72), (SHIL_LARGE, 88, 96), (SHIL_EXTRALARGE, 104, 110)];
    let (kind, cx, cy) = sizes[size];
    if let Ok(images) = image_list::<IImageList>(kind as i32) {
        put(list, LVM_SETIMAGELIST, LVSIL_NORMAL as usize, images.as_raw() as _);
        IMAGES.replace(Some(images));
    }
    put(
        list,
        LVM_SETICONSPACING,
        0,
        ((ui::sc(cy) as isize) << 16) | ui::sc(cx) as isize,
    );
    ShowWindow(list, if bits & HIDDEN == 0 { SW_SHOWNA } else { SW_HIDE });
}

unsafe fn item(m: HMENU, id: usize, label: &str, flags: u32) {
    AppendMenuW(m, flags | MF_STRING, id, w(label).as_ptr());
}
unsafe fn sep(m: HMENU) {
    AppendMenuW(m, MF_SEPARATOR, 0, ptr::null());
}
fn checked(on: bool) -> u32 {
    if on { MF_CHECKED } else { 0 }
}
unsafe fn open_settings(h: HWND, uri: &str) {
    if ShellExecuteW(h, ptr::null(), w(uri).as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL) as isize <= 32 {
        error(h, "Windows could not open Settings.");
    }
}

pub(super) unsafe fn show(h: HWND) {
    let m = CreatePopupMenu();
    let bits = VIEW.get();
    let view = CreatePopupMenu();
    for (id, name) in [(12, "Large icons"), (11, "Medium icons"), (10, "Small icons")] {
        item(view, id, name, checked((id - 10) as u32 == bits & 3));
    }
    sep(view);
    item(view, 13, "Auto arrange icons", checked(auto_arrange()));
    item(
        view,
        15,
        "Align icons to grid",
        checked(bits & FREE_POSITION == 0) | if auto_arrange() { MF_GRAYED } else { 0 },
    );
    item(view, 14, "Show desktop icons", checked(bits & HIDDEN == 0));
    item(m, view as usize, "View", MF_POPUP);
    let sort = CreatePopupMenu();
    for (i, label) in ["Name", "Date modified", "Item type", "Size"].iter().enumerate() {
        item(sort, 20 + i, label, checked(i == SORT_COL.load(Relaxed)));
    }
    sep(sort);
    item(sort, 24, "Ascending", checked(SORT_ASC.load(Relaxed)));
    item(sort, 25, "Descending", checked(!SORT_ASC.load(Relaxed)));
    item(m, sort as usize, "Sort by", MF_POPUP);
    item(m, 4, "Refresh", 0);
    sep(m);
    let can_paste = sh::clip_get().is_some();
    item(m, 3, "Paste", if can_paste { 0 } else { MF_GRAYED });
    item(m, 7, "Paste shortcut", if can_paste { 0 } else { MF_GRAYED });
    let new = CreatePopupMenu();
    item(new, 1, "Folder", 0);
    item(new, 2, "Text document", 0);
    item(m, new as usize, "New", MF_POPUP);
    sep(m);
    item(m, 30, "Display settings", 0);
    item(m, 31, "Personalize", 0);
    item(m, 32, "Change desktop background", 0);
    sep(m);
    item(m, 33, "Open in PowerShell", 0);
    item(m, 5, "Desktop folder properties", 0);
    let disk = tool("wiztree");
    if disk.is_some() {
        item(m, 6, "Disk usage (WinRust Disk)", 0);
    }
    item(m, 34, "More Windows options...", 0);
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    let id = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, h, ptr::null());
    DestroyMenu(m);
    let dir = cur_dir();
    match id {
        0 => return,
        1 => new_entry(&dir, "New folder", true),
        2 => new_entry(&dir, "New text document.txt", false),
        3 => paste(h, &dir),
        4 => {
            InvalidateRect(h, ptr::null(), 1);
        }
        5 => {
            properties(&dir);
            return;
        }
        6 => {
            if let Some(exe) = disk {
                let _ = std::process::Command::new(exe).arg(&dir).spawn();
            }
            return;
        }
        7 => {
            if let Some((paths, _)) = sh::clip_get() {
                if let Err(e) = sh::paste_shortcuts(&dir, &paths) {
                    error(h, &e.to_string());
                }
            }
        }
        10..=12 | 13 | 14 | 15 => {
            let next = match id {
                13 => bits ^ AUTO,
                14 => bits ^ HIDDEN,
                15 => bits ^ FREE_POSITION,
                _ => (bits & !3) | (id - 10) as u32,
            };
            set(h, next);
            init(ST.with_borrow(|st| st.list));
            if id == 14 {
                InvalidateRect(h, ptr::null(), 1);
                return;
            }
        }
        20..=25 => {
            let next = match id {
                24 => bits & !DESCENDING,
                25 => bits | DESCENDING,
                _ => (bits & !48) | ((id - 20) as u32) << 4,
            };
            set(h, next);
            SORT_COL.store(((VIEW.get() >> 4) & 3) as usize, Relaxed);
            SORT_ASC.store(VIEW.get() & DESCENDING == 0, Relaxed);
            REARRANGE.set(true);
        }
        30 => {
            open_settings(h, "ms-settings:display");
            return;
        }
        31 => {
            open_settings(h, "ms-settings:personalization");
            return;
        }
        32 => {
            open_settings(h, "ms-settings:personalization-background");
            return;
        }
        33 => {
            if let Err(e) = std::process::Command::new("powershell.exe").current_dir(&dir).spawn() {
                error(h, &e.to_string());
            }
            return;
        }
        34 => {
            if let Err(e) = sh::background(h) {
                error(h, &e.to_string());
            }
        }
        _ => return,
    }
    reload(h);
}
