use super::*;

pub(super) const EDGES: [u32; 4] = [ABE_BOTTOM, ABE_LEFT, ABE_TOP, ABE_RIGHT];

pub(super) fn current() -> u32 {
    EDGES[PREVIEW.get().unwrap_or_else(|| settings::preferences().edge_index)]
}
pub(super) fn vertical() -> bool {
    matches!(current(), ABE_LEFT | ABE_RIGHT)
}
pub(super) fn axis(point: POINT) -> i32 {
    if vertical() { point.y } else { point.x }
}
pub(super) fn thickness() -> i32 {
    if vertical() { bar_h() + sc(24) } else { bar_h() }
}

pub(super) unsafe fn screen() -> (i32, i32) {
    (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
}

pub(super) unsafe fn span() -> (i32, i32) {
    let (w, h) = screen();
    let full = if vertical() { h } else { w };
    let length = if settings::preferences().dock {
        (full * 3 / 5).max(sc(720)).min(full)
    } else {
        full
    };
    ((full - length) / 2, length)
}

pub(super) unsafe fn bounds(offset: i32) -> RECT {
    let (w, h) = screen();
    let (start, len) = span();
    let size = thickness();
    let inset = dock_gap() - offset;
    let (x, y, width, height) = match current() {
        ABE_LEFT => (inset, start, size, len),
        ABE_RIGHT => (w - inset - size, start, size, len),
        ABE_TOP => (start, inset, len, size),
        _ => (start, h - inset - size, len, size),
    };
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

pub(super) fn side(rect: &mut RECT, edge: u32) -> &mut i32 {
    match edge {
        ABE_TOP => &mut rect.top,
        ABE_LEFT => &mut rect.left,
        ABE_RIGHT => &mut rect.right,
        _ => &mut rect.bottom,
    }
}

pub(super) unsafe fn limit(edge: u32, strip: i32) -> i32 {
    let (w, h) = screen();
    match edge {
        ABE_LEFT | ABE_TOP => strip,
        ABE_RIGHT => w - strip,
        _ => h - strip,
    }
}

pub(super) unsafe fn popup(anchor: i32, w: i32, h: i32, gap: i32) -> (i32, i32) {
    let r = bounds(0);
    let (sw, sh) = screen();
    let (x, y) = match current() {
        ABE_LEFT => (r.right + gap, anchor - h / 2),
        ABE_RIGHT => (r.left - gap - w, anchor - h / 2),
        ABE_TOP => (anchor - w / 2, r.bottom + gap),
        _ => (anchor - w / 2, r.top - gap - h),
    };
    (x.clamp(0, (sw - w).max(0)), y.clamp(0, (sh - h).max(0)))
}

pub(super) unsafe fn reveal(point: POINT) -> bool {
    let (sw, sh) = screen();
    let (start, len) = span();
    (start..start + len).contains(&axis(point))
        && match current() {
            ABE_TOP => (0..2).contains(&point.y),
            ABE_LEFT => (0..2).contains(&point.x),
            ABE_RIGHT => (sw - 2..sw).contains(&point.x),
            _ => (sh - 2..sh).contains(&point.y),
        }
}

struct Drag {
    origin: POINT,
    gear: bool,
    moved: bool,
}
thread_local! { static PREVIEW: Cell<Option<usize>> = const { Cell::new(None) }; static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) }; }

pub(super) fn dragging() -> bool {
    DRAG.with_borrow(|d| d.is_some())
}
pub(super) unsafe fn cancel(bar: HWND) {
    DRAG.take();
    if PREVIEW.take().is_some() {
        place(bar);
    }
}

unsafe fn candidate(p: POINT) -> Option<usize> {
    let (w, h) = screen();
    if p.x < 0 || p.x >= w || p.y < 0 || p.y >= h {
        return None;
    }
    [h - 1 - p.y, p.x, p.y, w - 1 - p.x]
        .into_iter()
        .enumerate()
        .min_by_key(|(_, distance)| *distance)
        .and_then(|(index, distance)| (distance <= sc(96)).then_some(index))
}

pub(super) unsafe fn begin(bar: HWND, gear: bool) {
    let mut origin = zeroed();
    GetCursorPos(&mut origin);
    DRAG.replace(Some(Drag {
        origin,
        gear,
        moved: false,
    }));
    SetCapture(bar);
}

pub(super) unsafe fn motion(bar: HWND) {
    let mut p = zeroed();
    GetCursorPos(&mut p);
    DRAG.with_borrow_mut(|d| {
        if let Some(d) = d {
            d.moved |=
                (p.x - d.origin.x).abs() >= GetSystemMetrics(SM_CXDRAG) || (p.y - d.origin.y).abs() >= GetSystemMetrics(SM_CYDRAG);
            if d.moved {
                SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
            }
        }
    });
    if DRAG.with_borrow(|d| d.as_ref().is_some_and(|d| d.moved)) {
        let next = candidate(p).filter(|&i| i != settings::preferences().edge_index);
        if PREVIEW.replace(next) != next {
            let r = bounds(0);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let radius = thickness();
            SetWindowRgn(
                bar,
                if settings::preferences().dock {
                    CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius)
                } else {
                    null_mut()
                },
                1,
            );
            SetWindowPos(bar, HWND_TOPMOST, r.left, r.top, w, h, SWP_NOACTIVATE);
            InvalidateRect(bar, null(), 0);
        }
    }
}

pub(super) unsafe fn finish(bar: HWND) {
    motion(bar);
    let Some(d) = DRAG.take() else { return };
    let had_preview = PREVIEW.take().is_some();
    ReleaseCapture();
    if !d.moved {
        if d.gear {
            settings::open();
        }
        return;
    }
    let mut p = zeroed();
    GetCursorPos(&mut p);
    if let Some(index) = candidate(p) {
        settings::move_to(bar, index);
    }
    if had_preview {
        place(bar);
    }
}
