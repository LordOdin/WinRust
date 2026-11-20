use super::*;
use windows_sys::Win32::System::Registry::*;

const KEY: &str = r"Software\WinRust\Topbar\Timing";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Timing {
    Shuffle,
    Drift,
    AutoHide,
    Preview,
    PreviewDismiss,
    Tooltip,
    BarShow,
    BarHide,
    Volume,
    CaptureNotice,
    HoverIn,
    HoverOut,
    Click,
    NewIcon,
    FlyIn,
    FlyOut,
    MenuGuard,
    TrayIn,
    TrayStagger,
    WifiRowIn,
    WifiRowStagger,
    PinPulse,
    Caret,
    FolderPreview,
}

pub(super) const ALL: &[Timing] = &[
    Timing::Shuffle,
    Timing::Drift,
    Timing::AutoHide,
    Timing::Preview,
    Timing::PreviewDismiss,
    Timing::Tooltip,
    Timing::BarShow,
    Timing::BarHide,
    Timing::Volume,
    Timing::CaptureNotice,
    Timing::HoverIn,
    Timing::HoverOut,
    Timing::Click,
    Timing::NewIcon,
    Timing::FlyIn,
    Timing::FlyOut,
    Timing::MenuGuard,
    Timing::TrayIn,
    Timing::TrayStagger,
    Timing::WifiRowIn,
    Timing::WifiRowStagger,
    Timing::PinPulse,
    Timing::Caret,
    Timing::FolderPreview,
];

impl Timing {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shuffle => t!("Интервал перемешивания", "Icon shuffle interval"),
            Self::Drift => t!("Сдвиг OLED на пиксель", "OLED drift per pixel"),
            Self::AutoHide => t!("Задержка скрытия панели", "Taskbar auto-hide delay"),
            Self::Preview => t!("Задержка миниатюр", "Window preview delay"),
            Self::PreviewDismiss => t!("Задержка закрытия миниатюр", "Preview dismissal delay"),
            Self::Tooltip => t!("Задержка подсказок", "Control tooltip delay"),
            Self::BarShow => t!("Появление панели", "Taskbar slide in"),
            Self::BarHide => t!("Скрытие панели", "Taskbar slide out"),
            Self::Volume => t!("Время показа громкости", "Volume indicator duration"),
            Self::CaptureNotice => t!("Уведомление о снимке", "Screenshot notice duration"),
            Self::HoverIn => t!("Подсветка при наведении", "Hover highlight in"),
            Self::HoverOut => t!("Угасание подсветки", "Hover highlight out"),
            Self::Click => t!("Отклик на щелчок", "Click feedback"),
            Self::NewIcon => t!("Появление значка", "New icon animation"),
            Self::FlyIn => t!("Открытие меню", "Menu opening"),
            Self::FlyOut => t!("Закрытие меню", "Menu closing"),
            Self::MenuGuard => t!("Повторное открытие меню", "Menu reopen delay"),
            Self::TrayIn => t!("Открытие области уведомлений", "Tray opening"),
            Self::TrayStagger => t!("Интервал значков в трее", "Delay between tray icons"),
            Self::WifiRowIn => t!("Появление строки сети Wi-Fi", "Wi-Fi network row animation"),
            Self::WifiRowStagger => t!("Интервал строк сетей Wi-Fi", "Delay between Wi-Fi rows"),
            Self::PinPulse => t!("Подсветка закрепления", "New pin highlight"),
            Self::FolderPreview => t!("Задержка просмотра папки", "Folder contents preview delay"),
            Self::Caret => t!("Мигание курсора пароля Wi-Fi", "Wi-Fi password cursor blink"),
        }
    }
    fn definition(self) -> (&'static str, u32) {
        match self {
            Self::Shuffle => ("Shuffle", 10),
            Self::Drift => ("Drift", 10),
            Self::AutoHide => ("AutoHide", 750),
            Self::Preview => ("Preview", 225),
            Self::PreviewDismiss => ("PreviewDismiss", 220),
            Self::Tooltip => ("Tooltip", 450),
            Self::BarShow => ("BarShow", 380),
            Self::BarHide => ("BarHide", 240),
            Self::Volume => ("Volume", 1200),
            Self::CaptureNotice => ("CaptureNotice", 2500),
            Self::HoverIn => ("HoverIn", 130),
            Self::HoverOut => ("HoverOut", 180),
            Self::Click => ("Click", 320),
            Self::NewIcon => ("NewIcon", 260),
            Self::FlyIn => ("FlyIn", 220),
            Self::FlyOut => ("FlyOut", 150),
            Self::MenuGuard => ("MenuGuard", 400),
            Self::TrayIn => ("TrayIn", 200),
            Self::TrayStagger => ("TrayStagger", 25),
            Self::WifiRowIn => ("WifiRowIn", 240),
            Self::WifiRowStagger => ("WifiRowStagger", 45),
            Self::PinPulse => ("PinPulse", 1200),
            Self::FolderPreview => ("FolderPreview", 450),
            Self::Caret => ("Caret", 530),
        }
    }
    pub fn seconds(self) -> bool {
        matches!(self, Self::Shuffle | Self::Drift)
    }
    pub fn unit(self) -> &'static str {
        if self.seconds() { t!("с", "s") } else { t!("мс", "ms") }
    }
    pub fn value(self) -> u32 {
        VALUES.with_borrow(|v| v[self as usize])
    }
    pub fn ms(self) -> u32 {
        self.value() * if self.seconds() { 1000 } else { 1 }
    }
    pub fn display(self) -> String {
        format!("{} {}", self.value(), self.unit())
    }
    pub fn maximum(self) -> u32 {
        if self.seconds() { 86_400 } else { 86_400_000 }
    }
    fn read(self) -> u32 {
        let (name, default) = self.definition();
        let mut value = 0u32;
        let mut size = 4;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide(name).as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                &mut value as *mut _ as _,
                &mut size,
            )
        };
        if result == ERROR_SUCCESS && (1..=self.maximum()).contains(&value) {
            value
        } else {
            default
        }
    }
    pub fn save(self, value: u32) -> std::io::Result<()> {
        if !(1..=self.maximum()).contains(&value) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("1 – {}", self.maximum()),
            ));
        }
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide(self.definition().0).as_ptr(),
                REG_DWORD,
                &value as *const _ as _,
                4,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(result as i32));
        }
        VALUES.with_borrow_mut(|v| v[self as usize] = value);
        Ok(())
    }
}

thread_local! {static VALUES: RefCell<Vec<u32>>=RefCell::new(ALL.iter().map(|t|t.read()).collect());}
