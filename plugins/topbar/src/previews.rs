use super::*;
use windows_sys::Win32::Graphics::Dwm::*;

const NAME: &str = "WinRustWindowPreviews";

// DWM's window Peek entry point on Windows 8.1 and later. Resolve it at runtime
// because the compositor exports it by ordinal rather than in the public SDK.
type ActivatePeek = unsafe extern "system" fn(i32, HWND, HWND, u32, usize) -> i32;
const WINDOW_PEEK: u32 = 3;

fn peek_api() -> Option<ActivatePeek> {
    static API: std::sync::OnceLock<Option<ActivatePeek>> = std::sync::OnceLock::new();
    *API.get_or_init(|| unsafe {
        let module = GetModuleHandleW(wide("dwmapi.dll").as_ptr());
        GetProcAddress(module, 113usize as *const u8).map(|f| std::mem::transmute::<_, ActivatePeek>(f))
    })
}

struct Card {
    window: HWND,
    thumbnail: isize,
    size: SIZE,
    bounds: RECT,
    title: String,
    icon: HICON,
}

impl Drop for Card {
    fn drop(&mut self) {
        if self.thumbnail != 0 {
            unsafe {
                DwmUnregisterThumbnail(self.thumbnail);
            }
        }
    }
}

struct Group {
    button: i32,
    windows: Vec<HWND>,
    cards: Vec<Card>,
    page: usize,
    capacity: usize,
    outside_since: Option<u64>,
}

thread_local! {
    static HOVERED: Cell<HWND> = const { Cell::new(null_mut()) };
    static PEEK_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static POPUP: Cell<HWND> = const { Cell::new(null_mut()) };
    static GROUP: RefCell<Option<Group>> = const { RefCell::new(None) };
}

pub(super) unsafe fn init(instance: HINSTANCE) {
    let class = wide(NAME);
    RegisterClassW(&WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        lpszClassName: class.as_ptr(),
        ..zeroed()
    });
    POPUP.set(CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        class.as_ptr(),
        wide("Window previews").as_ptr(),
        WS_POPUP,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        instance,
        null(),
    ));
    keep_visible_during_peek(POPUP.get());
}

pub(super) unsafe fn keep_visible_during_peek(hwnd: HWND) {
    let enabled = 1i32;
    DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK as u32, &enabled as *const _ as _, size_of::<i32>() as u32);
}

unsafe fn hover(hwnd: HWND, window: HWND) {
    if HOVERED.replace(window) == window { return; }
    if let Some(peek) = peek_api() {
        if !window.is_null() && IsWindow(window) != 0 && peek(1, window, hwnd, WINDOW_PEEK, 0) >= 0 {
            PEEK_ACTIVE.set(true);
        } else if PEEK_ACTIVE.replace(false) {
            peek(0, null_mut(), hwnd, WINDOW_PEEK, 0);
        }
    }
    InvalidateRect(hwnd, null(), 0);
}

pub(super) unsafe fn hide() {
    let popup = POPUP.get();
    hover(popup, null_mut());
    if GROUP.take().is_some() {
        KillTimer(popup, 1);
        ShowWindow(popup, SW_HIDE);
    }
}

pub(super) fn windows(button: i32) -> Vec<HWND> {
    BTNS.with_borrow(|buttons| buttons.get(button as usize).map(|b| b.wins.iter().copied().filter(|&w| unsafe { IsWindow(w) != 0 }).collect()).unwrap_or_default())
}

unsafe fn caption(window: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(window, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn fit(area: RECT, size: SIZE) -> RECT {
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    let scale = (w as f64 / size.cx as f64).min(h as f64 / size.cy as f64);
    let (w, h) = (
        (size.cx as f64 * scale).round() as i32,
        (size.cy as f64 * scale).round() as i32,
    );
    let x = (area.left + area.right - w) / 2;
    let y = (area.top + area.bottom - h) / 2;
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

impl Card {
    unsafe fn update(&mut self) {
        if self.thumbnail == 0 {
            return;
        }
        let mut size: SIZE = zeroed();
        if DwmQueryThumbnailSourceSize(self.thumbnail, &mut size) >= 0
            && size.cx > 0
            && size.cy > 0
            && (size.cx, size.cy) != (self.size.cx, self.size.cy)
        {
            let r = self.bounds;
            let area = RECT {
                left: r.left + sc(7),
                top: r.top + sc(31),
                right: r.right - sc(7),
                bottom: r.bottom - sc(7),
            };
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: fit(area, size),
                fVisible: 1,
                opacity: 255,
                ..zeroed()
            };
            if DwmUpdateThumbnailProperties(self.thumbnail, &props) >= 0 {
                self.size = size;
            }
        }
    }
}

unsafe fn layout() {
    let hwnd = POPUP.get();
    let bar = BAR.load(Relaxed) as HWND;
    let (width, height, cx) = GROUP.with_borrow_mut(|state| {
        let g = state.as_mut().unwrap();
        let screen = GetSystemMetrics(SM_CXSCREEN) - if edge::vertical() {edge::thickness()+dock_gap()} else {0};
        let pad = sc(6);
        let card_w = sc(208).min((screen - 4 * pad).max(sc(40)));
        let card_h = sc(150);
        g.capacity = ((screen - 3 * pad) / (card_w + pad)).max(1) as usize;
        g.page = g.page.min((g.windows.len() - 1) / g.capacity);
        let mut left = pad;
        g.cards.clear();
        for &window in g.windows.iter().skip(g.page * g.capacity).take(g.capacity) {
            let mut thumb = 0;
            if DwmRegisterThumbnail(hwnd, window, &mut thumb) < 0 {
                thumb = 0;
            }
            let bounds = RECT {
                left,
                top: pad,
                right: left + card_w,
                bottom: pad + card_h,
            };
            let mut card = Card {
                window,
                thumbnail: thumb,
                size: zeroed(),
                bounds,
                title: caption(window),
                icon: win_icon(window),
            };
            card.update();
            g.cards.push(card);
            left += card_w + pad;
        }
        (
            left,
            2 * pad + card_h + if g.windows.len() > g.capacity { sc(28) } else { 0 },
            tip_cx(&bar_layout(bar), g.button),
        )
    });
    let (x,y)=edge::popup(cx,width,height,0);
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE);
    update_hover(hwnd);
    InvalidateRect(hwnd, null(), 0);
}

pub(super) fn visible() -> bool { GROUP.with_borrow(|g|g.is_some()) }

pub(super) fn showing(button: i32) -> bool {
    GROUP.with_borrow(|state| state.as_ref().is_some_and(|g| g.button == button))
}

pub(super) unsafe fn show(button: i32) -> bool {
    let hwnd = POPUP.get();
    if button < 1 || hwnd.is_null() {
        return false;
    }
    if showing(button) {
        return true;
    }
    let wins = windows(button);
    if wins.is_empty() {
        return false;
    }
    GROUP.replace(Some(Group {
        button,
        windows: wins,
        cards: Vec::new(),
        page: 0,
        capacity: 1,
        outside_since: None,
    }));
    layout();
    SetTimer(hwnd, 1, 100, None);
    true
}

unsafe fn pointer(hwnd: HWND) -> POINT {
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    ScreenToClient(hwnd, &mut p);
    p
}

unsafe fn update_hover(hwnd: HWND) {
    let mut screen: POINT = zeroed();
    GetCursorPos(&mut screen);
    let hovered = if WindowFromPoint(screen) == hwnd {
        let pt = pointer(hwnd);
        GROUP.with_borrow(|s| s.as_ref()
            .and_then(|g| g.cards.iter().find(|c| PtInRect(&c.bounds, pt) != 0))
            .map_or(null_mut(), |c| c.window))
    } else { null_mut() };
    hover(hwnd, hovered);
}

unsafe fn maintain(hwnd: HWND) {
    let bar = BAR.load(Relaxed) as HWND;
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    let under = WindowFromPoint(p);
    let mut rebuild = false;
    let keep_open = GROUP.with_borrow_mut(|state| {
        let Some(g) = state.as_mut() else { return false };
        if under == hwnd || (under == bar && hit(bar, edge::axis(pointer(bar))) == g.button) {
            g.outside_since = None;
        } else if now_ms().saturating_sub(*g.outside_since.get_or_insert_with(|| now_ms())) >= u64::from(Timing::PreviewDismiss.ms()) {
            return false;
        }
        let current = windows(g.button);
        if !g.windows.iter().any(|w| current.contains(w)) {
            return false;
        }
        let previous_count = g.windows.len();
        g.windows.retain(|w| current.contains(w));
        for &w in &current {
            if !g.windows.contains(&w) {
                g.windows.push(w);
            }
        }
        let start = g.page * g.capacity;
        let visible = g.windows.iter().skip(start).take(g.capacity);
        rebuild = previous_count != g.windows.len()
            || !visible.eq(g.cards.iter().map(|c| &c.window))
            || (g.windows.len() <= g.capacity && g.page != 0);
        if !rebuild {
            for card in &mut g.cards {
                let title = caption(card.window);
                if title != card.title {
                    card.title = title;
                    InvalidateRect(hwnd, null(), 0);
                }
                card.update();
            }
        }
        true
    });
    if !keep_open || fly::any_open() || FULLSCREEN.load(Relaxed) {
        hide();
    } else if rebuild {
        layout();
    } else {
        update_hover(hwnd);
    }
}

unsafe fn page(delta: isize) {
    let changed = GROUP.with_borrow_mut(|state| {
        let Some(g) = state.as_mut() else { return false };
        let next = g.page.saturating_add_signed(delta).min((g.windows.len() - 1) / g.capacity);
        if next == g.page {
            return false;
        }
        g.page = next;
        true
    });
    if changed {
        layout();
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let target = BeginPaint(hwnd, &mut ps);
    let mut r: RECT = zeroed();
    GetClientRect(hwnd, &mut r);
    // Present the completed frame once so the cleared background never flashes between cards.
    let buffer = CreateCompatibleDC(target);
    let bitmap = CreateCompatibleBitmap(target, r.right, r.bottom);
    let buffered = !buffer.is_null() && !bitmap.is_null();
    let dc = if buffered { buffer } else { target };
    let old_bitmap = if buffered { SelectObject(dc, bitmap) } else { null_mut() };
    fill(dc, r, pal().bg, 0);
    SetBkMode(dc, TRANSPARENT as i32);
    let font = tip_font();
    let old = SelectObject(dc, font);
    GROUP.with_borrow(|state| {
        let Some(g) = state else { return };
        for card in &g.cards {
            let b = card.bounds;
            fill(
                dc,
                b,
                if HOVERED.get() == card.window {
                    pal().hover
                } else {
                    pal().active
                },
                rr(6),
            );
            let icon = sc(16);
            if !card.icon.is_null() {
                DrawIconEx(
                    dc,
                    b.left + sc(7),
                    b.top + sc(7),
                    card.icon,
                    icon,
                    icon,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }
            text(
                dc,
                &card.title,
                RECT {
                    left: b.left + sc(30),
                    right: b.right - sc(8),
                    top: b.top,
                    bottom: b.top + sc(30),
                },
                pal().text,
                DT_LEFT,
            );
            if card.size.cx == 0 && !card.icon.is_null() {
                let icon = sc(32);
                DrawIconEx(
                    dc,
                    (b.left + b.right - icon) / 2,
                    (b.top + b.bottom + sc(30) - icon) / 2,
                    card.icon,
                    icon,
                    icon,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }
        }
        if g.windows.len() > g.capacity {
            let top = r.bottom - sc(28);
            text(
                dc,
                &format!("{} / {}", g.page + 1, g.windows.len().div_ceil(g.capacity)),
                RECT { top, ..r },
                pal().text,
                DT_CENTER,
            );
            text(dc, "‹", RECT { left: sc(12), top, ..r }, pal().text, DT_LEFT);
            text(
                dc,
                "›",
                RECT {
                    right: r.right - sc(12),
                    top,
                    ..r
                },
                pal().text,
                DT_RIGHT,
            );
        }
    });
    SelectObject(dc, old);
    DeleteObject(font);
    if buffered {
        BitBlt(target, 0, 0, r.right, r.bottom, dc, 0, 0, SRCCOPY);
        SelectObject(dc, old_bitmap);
    }
    if !bitmap.is_null() {
        DeleteObject(bitmap);
    }
    if !buffer.is_null() {
        DeleteDC(buffer);
    }
    EndPaint(hwnd, &ps);
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as isize,
        WM_PAINT => paint(hwnd),
        WM_ERASEBKGND => return 1,
        WM_TIMER => maintain(hwnd),
        WM_MOUSEMOVE => {
            update_hover(hwnd);
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut track);
        }
        WM_MOUSELEAVE => hover(hwnd, null_mut()),
        WM_MOUSEWHEEL => page(if (wp >> 16) as i16 > 0 { -1 } else { 1 }),
        WM_LBUTTONUP => {
            let pt = POINT {
                x: lp as i16 as i32,
                y: (lp >> 16) as i16 as i32,
            };
            let target = GROUP.with_borrow(|s| {
                s.as_ref()
                    .and_then(|g| g.cards.iter().find(|c| PtInRect(&c.bounds, pt) != 0).map(|c| c.window))
            });
            if let Some(w) = target {
                hide();
                activate_window(w);
            } else {
                let mut r: RECT = zeroed();
                GetClientRect(hwnd, &mut r);
                if pt.y >= r.bottom - sc(28) {
                    page(if pt.x < r.right / 2 { -1 } else { 1 });
                }
            }
        }
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    0
}
