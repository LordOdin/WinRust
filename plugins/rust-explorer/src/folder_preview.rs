//! A delayed tracking tooltip for the file list. One reader per Explorer window keeps slow UNC paths off the UI thread.
use super::*;
use std::{
    mem::{size_of, zeroed},
    sync::{
        Arc,
        atomic::AtomicU64,
        mpsc::{self, Receiver, Sender},
    },
};
use windows_sys::Win32::{Graphics::Gdi::ScreenToClient, System::Registry::*};

const DWELL: usize = 0x7241;
const WATCH: usize = 0x7242;
const LIMIT: usize = 12;
type Request = (u64, PathBuf, bool);

struct Preview {
    generation: Arc<AtomicU64>,
    send: Sender<Request>,
    results: Receiver<(u64, String)>,
    target: Option<(usize, PathBuf)>,
    tip: HWND,
    text: Vec<u16>,
}
thread_local! { static PREVIEW: RefCell<Option<Preview>> = const {RefCell::new(None)}; }

fn contents(path: &Path, hidden: bool, current: impl Fn() -> bool) -> Option<String> {
    let mut rows = Vec::new();
    let listing = fs::read_dir(path).map_err(|e| e.to_string()).and_then(|entries| {
        for entry in entries {
            if !current() {
                break;
            }
            let entry = entry.map_err(|e| e.to_string())?;
            if !hidden && entry.metadata().is_ok_and(|m| m.file_attributes() & 2 != 0) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().replace(['\r', '\n'], " ");
            rows.push((!entry.file_type().is_ok_and(|t| t.is_dir()), name));
            if rows.len() > LIMIT {
                break;
            }
        }
        Ok(())
    });
    if !current() {
        return None;
    }
    if let Err(why) = listing {
        return Some(format!(
            "{}\n{why}",
            winrust_common::t!("Не удалось прочитать папку.", "Could not read folder.")
        ));
    }
    if rows.is_empty() {
        return Some(winrust_common::t!("Пустая папка", "Empty folder").into());
    }
    let more = rows.len() > LIMIT;
    rows.truncate(LIMIT);
    rows.sort_by_cached_key(|(file, name)| (*file, name.to_lowercase()));
    let mut lines: Vec<_> = rows
        .into_iter()
        .map(|(file, name)| if file { name } else { format!("{name}\\") })
        .collect();
    if more {
        lines.push(winrust_common::t!("… и другие", "… and more").into());
    }
    Some(lines.join("\n"))
}

unsafe fn tool(list: HWND) -> TTTOOLINFOW {
    TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_TRACK | TTF_ABSOLUTE | TTF_TRANSPARENT,
        hwnd: list,
        uId: 1,
        ..unsafe { zeroed() }
    }
}

impl Preview {
    unsafe fn new(list: HWND) -> Option<Self> {
        let (send, requests) = mpsc::channel::<Request>();
        let (done, results) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let epoch = generation.clone();
        std::thread::Builder::new()
            .name("folder preview".into())
            .spawn(move || {
                while let Ok(first) = requests.recv() {
                    let (id, path, hidden) = requests.try_iter().last().unwrap_or(first);
                    let current = || epoch.load(Relaxed) == id;
                    if current()
                        && let Some(text) = contents(&path, hidden, current)
                    {
                        let _ = done.send((id, text));
                    }
                }
            })
            .ok()?;
        unsafe {
            let tip = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                TOOLTIPS_CLASSW,
                ptr::null(),
                WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
                0,
                0,
                0,
                0,
                list,
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            if tip.is_null() {
                return None;
            }
            let mut text = w("");
            let mut ti = tool(list);
            ti.lpszText = text.as_mut_ptr();
            if put(tip, TTM_ADDTOOLW, 0, &ti as *const _ as _) == 0 {
                DestroyWindow(tip);
                return None;
            }
            put(tip, TTM_SETMAXTIPWIDTH, 0, ui::sc(390) as _);
            Some(Self {
                generation,
                send,
                results,
                target: None,
                tip,
                text,
            })
        }
    }

    unsafe fn hide(&mut self, list: HWND) {
        self.generation.fetch_add(1, Relaxed);
        self.target = None;
        unsafe {
            KillTimer(list, DWELL);
            KillTimer(list, WATCH);
            put(self.tip, TTM_TRACKACTIVATE, 0, &tool(list) as *const _ as _);
        }
    }
}

pub fn cancel(list: HWND) {
    PREVIEW.with_borrow_mut(|s| {
        if let Some(s) = s
            && s.target.is_some()
        {
            unsafe {
                s.hide(list);
            }
        }
    });
}

fn row_path(index: isize) -> Option<PathBuf> {
    ST.with(|cell| {
        let st = cell.try_borrow().ok()?;
        let row = st.rows.get(index as usize)?;
        row.dir.then(|| st.dir.join(&row.name))
    })
}

fn delay() -> u32 {
    let mut value = 0u32;
    let mut size = 4;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows_sys::w!(r"Software\WinRust\Topbar\Timing"),
            windows_sys::w!("FolderPreview"),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    if result == ERROR_SUCCESS && (1..=86_400_000).contains(&value) {
        value
    } else {
        450
    }
}

pub unsafe fn moved(list: HWND, index: isize) {
    let path = row_path(index);
    PREVIEW.with_borrow_mut(|state| unsafe {
        if state.is_none() && path.is_some() {
            *state = Preview::new(list);
        }
        let Some(s) = state else { return };
        if s.target
            .as_ref()
            .is_some_and(|(row, p)| *row == index as usize && Some(p) == path.as_ref())
        {
            return;
        }
        if s.target.is_some() {
            s.hide(list);
        }
        if let Some(path) = path {
            s.target = Some((index as usize, path));
            SetTimer(list, DWELL, delay(), None);
        }
    });
}

pub unsafe fn timer(list: HWND, id: usize) -> bool {
    if id != DWELL && id != WATCH {
        return false;
    }
    PREVIEW.with_borrow_mut(|state| unsafe {
        let Some(s) = state else { return };
        let Some((index, path)) = &s.target else { return };
        let mut pt: POINT = zeroed();
        GetCursorPos(&mut pt);
        let mut hit: LVHITTESTINFO = zeroed();
        hit.pt = pt;
        ScreenToClient(list, &mut hit.pt);
        let row = put(list, LVM_HITTEST, 0, &mut hit as *mut _ as _);
        if row != *index as isize
            || row_path(row).as_ref() != Some(path)
            || WindowFromPoint(pt) != list
            || GetAncestor(list, GA_ROOT) != GetForegroundWindow()
        {
            s.hide(list);
            return;
        }
        let generation = s.generation.load(Relaxed);
        if id == DWELL {
            KillTimer(list, DWELL);
            let _ = s.send.send((generation, path.clone(), SHOW_HIDDEN.load(Relaxed)));
            SetTimer(list, WATCH, 100, None);
        }
        while let Ok((done, text)) = s.results.try_recv() {
            if done != generation {
                continue;
            }
            s.text = w(&text);
            let mut ti = tool(list);
            ti.lpszText = s.text.as_mut_ptr();
            put(s.tip, TTM_UPDATETIPTEXTW, 0, &ti as *const _ as _);
            let xy = |x: i32, y: i32| ((x as u16 as u32) | (y as u16 as u32) << 16) as isize;
            put(s.tip, TTM_TRACKPOSITION, 0, xy(pt.x + ui::sc(14), pt.y + ui::sc(18)));
            put(s.tip, TTM_TRACKACTIVATE, 1, &ti as *const _ as _);
            let mut bounds: RECT = zeroed();
            GetWindowRect(s.tip, &mut bounds);
            use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..zeroed()
            };
            if GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut monitor) != 0 {
                let area = monitor.rcWork;
                let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
                let x = (pt.x + ui::sc(14)).min(area.right - width).max(area.left);
                let below = pt.y + ui::sc(18);
                let y = if below + height <= area.bottom {
                    below
                } else {
                    (pt.y - height - ui::sc(6)).max(area.top)
                };
                put(s.tip, TTM_TRACKPOSITION, 0, xy(x, y));
            }
        }
    });
    true
}

pub unsafe fn destroy(list: HWND) {
    let preview = PREVIEW.take();
    if let Some(mut s) = preview {
        unsafe {
            s.hide(list);
            DestroyWindow(s.tip);
        }
    }
}
