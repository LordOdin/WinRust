use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    fn value(self, r: &mut RECT) -> &mut i32 {
        match self {
            Self::Left => &mut r.left,
            Self::Right => &mut r.right,
            Self::Top => &mut r.top,
            Self::Bottom => &mut r.bottom,
        }
    }
}

// Release only an inset that coincides with a hidden Windows taskbar.
// Preserve toolbars on the other edges and screens with no matching tray.
fn tray_edge(screen: RECT, work: RECT, tray: RECT) -> Option<Edge> {
    if tray.left == screen.left && tray.right == screen.right {
        if tray.bottom == screen.bottom && tray.top == work.bottom && work.bottom < screen.bottom {
            return Some(Edge::Bottom);
        }
        if tray.top == screen.top && tray.bottom == work.top && work.top > screen.top {
            return Some(Edge::Top);
        }
    }
    if tray.top == screen.top && tray.bottom == screen.bottom {
        if tray.left == screen.left && tray.right == work.left && work.left > screen.left {
            return Some(Edge::Left);
        }
        if tray.right == screen.right && tray.left == work.right && work.right < screen.right {
            return Some(Edge::Right);
        }
    }
    None
}

thread_local! {
    static RELEASED: RefCell<HashMap<usize, (Edge, i32, i32)>> = RefCell::new(HashMap::new());
}

unsafe fn change_area(mut area: RECT) -> bool {
    SystemParametersInfoW(SPI_SETWORKAREA, 0, &mut area as *mut _ as _, SPIF_SENDCHANGE) != 0
}

pub(super) unsafe fn secondary_trays() -> Vec<HWND> {
    let class = wide("Shell_SecondaryTrayWnd");
    let mut trays = Vec::new();
    let mut h = null_mut();
    loop {
        h = FindWindowExW(null_mut(), h, class.as_ptr(), null());
        if h.is_null() {
            return trays;
        }
        trays.push(h);
    }
}

pub(super) unsafe fn reconcile(trays: &[HWND]) {
    if !settings::preferences().expand_secondary {
        return;
    }
    RELEASED.with_borrow_mut(|saved| saved.retain(|h, _| trays.iter().any(|t| *t as usize == *h)));
    for &tray in trays {
        if IsWindowVisible(tray) != 0 {
            continue;
        }
        let mut bounds: RECT = zeroed();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..zeroed()
        };
        if GetWindowRect(tray, &mut bounds) == 0
            || GetMonitorInfoW(MonitorFromWindow(tray, MONITOR_DEFAULTTONULL), &mut info) == 0
            || info.dwFlags & MONITORINFOF_PRIMARY != 0
        {
            continue;
        }
        let Some(edge) = tray_edge(info.rcMonitor, info.rcWork, bounds) else {
            continue;
        };
        let mut area = info.rcWork;
        let original = *edge.value(&mut area);
        let applied = *edge.value(&mut info.rcMonitor);
        *edge.value(&mut area) = applied;
        if change_area(area) {
            RELEASED.with_borrow_mut(|saved| saved.insert(tray as usize, (edge, original, applied)));
        }
    }
}

pub(super) unsafe fn restore() {
    let saved = RELEASED.take();
    for tray in secondary_trays() {
        let Some(&(edge, original, applied)) = saved.get(&(tray as usize)) else {
            continue;
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..zeroed()
        };
        let mut bounds: RECT = zeroed();
        if GetMonitorInfoW(MonitorFromWindow(tray, MONITOR_DEFAULTTONULL), &mut info) == 0
            || info.dwFlags & MONITORINFOF_PRIMARY != 0
            || GetWindowRect(tray, &mut bounds) == 0
            || *edge.value(&mut info.rcWork) != applied
        {
            continue;
        }
        *edge.value(&mut info.rcWork) = original;
        if tray_edge(info.rcMonitor, info.rcWork, bounds) == Some(edge) {
            change_area(info.rcWork);
        }
    }
}

pub(super) unsafe fn rows() -> Vec<[String; 4]> {
    unsafe extern "system" fn monitor(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        let mut info: MONITORINFOEXW = zeroed();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(h, &mut info.monitorInfo) != 0 {
            let mi = info.monitorInfo;
            let screen = mi.rcMonitor;
            let work = mi.rcWork;
            let name = String::from_utf16_lossy(
                &info.szDevice[..info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len())],
            );
            let label = name.rsplit('\\').next().unwrap_or(&name);
            let role = if mi.dwFlags & MONITORINFOF_PRIMARY != 0 {
                t!("Панель WinRust", "WinRust bar")
            } else {
                t!("Дополнительный", "Secondary")
            };
            (*(data as *mut Vec<[String; 4]>)).push([
                label.to_owned(),
                role.to_owned(),
                format!("{} × {}", screen.right - screen.left, screen.bottom - screen.top),
                format!("{} × {}", work.right - work.left, work.bottom - work.top),
            ]);
        }
        1
    }
    let mut lines: Vec<[String; 4]> = Vec::new();
    EnumDisplayMonitors(null_mut(), null(), Some(monitor), &mut lines as *mut _ as _);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }
    #[test]
    fn releases_only_the_matching_hidden_tray_edge() {
        let screen = r(-1920, -80, 0, 1000);
        let tray = r(-1920, 952, 0, 1000);
        assert_eq!(tray_edge(screen, r(-1800, -80, 0, 952), tray), Some(Edge::Bottom));
        assert_eq!(tray_edge(screen, r(-1920, -80, 0, 940), tray), None);
        assert_eq!(tray_edge(screen, screen, tray), None);
        assert_eq!(
            tray_edge(screen, r(-1880, -80, 0, 1000), r(-1920, -80, -1880, 1000)),
            Some(Edge::Left)
        );
    }
}
