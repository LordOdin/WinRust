//! Individually editable colors. Registry value names stay stable when display order changes.
use super::{Pal, pal, rgb};
use std::{
    cell::RefCell,
    io,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::*,
};

const KEY: *const u16 = windows_sys::w!(r"Software\WinRust\Appearance");
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Taskbar,
    Previews,
    Frame,
    Desktop,
    Explorer,
    Search,
    Task,
    Photo,
    Disk,
    Settings,
    Installer,
}

pub const AREAS: &[(Area, &str, &str)] = &[
    (Area::Taskbar, "Taskbar", "Панель задач"),
    (Area::Previews, "Window previews", "Эскизы окон"),
    (Area::Frame, "Window frames", "Рамки окон"),
    (Area::Desktop, "Desktop", "Рабочий стол"),
    (Area::Explorer, "File Explorer", "Проводник"),
    (Area::Search, "Search", "Поиск"),
    (Area::Task, "Task Manager", "Диспетчер задач"),
    (Area::Photo, "Photo Viewer", "Просмотр фото"),
    (Area::Disk, "WinRust Disk", "WinRust Disk"),
    (Area::Settings, "Settings", "Настройки"),
    (Area::Installer, "Installer", "Установщик"),
];

macro_rules! elements {
    ($p:ident, $get:ident; $( $area:ident { $( $id:ident: ($en:literal, $ru:literal) => $default:expr, )+ } )+) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum Element { $($($id,)+)+ }
        pub const ELEMENTS: &[(Element, Area, &str, &str, &str)] = &[$($((Element::$id, Area::$area, $en, $ru, stringify!($id)),)+)+];
        impl Element {
            fn default(self, $p: &Pal, values: &[u32]) -> u32 {
                let $get = |element: Element| values[element as usize];
                use Element::*;
                match self { $($(Self::$id => $default,)+)+ }
            }
        }
    }
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s| {
        let x = ((a >> s) & 255u32) as f32;
        (x + (((b >> s) & 255u32) as f32 - x) * t) as u32
    };
    ch(0) | ch(8) << 8 | ch(16) << 16
}

fn dark(c: u32) -> bool {
    299 * (c & 255) + 587 * ((c >> 8) & 255) + 114 * ((c >> 16) & 255) < 128000
}

// Derived colors reference earlier entries so overriding a background also updates its dependent shades.
elements! { p, get;
    Taskbar {
        BarBg: ("Background", "Фон") => p.bg,
        BarLine: ("Border", "Граница") => p.line,
        BarHover: ("Button hover", "Наведение") => p.hover,
        BarActive: ("Active button", "Активная кнопка") => p.active,
        BarAccent: ("Accent / indicator", "Акцент / индикатор") => p.accent,
        BarText: ("Text and icons", "Текст и значки") => p.text,
        BarDim: ("Secondary text", "Вторичный текст") => p.dim,
        BarGood: ("Success", "Успех") => rgb(108,203,95),
        BarWarn: ("Warning", "Предупреждение") => rgb(255,196,0),
        BarBad: ("Error / danger", "Ошибка") => rgb(255,99,97),
    }
    Previews {
        PreviewBg: ("Background", "Фон") => get(BarBg),
        PreviewHover: ("Hovered card", "Наведение") => get(BarHover),
        PreviewCard: ("Card background", "Фон карточки") => get(BarActive),
        PreviewText: ("Window titles and arrows", "Заголовки и стрелки") => get(BarText),
    }
    Frame {
        FrameBg: ("Title bar", "Заголовок") => p.bg,
        FrameText: ("Title text", "Текст заголовка") => p.text,
        FrameLine: ("Window border", "Граница окна") => p.line,
    }
    Desktop {
        DesktopLabel: ("Icon labels", "Подписи значков") => 0xffffff,
    }
    Explorer {
        ExplorerBg: ("Toolbar and pane background", "Фон панелей") => p.bg,
        ExplorerLine: ("Dividers", "Разделители") => p.line,
        ExplorerHover: ("Button hover", "Наведение") => p.hover,
        ExplorerActive: ("Active tab and folder", "Активная вкладка и папка") => p.active,
        ExplorerAccent: ("Accent and focus", "Акцент и фокус") => p.accent,
        ExplorerText: ("Names and labels", "Имена и подписи") => p.text,
        ExplorerDim: ("Details and status text", "Сведения и состояние") => p.dim,
        ExplorerList: ("File list background", "Фон списка файлов") => mix(get(ExplorerBg),0,0.22),
        ExplorerAddress: ("Address field", "Поле адреса") => mix(get(ExplorerBg),0,0.35),
        ExplorerDisabled: ("Disabled controls", "Недоступные элементы") => mix(get(ExplorerDim),get(ExplorerBg),0.45),
        ExplorerNav: ("Sidebar background", "Фон боковой панели") => get(ExplorerBg),
        ExplorerStatus: ("Status bar background", "Фон строки состояния") => get(ExplorerBg),
        ExplorerSelected: ("Selected file", "Выбранный файл") => mix(get(ExplorerList),get(ExplorerAccent),0.20),
        ExplorerRowHot: ("Hovered file", "Файл под указателем") => mix(get(ExplorerList),get(ExplorerText),0.055),
        ExplorerLow: ("Low disk space", "Мало места на диске") => rgb(255,99,97),
    }
    Search {
        SearchBg: ("Search background", "Фон поиска") => p.bg,
        SearchLine: ("Border", "Граница") => p.line,
        SearchSel: ("Selected result", "Выбранный результат") => p.active,
        SearchAccent: ("Accent", "Акцент") => p.accent,
        SearchText: ("Result titles", "Заголовки результатов") => p.text,
        SearchDim: ("Secondary text", "Вторичный текст") => p.dim,
        SearchFaint: ("Hint text", "Подсказки") => mix(get(SearchDim),get(SearchBg),0.4),
        SearchMenu: ("Menu background", "Фон меню") => mix(get(SearchBg),get(SearchLine),0.5),
        SearchDanger: ("Destructive action", "Опасное действие") => rgb(248,113,113),
        SearchDangerText: ("Text on destructive action", "Текст опасного действия") => rgb(20,20,24),
        SearchMenuHot: ("Menu selection", "Выбор в меню") => get(SearchSel),
    }
    Task {
        TaskBg: ("Page background", "Фон страницы") => p.bg,
        TaskHover: ("Hovered row", "Наведение") => p.hover,
        TaskNav: ("Navigation pane", "Панель навигации") => mix(get(TaskBg),get(TaskHover),0.5),
        TaskPanel: ("Panels and search box", "Панели и поле поиска") => get(TaskHover),
        TaskText: ("Primary text", "Основной текст") => p.text,
        TaskDim: ("Secondary text", "Вторичный текст") => p.dim,
        TaskBorder: ("Borders", "Границы") => p.line,
        TaskSel: ("Selected row", "Выбранная строка") => p.active,
        TaskAccent: ("Accent", "Акцент") => p.accent,
        TaskHeatLow: ("Low usage heatmap", "Малая нагрузка") => mix(get(TaskBg),get(TaskAccent),0.10),
        TaskHeatHigh: ("High usage heatmap", "Высокая нагрузка") => mix(get(TaskBg),get(TaskAccent),0.48),
        TaskWarn: ("Warning", "Предупреждение") => if dark(get(TaskBg)) {rgb(252,225,0)} else {rgb(157,93,0)},
        TaskBad: ("Error", "Ошибка") => if dark(get(TaskBg)) {rgb(255,99,97)} else {rgb(196,43,28)},
        GraphCPU: ("CPU graph", "График ЦП") => rgb(23,169,208),
        GraphMemory: ("Memory graph", "График памяти") => rgb(86,124,214),
        GraphDisk: ("Disk graph", "График диска") => rgb(82,177,95),
        GraphNetwork: ("Network graph", "График сети") => rgb(224,138,60),
        GraphGPU: ("GPU graph", "График ГП") => rgb(176,92,216),
    }
    Photo {
        PhotoBg: ("Canvas and transparent pixels", "Фон изображения и прозрачности") => p.bg,
        PhotoNote: ("Viewer messages", "Сообщения") => p.dim,
    }
    Disk {
        DiskBg: ("Background", "Фон") => p.bg,
        DiskHeader: ("Heading", "Заголовок") => p.accent,
        DiskFooter: ("Status text", "Строка состояния") => p.dim,
        DiskLabel: ("Treemap labels", "Подписи на карте") => 0xffffff,
        DiskFiles: ("File tile color", "Цвет файлов") => rgb(92,135,184),
        DiskFolders: ("Folder tile color", "Цвет папок") => rgb(80,106,86),
        DiskOutline: ("Hovered tile outline", "Граница под указателем") => 0xffffff,
    }
    Settings {
        SettingsBg: ("Dialog background", "Фон диалога") => p.bg,
        SettingsField: ("Input background", "Фон полей") => p.active,
        SettingsText: ("Labels and input text", "Подписи и текст полей") => p.text,
        SettingsList: ("Table background", "Фон таблиц") => get(SettingsBg),
        SettingsRows: ("Table text", "Текст таблиц") => get(SettingsText),
    }
    Installer {
        InstallBg: ("Background", "Фон") => p.bg,
        InstallPanel: ("Panels and controls", "Панели и кнопки") => p.active,
        InstallLine: ("Borders", "Границы") => p.line,
        InstallAccent: ("Accent", "Акцент") => p.accent,
        InstallText: ("Text", "Текст") => p.text,
        InstallDim: ("Secondary text", "Вторичный текст") => p.dim,
        InstallGood: ("Success", "Успех") => rgb(110,200,120),
        InstallBad: ("Error / danger", "Ошибка") => rgb(255,99,97),
        InstallOnAccent: ("Text on accent", "Текст на акцентном фоне") => 0xffffff,
    }
}

#[derive(Clone, Copy)]
pub enum Choice {
    Font,
    Corners,
    TaskMode,
}
impl Choice {
    fn name(self) -> &'static str {
        match self {
            Self::Font => "Font",
            Self::Corners => "Corners",
            Self::TaskMode => "TaskMode",
        }
    }
    pub fn value(self) -> u32 {
        cached(|s| s.choices[self as usize])
    }
    pub fn save(self, value: u32) -> io::Result<()> {
        if value > self.max() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid appearance option"));
        }
        write(self.name(), (value != 0).then_some(value))
    }
    fn max(self) -> u32 {
        if matches!(self, Self::TaskMode) { 4 } else { 2 }
    }
}

const COUNT: usize = ELEMENTS.len();
struct Snapshot {
    colors: [u32; COUNT],
    overrides: [Option<u32>; COUNT],
    choices: [u32; 3],
}

impl Snapshot {
    fn read() -> Self {
        let mut state = Self {
            colors: [0; COUNT],
            overrides: [None; COUNT],
            choices: [0; 3],
        };
        unsafe {
            let mut key = null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, KEY, 0, KEY_QUERY_VALUE, &mut key) == ERROR_SUCCESS {
                let mut index = 0;
                loop {
                    let mut name = [0u16; 64];
                    let mut len = name.len() as u32;
                    let (mut ty, mut value, mut size) = (0, 0u32, 4);
                    let err = RegEnumValueW(key, index, name.as_mut_ptr(), &mut len, null(), &mut ty, (&mut value as *mut u32).cast(), &mut size);
                    if err == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    index += 1;
                    if err == ERROR_MORE_DATA {
                        continue;
                    }
                    if err != ERROR_SUCCESS {
                        break;
                    }
                    if ty != REG_DWORD || size != 4 {
                        continue;
                    }
                    let name = String::from_utf16_lossy(&name[..len as usize]);
                    if let Some(&(id, ..)) = ELEMENTS.iter().find(|entry| entry.4.eq_ignore_ascii_case(&name)) {
                        if value <= 0xffffff {
                            state.overrides[id as usize] = Some(value);
                        }
                    } else if let Some(choice) = [Choice::Font, Choice::Corners, Choice::TaskMode]
                        .into_iter()
                        .find(|c| c.name().eq_ignore_ascii_case(&name) && value <= c.max())
                    {
                        state.choices[choice as usize] = value;
                    }
                }
                RegCloseKey(key);
            }
        }
        let palette = pal();
        for &(id, ..) in ELEMENTS {
            state.colors[id as usize] = state.overrides[id as usize].unwrap_or_else(|| id.default(&palette, &state.colors));
        }
        state
    }
}

thread_local! { static CACHE: RefCell<Option<(Instant, Snapshot)>> = const { RefCell::new(None) }; }
pub(super) fn reload() {
    CACHE.with_borrow_mut(|cache| *cache = None);
}
fn cached<T>(f: impl FnOnce(&Snapshot) -> T) -> T {
    CACHE.with_borrow_mut(|cache| {
        if cache.as_ref().is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(1)) {
            *cache = Some((Instant::now(), Snapshot::read()));
        }
        f(&cache.as_ref().unwrap().1)
    })
}

impl Element {
    pub fn color(self) -> u32 {
        cached(|s| s.colors[self as usize])
    }
    pub fn is_custom(self) -> bool {
        cached(|s| s.overrides[self as usize].is_some())
    }
    pub fn save(self, color: Option<u32>) -> io::Result<()> {
        if color.is_some_and(|c| c > 0xffffff) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid color"));
        }
        write(ELEMENTS[self as usize].4, color)
    }
}

fn write(name: &str, value: Option<u32>) -> io::Result<()> {
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let err = unsafe {
        match value {
            Some(v) => RegSetKeyValueW(HKEY_CURRENT_USER, KEY, name.as_ptr(), REG_DWORD, (&v as *const u32).cast(), 4),
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, name.as_ptr()),
        }
    };
    if err != ERROR_SUCCESS && !(value.is_none() && err == ERROR_FILE_NOT_FOUND) {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    super::changed();
    Ok(())
}

// Fetch one snapshot per paint palette, even when several fields are used together.
pub fn bar_palette() -> Pal {
    cached(|s| Pal {
        bg: s.colors[Element::BarBg as usize],
        line: s.colors[Element::BarLine as usize],
        hover: s.colors[Element::BarHover as usize],
        active: s.colors[Element::BarActive as usize],
        accent: s.colors[Element::BarAccent as usize],
        text: s.colors[Element::BarText as usize],
        dim: s.colors[Element::BarDim as usize],
    })
}
pub fn explorer_palette() -> Pal {
    cached(|s| Pal {
        bg: s.colors[Element::ExplorerBg as usize],
        line: s.colors[Element::ExplorerLine as usize],
        hover: s.colors[Element::ExplorerHover as usize],
        active: s.colors[Element::ExplorerActive as usize],
        accent: s.colors[Element::ExplorerAccent as usize],
        text: s.colors[Element::ExplorerText as usize],
        dim: s.colors[Element::ExplorerDim as usize],
    })
}
pub fn installer_palette() -> Pal {
    cached(|s| Pal {
        bg: s.colors[Element::InstallBg as usize],
        line: s.colors[Element::InstallLine as usize],
        hover: s.colors[Element::InstallPanel as usize],
        active: s.colors[Element::InstallPanel as usize],
        accent: s.colors[Element::InstallAccent as usize],
        text: s.colors[Element::InstallText as usize],
        dim: s.colors[Element::InstallDim as usize],
    })
}
