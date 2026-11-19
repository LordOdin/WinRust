use super::*;
use windows_sys::Win32::UI::{Controls::Dialogs::*, Controls::*};
use winrust_common::style::{
    CUSTOM, Theme,
    appearance::{AREAS, Area, Choice, ELEMENTS, Element},
    theme,
};
pub(super) use winrust_common::style::{appearance::bar_palette as pal, ui_face};
pub(super) const LIST: i32 = 129;
pub(super) const PRESET: i32 = 128;
pub(super) const EDIT: i32 = 130;

pub(super) fn square() -> bool {
    winrust_common::style::square()
}

pub(super) const FONT: i32 = 146;
pub(super) const CORNERS: i32 = 148;
pub(super) const GROUP: i32 = 150;
pub(super) const RESET: i32 = 151;
pub(super) const MODE: i32 = 153;

unsafe fn finish(dlg: HWND, result: std::io::Result<()>) {
    if let Err(err) = result {
        MessageBoxW(
            dlg,
            wide(&err.to_string()).as_ptr(),
            wide(t!("Ошибка сохранения", "Could not save theme")).as_ptr(),
            MB_ICONERROR,
        );
    }
    sync(dlg);
}

unsafe fn area(dlg: HWND) -> Option<Area> {
    let index = SendDlgItemMessageW(dlg, GROUP, CB_GETCURSEL, 0, 0);
    index.checked_sub(1).and_then(|i| AREAS.get(i as usize)).map(|a| a.0)
}

#[derive(Clone, Copy)]
enum Row {
    Palette(usize),
    Element(Element),
}
impl Row {
    fn color(self) -> u32 {
        match self {
            Self::Palette(i) => theme().colors()[i],
            Self::Element(id) => id.color(),
        }
    }
    fn custom(self) -> bool {
        match self {
            Self::Palette(i) => {
                let t = theme();
                t.selected == CUSTOM && t.custom[i] != t.preset_colors()[i]
            }
            Self::Element(id) => id.is_custom(),
        }
    }
    fn save(self, value: Option<u32>) -> std::io::Result<()> {
        match self {
            Self::Element(id) => id.save(value),
            Self::Palette(i) => {
                let mut t = theme();
                if t.selected != CUSTOM {
                    if value.is_none() {
                        return Ok(());
                    }
                    t.base = t.selected;
                    t.custom = t.colors();
                    t.selected = CUSTOM;
                }
                t.custom[i] = value.unwrap_or_else(|| t.preset_colors()[i]);
                t.save()
            }
        }
    }
}
unsafe fn rows(dlg: HWND) -> Vec<(Row, &'static str)> {
    if let Some(area) = area(dlg) {
        ELEMENTS
            .iter()
            .filter(|item| item.1 == area)
            .map(|&(id, _, en, ru, _)| (Row::Element(id), if i18n::english() { en } else { ru }))
            .collect()
    } else {
        names().into_iter().enumerate().map(|(i, label)| (Row::Palette(i), label)).collect()
    }
}
unsafe fn selected(dlg: HWND) -> Option<Row> {
    let index = SendDlgItemMessageW(dlg, LIST, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as _) as usize;
    rows(dlg).get(index).map(|&(row, _)| row)
}

fn names() -> [&'static str; 7] {
    [
        t!("Фон", "Background"),
        t!("Граница", "Border"),
        t!("Наведение", "Hover"),
        t!("Выделение", "Selection / active"),
        t!("Акцент", "Accent"),
        t!("Текст", "Text"),
        t!("Вторичный текст", "Secondary text"),
    ]
}

pub(super) unsafe fn init(dlg: HWND) {
    for label in [
        t!("Синий", "Windows blue"),
        t!("Терминал", "Terminal green"),
        t!("Графит", "Graphite violet"),
        t!("Янтарь", "Amber"),
        t!("Своя палитра", "Custom"),
    ] {
        SendDlgItemMessageW(dlg, PRESET, CB_ADDSTRING, 0, wide(label).as_ptr() as _);
    }
    for (id, labels) in [
        (FONT, vec![t!("Из палитры", "From palette"), "Segoe UI", "Consolas"]),
        (CORNERS, vec![t!("Из палитры", "From palette"), t!("Скруглённые", "Rounded"), t!("Прямые", "Square")]),
        (
            MODE,
            vec![
                t!("Из приложения", "App setting"),
                t!("Светлая", "Light"),
                t!("Тёмная", "Dark"),
                "WinRust",
                t!("Системная", "Windows"),
            ],
        ),
    ] {
        for label in labels {
            SendDlgItemMessageW(dlg, id, CB_ADDSTRING, 0, wide(label).as_ptr() as _);
        }
    }
    for label in std::iter::once(t!("Общая палитра", "Shared palette")).chain(AREAS.iter().map(|&(_, en, ru)| if i18n::english() { en } else { ru })) {
        SendDlgItemMessageW(dlg, GROUP, CB_ADDSTRING, 0, wide(label).as_ptr() as _);
    }
    SendDlgItemMessageW(dlg, GROUP, CB_SETCURSEL, 0, 0);
    settings::setup_table(
        GetDlgItem(dlg, LIST),
        &[(t!("Элемент", "Element"), 51), (t!("Цвет", "Color"), 23), (t!("Источник", "Source"), 26)],
        false,
    );
    rebuild(dlg);
}

pub(super) unsafe fn mode_visible(dlg: HWND, page_visible: bool) {
    let visible = page_visible && area(dlg) == Some(Area::Task);
    for id in [152, MODE] {
        ShowWindow(GetDlgItem(dlg, id), if visible { SW_SHOW } else { SW_HIDE });
    }
}

pub(super) unsafe fn rebuild(dlg: HWND) {
    let list = GetDlgItem(dlg, LIST);
    SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    for (i, (_, label)) in rows(dlg).iter().enumerate() {
        settings::insert_row(list, i, &[label, "", ""]);
    }
    let item = LVITEMW {
        state: LVIS_SELECTED | LVIS_FOCUSED,
        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
        ..zeroed()
    };
    SendMessageW(list, LVM_SETITEMSTATE, 0, &item as *const _ as _);
    mode_visible(dlg, true);
    sync(dlg);
}

pub(super) unsafe fn sync(dlg: HWND) {
    for (id, value) in [
        (PRESET, theme().selected),
        (FONT, Choice::Font.value() as usize),
        (CORNERS, Choice::Corners.value() as usize),
        (MODE, Choice::TaskMode.value() as usize),
    ] {
        SendDlgItemMessageW(dlg, id, CB_SETCURSEL, value, 0);
    }
    let list = GetDlgItem(dlg, LIST);
    for (i, (row, _)) in rows(dlg).into_iter().enumerate() {
        let c = row.color();
        let automatic = matches!(row, Row::Element(Element::DiskFiles | Element::DiskFolders)) && !row.custom();
        if automatic {
            settings::set_cell(list, i, 1, t!("По типу", "By file type / name"));
        } else {
            settings::set_cell(list, i, 1, &format!("#{:02X}{:02X}{:02X}", c & 255, (c >> 8) & 255, (c >> 16) & 255));
        }
        settings::set_cell(
            list,
            i,
            2,
            if row.custom() {
                t!("Свой", "Override")
            } else {
                t!("По умолчанию", "Default")
            },
        );
    }
    selection_changed(dlg);
    InvalidateRect(list, null(), 1);
}

pub(super) unsafe fn selection_changed(dlg: HWND) {
    let row = selected(dlg);
    EnableWindow(GetDlgItem(dlg, EDIT), row.is_some() as i32);
    EnableWindow(GetDlgItem(dlg, RESET), row.is_some_and(Row::custom) as i32);
}

pub(super) unsafe fn select(dlg: HWND, id: i32) {
    let index = SendDlgItemMessageW(dlg, id, CB_GETCURSEL, 0, 0);
    if index < 0 {
        return;
    }
    let result = match id {
        GROUP => {
            rebuild(dlg);
            return;
        }
        PRESET if index as usize <= CUSTOM => Theme {
            selected: index as usize,
            ..theme()
        }
        .save(),
        FONT => Choice::Font.save(index as u32),
        CORNERS => Choice::Corners.save(index as u32),
        MODE => Choice::TaskMode.save(index as u32),
        _ => return,
    };
    finish(dlg, result);
}

pub(super) unsafe fn reset(dlg: HWND) {
    if let Some(row) = selected(dlg) {
        finish(dlg, row.save(None));
    }
}

pub(super) unsafe fn edit(dlg: HWND, index: i32) {
    let Some(&(row, _)) = rows(dlg).get(index as usize) else { return };
    let mut slots = [0xffffff; 16];
    slots[..7].copy_from_slice(&theme().colors());
    let mut picker = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: dlg,
        rgbResult: row.color(),
        lpCustColors: slots.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..zeroed()
    };
    if ChooseColorW(&mut picker) != 0 {
        finish(dlg, row.save(Some(picker.rgbResult)));
    }
}

pub(super) unsafe fn draw(dlg: HWND, lp: LPARAM) -> isize {
    let cd = &mut *(lp as *mut NMLVCUSTOMDRAW);
    let result = match cd.nmcd.dwDrawStage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW,
        CDDS_ITEMPREPAINT => CDRF_NOTIFYSUBITEMDRAW,
        stage if stage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) && cd.iSubItem == 1 => {
            if let Some(&(row, _)) = rows(dlg).get(cd.nmcd.dwItemSpec)
                && (!matches!(row, Row::Element(Element::DiskFiles | Element::DiskFolders)) || row.custom())
            {
                let color = row.color();
                cd.clrTextBk = color;
                let light = (color & 255) * 299 + ((color >> 8) & 255) * 587 + ((color >> 16) & 255) * 114;
                cd.clrText = if light > 128000 { 0 } else { 0xffffff };
            }
            CDRF_NEWFONT
        }
        _ => CDRF_DODEFAULT,
    };
    SetWindowLongPtrW(dlg, DWLP_MSGRESULT as i32, result as _);
    1
}
