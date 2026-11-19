use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};
use windows_sys::Win32::System::Registry::*;

const KEY: &str = r"Software\WinRust\Topbar";

fn boot_id() -> u64 {
    let mut boot = 0u32;
    let mut size = 4;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters").as_ptr(),
            wide("BootId").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut boot as *mut _ as _,
            &mut size,
        )
    };
    if status == ERROR_SUCCESS {
        return boot as u64;
    }
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    epoch.saturating_sub(unsafe { GetTickCount64() }).div_ceil(1000)
}

fn read_order(name: &str) -> Vec<String> {
    let key = wide(KEY);
    let name = wide(name);
    let mut size = 0;
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_MULTI_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        ) != ERROR_SUCCESS
            || size > 1_048_576
        {
            return Vec::new();
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_MULTI_SZ,
            null_mut(),
            buf.as_mut_ptr() as _,
            &mut size,
        ) != ERROR_SUCCESS
        {
            return Vec::new();
        }
        buf.split(|&c| c == 0)
            .take_while(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

struct Order {
    boot: u64,
    manual: Vec<String>,
    shuffled: Vec<String>,
    timed: Vec<String>,
}
impl Order {
    fn load() -> Self {
        let boot = boot_id();
        let mut shuffled = read_order("CurrentBootIconOrder").into_iter();
        let same = shuffled.next().is_some_and(|s| s == boot.to_string());
        Self {
            boot,
            timed: Vec::new(),
            manual: read_order("IconOrder"),
            shuffled: if same { shuffled.collect() } else { Vec::new() },
        }
    }
}

thread_local! {
    static ORDERING: RefCell<Order> = RefCell::new(Order::load());
    static PRESS: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

pub(super) const SHUFFLE_TIMER: usize = 8;

pub(super) unsafe fn schedule(bar: HWND) {
    KillTimer(bar, SHUFFLE_TIMER);
    if settings::preferences().timed_shuffle {
        SetTimer(bar, SHUFFLE_TIMER, Timing::Shuffle.ms(), None);
    } else {
        ORDERING.with_borrow_mut(|o| o.timed.clear());
    }
}

pub(super) unsafe fn reshuffle(bar: HWND) {
    if pending() || !settings::preferences().timed_shuffle {
        return;
    }
    let mut keys = BTNS.with_borrow(|buttons| buttons.iter().skip(1).map(|b| b.key.clone()).collect::<Vec<_>>());
    if keys.len() < 2 {
        return;
    }
    let previous = keys.clone();
    let seed = GetTickCount64();
    keys.sort_by_cached_key(|key| {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        key.hash(&mut h);
        h.finish()
    });
    if keys == previous {
        keys.rotate_left(1);
    }
    ORDERING.with_borrow_mut(|o| o.timed = keys);
    hide_tip(bar);
    previews::hide();
    HOT.set(NONE);
    refresh_buttons();
    InvalidateRect(bar, null(), 0);
}

pub(super) fn apply(buttons: &mut [Btn]) {
    let shuffle = settings::preferences().shuffle;
    ORDERING.with_borrow(|o| {
        let list = if settings::preferences().timed_shuffle && !o.timed.is_empty() {
            &o.timed
        } else if shuffle {
            &o.shuffled
        } else {
            &o.manual
        };
        if !shuffle && list.is_empty() {
            return;
        }
        buttons.sort_by_cached_key(|b| {
            let rank = list.iter().position(|k| k == &b.key).unwrap_or(usize::MAX);
            let tie = if shuffle {
                let mut hash = DefaultHasher::new();
                o.boot.hash(&mut hash);
                b.key.hash(&mut hash);
                hash.finish()
            } else {
                0
            };
            (rank, tie)
        });
    });
}

struct Drag {
    key: String,
    origin: POINT,
    dismissed_preview: bool,
    moving: bool,
    target: Option<String>,
}

pub(super) fn pending() -> bool {
    PRESS.with_borrow(|p| p.is_some())
}
pub(super) fn marker() -> Option<String> {
    PRESS.with_borrow(|p| p.as_ref().and_then(|d| d.target.clone()))
}

pub(super) unsafe fn begin(bar: HWND, button: i32, point: POINT, dismissed_preview: bool) {
    let key = BTNS.with_borrow(|bs| bs.get(button as usize).map(|b| b.key.clone()));
    if let Some(key) = key {
        PRESS.replace(Some(Drag {
            key,
            origin: point,
            dismissed_preview,
            moving: false,
            target: None,
        }));
        SetCapture(bar);
    }
}

// A release in the narrow gap between buttons lands on the nearest app.
unsafe fn drop_index(bar: HWND, x: i32) -> i32 {
    let direct = hit(bar, x);
    if direct != NONE {
        return direct;
    }
    let l = bar_layout(bar);
    let distance = x - l.button_x(1);
    if distance < 0 || x >= l.apps.0 {
        return NONE;
    }
    let index = 1 + (distance + l.gap / 2) / (l.bw + l.gap);
    if BTNS.with_borrow(|buttons| (index as usize) < buttons.len()) {
        index
    } else {
        NONE
    }
}

pub(super) unsafe fn motion(bar: HWND, point: POINT) {
    let target = drop_index(bar, edge::axis(point));
    PRESS.with_borrow_mut(|p| {
        let Some(d) = p else { return };
        if !d.moving
            && (point.x - d.origin.x).abs() < GetSystemMetrics(SM_CXDRAG).max(3)
            && (point.y - d.origin.y).abs() < GetSystemMetrics(SM_CYDRAG).max(3)
        {
            return;
        }
        d.moving = true;
        let key = BTNS.with_borrow(|bs| {
            bs.get(target as usize)
                .filter(|b| target > 0 && b.key != d.key)
                .map(|b| b.key.clone())
        });
        if key != d.target {
            d.target = key;
            InvalidateRect(bar, null(), 0);
        }
    });
}

pub(super) fn cancel() {
    PRESS.take();
}

pub(super) unsafe fn finish(bar: HWND, point: POINT) {
    let Some(d) = PRESS.take() else { return };
    ReleaseCapture();
    let mut bounds: RECT = zeroed();
    GetClientRect(bar, &mut bounds);
    if PtInRect(&bounds, point) == 0 {
        return;
    }
    let target = if d.moving {
        drop_index(bar, edge::axis(point))
    } else {
        hit(bar, edge::axis(point))
    };
    let mut keys = BTNS.with_borrow(|bs| bs.iter().skip(1).map(|b| b.key.clone()).collect::<Vec<_>>());
    let Some(from) = keys.iter().position(|k| k == &d.key) else {
        return;
    };
    if target < 1 || target as usize > keys.len() {
        return;
    }
    let to = target as usize - 1;
    if !d.moving {
        if from == to && !d.dismissed_preview && !(settings::preferences().click_previews && previews::show(target)) {
            click(target, true, false);
        }
        return;
    }
    if from == to {
        return;
    }
    let key = keys.remove(from);
    keys.insert(to, key);
    let err = ORDERING.with_borrow_mut(|o| {
        let shuffle = settings::preferences().shuffle;
        if settings::preferences().timed_shuffle {
            o.timed = keys;
            return ERROR_SUCCESS;
        }
        let name = if shuffle { "CurrentBootIconOrder" } else { "IconOrder" };
        let mut data = Vec::new();
        if shuffle {
            data.extend(wide(&o.boot.to_string()));
        }
        for k in &keys {
            data.extend(wide(k));
        }
        data.push(0);
        let err = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            wide(name).as_ptr(),
            REG_MULTI_SZ,
            data.as_ptr() as _,
            (data.len() * 2) as u32,
        );
        if err == ERROR_SUCCESS {
            if shuffle {
                o.shuffled = keys;
            } else {
                o.manual = keys;
            }
        }
        err
    });
    if err == ERROR_SUCCESS {
        refresh_buttons();
    } else {
        MessageBoxW(
            bar,
            wide(&std::io::Error::from_raw_os_error(err as i32).to_string()).as_ptr(),
            wide(t!("Не удалось сохранить порядок", "Could not save icon order")).as_ptr(),
            MB_ICONERROR,
        );
    }
}
