//! Her tray's menu, owner-drawn in her colours (Haseeb: the stock white menu read as another
//! app's). Her own menu's rows (`crate::menu`, as `TrayItem`s) red on jet black, the hovered one
//! ink on her red blade, what can't act now dim (outlined while the arrow keys are on it); a dot
//! on ATTENTION's choice in force (a switch has no tick: its words say what a click does); sizes
//! and fonts at the DPI of the monitor it opens on (`platform::menulook`). A stock menu
//! underneath: the arrows, Enter, Esc and submenus are Windows' own, a typed letter goes to her row
//! (`WM_MENUCHAR`), each row's words are a screen reader's (`Name`), and her frame replaces the
//! system's light one (each menu window's own proc wrapped while it's up). Under high contrast it
//! is the stock menu, in his theme's colours (`hc`, asked at each open).

use super::super::menulook::{self, COLORS, Keyed, Metrics, Typed};
use super::super::{Mark, TrayItem};
use super::fx::colorref;
use ::windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use ::windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute};
use ::windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, ClientToScreen, CreateCompatibleDC, CreateFontIndirectW, CreateSolidBrush, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC,
    DeleteObject, DrawTextW, ExcludeClipRect, FONT_CHARSET, FW_BOLD, FW_NORMAL, FW_SEMIBOLD, FillRect, GetStockObject, GetTextExtentPoint32W, GetWindowDC, HBRUSH, HDC, HFONT, HGDIOBJ, LOGFONTW, MONITOR_DEFAULTTONEAREST,
    MonitorFromPoint, NULL_PEN, OUT_TT_PRECIS, Polygon, ReleaseDC, RestoreDC, SYMBOL_CHARSET, SaveDC, SelectObject, SetBkMode, SetTextCharacterExtra, SetTextColor, TRANSPARENT,
};
use ::windows::Win32::System::Threading::GetCurrentThreadId;
use ::windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW, MSAA_MENU_SIG, MSAAMENUINFO};
use ::windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::PWSTR;
use std::cell::RefCell;

/// `MEASUREITEMSTRUCT` (winuser.h; the `windows` crate has it in Controls, not taken here).
#[repr(C)]
struct MeasureItem {
    ctl_type: u32,
    _ctl_id: u32,
    _item_id: u32,
    // Windows reads these (her answer), she only writes them.
    #[allow(dead_code)]
    item_width: u32,
    #[allow(dead_code)]
    item_height: u32,
    item_data: usize,
}

/// `DRAWITEMSTRUCT`.
#[repr(C)]
struct DrawItem {
    ctl_type: u32,
    _ctl_id: u32,
    _item_id: u32,
    _item_action: u32,
    item_state: u32,
    _hwnd_item: HWND,
    hdc: HDC,
    rc_item: RECT,
    item_data: usize,
}

const ODT_MENU: u32 = 1;
const ODS_SELECTED: u32 = 0x1;
const ODS_GRAYED: u32 = 0x2;
const ODS_DISABLED: u32 = 0x4;
/// The menu windows' class.
const MENU_CLASS: &str = "#32768";

/// A row of her menu as it's drawn: its item data is where its `name` is.
#[derive(Debug, Default)]
struct Line {
    label: String,
    mark: Mark,
    lead: bool,
    opens: bool,
    rule: bool,
    name: Name,
}

/// A row's words for a screen reader, beside the string the row keeps (oleacc's menu proxy reads
/// that one: the shots' tray scene, #112 round 2). A proxy that reads an owner-drawn row by its
/// item data (dotnet/wpf's `WindowsMenu.cs`, when `GetMenuString` gives it nothing) reads it as a
/// pointer to an `MSAAMENUINFO`, from her memory: this, boxed so it stays where it was pointed at.
struct Name(Box<Named>);

#[repr(C)]
struct Named {
    /// First: where the item data points.
    msaa: MSAAMENUINFO,
    /// Its words, ended by a 0 (`msaa.pszWText`).
    words: Vec<u16>,
}

impl Name {
    fn new(s: &str) -> Name {
        let mut words: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
        let msaa = MSAAMENUINFO { dwMSAASignature: MSAA_MENU_SIG as u32, cchWText: (words.len() - 1) as u32, pszWText: PWSTR(words.as_mut_ptr()) };
        Name(Box::new(Named { msaa, words }))
    }

    /// Its row's item data.
    fn data(&self) -> usize {
        std::ptr::from_ref(&self.0.msaa).expose_provenance()
    }
}

impl Default for Name {
    fn default() -> Name {
        Name::new("")
    }
}

impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let w = &self.0.words;
        write!(f, "Name({:?})", String::from_utf16_lossy(&w[..w.len() - 1]))
    }
}

/// Her menu's pen while it's up (`track`): its rows, its sizes and fonts at the DPI of the monitor
/// it opens on. GDI objects of this thread's, deleted with it. A submenu that opens on a monitor
/// of another scale keeps these sizes (only its frame takes its own: `paint_frame`); rare at the
/// tray's corner.
struct Pen {
    lines: Vec<Line>,
    dpi: u32,
    m: Metrics,
    font: HFONT,
    bold: HFONT,
    marks: HFONT,
    bg: HBRUSH,
}

impl Pen {
    fn new(lines: Vec<Line>, dpi: u32) -> Pen {
        let m = menulook::metrics(dpi);
        Pen {
            lines,
            dpi,
            m,
            font: font("Segoe UI", m.font, FW_SEMIBOLD.0 as i32, DEFAULT_CHARSET),
            bold: font("Segoe UI", m.font, FW_BOLD.0 as i32, DEFAULT_CHARSET),
            // DrawFrameControl's own marks: a choice's dot, the submenu arrow.
            marks: font("Marlett", m.glyph, FW_NORMAL.0 as i32, SYMBOL_CHARSET),
            bg: unsafe { CreateSolidBrush(rgb(COLORS.bg)) },
        }
    }

    /// The row whose item data is `data`: one of hers, or None.
    fn line(&self, data: usize) -> Option<&Line> {
        self.lines.iter().find(|l| l.name.data() == data)
    }

    /// The words' font: bold on the row that leads (SHOW while she's hidden).
    fn words(&self, line: &Line) -> HFONT {
        if line.lead { self.bold } else { self.font }
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        let objs: [HGDIOBJ; 4] = [self.font.into(), self.bold.into(), self.marks.into(), self.bg.into()];
        for o in objs {
            let _ = unsafe { DeleteObject(o) };
        }
    }
}

thread_local! {
    static PEN: RefCell<Option<Pen>> = const { RefCell::new(None) };
    /// Her menu windows and their own procs, while hers wraps them (`frame`).
    static PROCS: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
}

fn rgb(c: u32) -> COLORREF {
    COLORREF(colorref(c))
}

fn font(face: &str, px: i32, weight: i32, charset: FONT_CHARSET) -> HFONT {
    let mut lf = LOGFONTW { lfHeight: -px, lfWeight: weight, lfCharSet: charset, lfOutPrecision: OUT_TT_PRECIS, lfQuality: CLEARTYPE_QUALITY, ..Default::default() };
    for (d, s) in lf.lfFaceName.iter_mut().zip(face.encode_utf16()) {
        *d = s;
    }
    unsafe { CreateFontIndirectW(&lf) }
}

/// Her menu as her model laid it out, every row owner-drawn (`lines`: each one's words, marks and
/// name, its item data where the name is), or the stock rows (`stock`: high contrast): a row with
/// choices opens them, the lead row is the default, what can't act now is greyed
/// (`TrackPopupMenu` never picks it), the choice in force is checked (a radio check: a dot). Each
/// row keeps its words (`&` doubled: hers are written as they read).
fn build(items: &[TrayItem], lines: &mut Vec<Line>, stock: bool) -> ::windows::core::Result<HMENU> {
    unsafe {
        let m = CreatePopupMenu()?;
        for (pos, it) in items.iter().enumerate() {
            let (mut line, id, sub, live) = match it {
                TrayItem::Item { id, label, mark, lead, enabled } => (Line { label: label.clone(), mark: *mark, lead: *lead, ..Line::default() }, *id, None, *enabled),
                // A submenu not taken in is ours to destroy.
                TrayItem::Sub { label, items, enabled } => match build(items, lines, stock) {
                    Ok(sub) => (Line { label: label.clone(), opens: true, ..Line::default() }, 0, Some(sub), *enabled),
                    Err(e) => {
                        let _ = DestroyMenu(m);
                        return Err(e);
                    }
                },
                TrayItem::Rule => (Line { rule: true, ..Line::default() }, 0, None, true),
            };
            line.name = Name::new(&line.label);
            let mut state = if live { MENU_ITEM_STATE(0) } else { MFS_GRAYED };
            if line.mark.on() {
                state |= MFS_CHECKED;
            }
            if line.lead {
                state |= MFS_DEFAULT;
            }
            let mut kind = match (line.rule, stock) {
                (true, true) => MFT_SEPARATOR,
                (false, true) => MFT_STRING,
                (true, false) => MFT_OWNERDRAW | MFT_SEPARATOR,
                (false, false) => MFT_OWNERDRAW,
            };
            if matches!(line.mark, Mark::Choice(_)) {
                kind |= MFT_RADIOCHECK;
            }
            let mut text: Vec<u16> = line.label.replace('&', "&&").encode_utf16().chain(Some(0)).collect();
            let mut mi = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_DATA,
                fType: kind,
                fState: state,
                wID: id,
                dwItemData: line.name.data(),
                ..Default::default()
            };
            if !line.rule {
                mi.fMask |= MIIM_STRING;
                mi.dwTypeData = PWSTR(text.as_mut_ptr());
                mi.cch = (text.len() - 1) as u32;
            }
            if let Some(sub) = sub {
                mi.fMask |= MIIM_SUBMENU;
                mi.hSubMenu = sub;
            }
            lines.push(line);
            if let Err(e) = InsertMenuItemW(m, pos as u32, true, &mi) {
                if let Some(sub) = sub {
                    let _ = DestroyMenu(sub);
                }
                let _ = DestroyMenu(m);
                return Err(e);
            }
        }
        Ok(m)
    }
}

/// The DPI of the monitor at `at` (her menu opens there).
fn dpi_at(at: POINT) -> u32 {
    let (mut x, mut y) = (96, 96);
    let ok = unsafe { GetDpiForMonitor(MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_ok();
    if ok { x } else { 96 }
}

/// High contrast is on: his theme's colours over hers (the stock menu). None: Windows didn't say.
fn hc() -> Option<bool> {
    let mut h = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let ok = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, h.cbSize, Some(&mut h as *mut HIGHCONTRASTW as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok();
    ok.then(|| h.dwFlags.contains(HCF_HIGHCONTRASTON))
}

/// Her menu at `at` (screen px), owned by her tray's window `h`, until he picks (a `WM_COMMAND`
/// to `h`) or clicks away. Owner-drawn, or the stock menu under high contrast (asked each open:
/// turned on mid-session, the next one follows it).
pub(super) fn track(h: HWND, at: POINT, items: &[TrayItem]) -> ::windows::core::Result<()> {
    // Already up: that one stays (a new pen would delete the fonts it's drawn with).
    if PEN.with(|p| p.borrow().is_some()) {
        return Ok(());
    }
    let stock = hc().unwrap_or(false);
    let mut lines = Vec::new();
    let m = build(items, &mut lines, stock)?;
    // Her rows' words for a typed letter, the stock menu's too.
    let pen = Pen::new(lines, dpi_at(at));
    if !stock {
        // Her black under the rows (the margins too), the submenu's as well.
        let info = MENUINFO { cbSize: size_of::<MENUINFO>() as u32, fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS, hbrBack: pen.bg, ..Default::default() };
        let _ = unsafe { SetMenuInfo(m, &info) };
    }
    PEN.with(|p| *p.borrow_mut() = Some(pen));
    // Her frame on each of its windows as it's made (the menu's, a submenu's).
    let hook = if stock { None } else { unsafe { SetWindowsHookExW(WH_CBT, Some(cbt), None, GetCurrentThreadId()) }.ok() };
    unsafe {
        // In front, or a click elsewhere won't close it (the shell's own sample does the same).
        let _ = SetForegroundWindow(h);
        let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
        let _ = TrackPopupMenu(m, TPM_RIGHTBUTTON | align, at.x, at.y, None, h, None);
        let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
        if let Some(k) = hook {
            let _ = UnhookWindowsHookEx(k);
        }
        let _ = DestroyMenu(m);
    }
    PEN.with(|p| p.borrow_mut().take());
    Ok(())
}

/// Her menu's messages to her tray's window while it's up: a row's size, a row drawn, a typed
/// letter. None: not hers (the default handling).
pub(super) fn on_message(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_MEASUREITEM => {
            // SAFETY: WM_MEASUREITEM's lParam is the sender's MEASUREITEMSTRUCT, for this call.
            let mi = unsafe { &mut *(lp.0 as *mut MeasureItem) };
            let done = mi.ctl_type == ODT_MENU && PEN.with(|p| p.borrow().as_ref().is_some_and(|pen| measure(pen, mi)));
            done.then_some(LRESULT(1))
        }
        WM_DRAWITEM => {
            // SAFETY: WM_DRAWITEM's lParam is the sender's DRAWITEMSTRUCT, for this call.
            let di = unsafe { &*(lp.0 as *const DrawItem) };
            let done = di.ctl_type == ODT_MENU && PEN.with(|p| p.borrow().as_ref().is_some_and(|pen| draw(pen, di)));
            done.then_some(LRESULT(1))
        }
        WM_MENUCHAR => {
            let c = char::from_u32((wp.0 & 0xffff) as u32)?;
            PEN.with(|p| p.borrow().as_ref().and_then(|pen| typed_in(pen, HMENU(lp.0 as *mut _), c)))
        }
        _ => None,
    }
}

/// A row's size: its words' width in its font at the menu's DPI, its mark and arrow columns.
fn measure(pen: &Pen, mi: &mut MeasureItem) -> bool {
    let Some(line) = pen.line(mi.item_data) else { return false };
    let m = &pen.m;
    if line.rule {
        mi.item_height = m.rule as u32;
        mi.item_width = 0;
    } else {
        // Windows adds a check mark's width to what an owner-drawn row asks for: taken off.
        let check = unsafe { GetSystemMetricsForDpi(SM_CXMENUCHECK, pen.dpi) } - 1;
        mi.item_width = (menulook::row_width(text_width(pen, line), m) - check.max(0)).max(0) as u32;
        mi.item_height = m.row as u32;
    }
    true
}

fn text_width(pen: &Pen, line: &Line) -> i32 {
    let w: Vec<u16> = line.label.encode_utf16().collect();
    let mut s = SIZE::default();
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, pen.words(line).into());
        SetTextCharacterExtra(dc, pen.m.track);
        let _ = GetTextExtentPoint32W(dc, &w, &mut s);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
    }
    s.cx
}

fn draw(pen: &Pen, di: &DrawItem) -> bool {
    let Some(line) = pen.line(di.item_data) else { return false };
    paint_line(pen, line, di.hdc, di.rc_item, di.item_state);
    true
}

fn text(dc: HDC, s: &str, mut rc: RECT, f: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    unsafe { DrawTextW(dc, &mut w, &mut rc, f) };
}

fn fill(dc: HDC, rc: RECT, c: u32) {
    unsafe {
        let b = CreateSolidBrush(rgb(c));
        FillRect(dc, &rc, b);
        let _ = DeleteObject(b.into());
    }
}

/// A `w` px line of `c` just inside `r`.
fn outline(dc: HDC, r: RECT, w: i32, c: u32) {
    for edge in [RECT { bottom: r.top + w, ..r }, RECT { top: r.bottom - w, ..r }, RECT { right: r.left + w, ..r }, RECT { left: r.right - w, ..r }] {
        fill(dc, edge, c);
    }
}

/// One row in `r` (`state`: the ODS_ flags): her black, or (lit and live) her red blade with its
/// bottom right corner cut, as her own menu's; lit but greyed (the arrow keys stop on it), a dim
/// line around it: where they are. Its mark, its words, its arrow. Nothing of the system's goes
/// over it after (its own arrow and check are black: clipped out).
fn paint_line(pen: &Pen, line: &Line, dc: HDC, r: RECT, state: u32) {
    let m = &pen.m;
    let live = state & (ODS_GRAYED | ODS_DISABLED) == 0;
    let lit = live && state & ODS_SELECTED != 0 && !line.rule;
    let cell = |left: i32, right: i32| RECT { left, top: r.top, right, bottom: r.bottom };
    unsafe {
        let saved = SaveDC(dc);
        fill(dc, r, COLORS.bg);
        if line.rule {
            let y = r.top + (r.bottom - r.top - m.line) / 2;
            fill(dc, RECT { left: r.left + m.pad, top: y, right: r.right - m.pad, bottom: y + m.line }, COLORS.rule);
        } else {
            if lit {
                let b = CreateSolidBrush(rgb(COLORS.blade));
                let (ob, op) = (SelectObject(dc, b.into()), SelectObject(dc, GetStockObject(NULL_PEN)));
                let blade = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.top }, POINT { x: r.right - m.cut, y: r.bottom }, POINT { x: r.left, y: r.bottom }];
                let _ = Polygon(dc, &blade);
                SelectObject(dc, op);
                SelectObject(dc, ob);
                let _ = DeleteObject(b.into());
            } else if !live && state & ODS_SELECTED != 0 {
                outline(dc, r, m.line, COLORS.dim);
            }
            let ink = if lit { COLORS.on_blade } else if live { COLORS.item } else { COLORS.dim };
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, rgb(ink));
            let center = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
            SelectObject(dc, pen.marks.into());
            // The choice in force: Marlett's dot.
            if line.mark.on() {
                text(dc, "h", cell(r.left + m.pad, r.left + m.pad + m.mark), center);
            }
            if line.opens {
                text(dc, "8", cell(r.right - m.pad - m.arrow, r.right - m.pad), center);
            }
            SelectObject(dc, pen.words(line).into());
            SetTextCharacterExtra(dc, m.track);
            text(dc, &line.label, cell(r.left + m.pad + m.mark, r.right - m.pad - m.arrow), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        }
        // Its fonts and colours back as they were; then the row is closed to the system's drawing.
        let _ = RestoreDC(dc, saved);
        ExcludeClipRect(dc, r.left, r.top, r.right, r.bottom);
    }
}

/// A typed letter in `menu` (her menu, or ATTENTION's): to the row it starts (`menulook::typed`).
fn typed_in(pen: &Pen, menu: HMENU, c: char) -> Option<LRESULT> {
    let n = unsafe { GetMenuItemCount(Some(menu)) };
    let mut rows = Vec::new();
    let mut lit = None;
    for pos in 0..n.max(0) as u32 {
        let mut mi = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_DATA | MIIM_STATE | MIIM_SUBMENU, ..Default::default() };
        unsafe { GetMenuItemInfoW(menu, pos, true, &mut mi) }.ok()?;
        let line = pen.line(mi.dwItemData)?;
        if mi.fState.0 & MFS_HILITE.0 != 0 {
            lit = Some(pos as usize);
        }
        rows.push(Keyed { label: &line.label, live: !line.rule && mi.fState.0 & MFS_GRAYED.0 == 0, opens: !mi.hSubMenu.is_invalid() });
    }
    let go = |pos: usize, how: u32| Some(LRESULT(((how << 16) | pos as u32) as isize));
    match menulook::typed(&rows, c, lit) {
        Typed::Nothing => None,
        Typed::Select(i) => go(i, MNC_SELECT),
        Typed::Run(i) => go(i, MNC_EXECUTE),
    }
}

/// Each menu window of this thread's as it's made (while her menu is up): framed as hers.
unsafe extern "system" fn cbt(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HCBT_CREATEWND as i32 {
        let h = HWND(wp.0 as *mut _);
        if super::class_name(h) == MENU_CLASS {
            frame(h);
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// Her frame on a menu window of hers: square (11 rounds it), her dark red line around it (DWM's
/// border on 11; drawn on 10), the non-client strip her black, not the system's light grey: its
/// own proc wrapped till it goes.
fn frame(h: HWND) {
    unsafe {
        let set = |attr, v: u32| DwmSetWindowAttribute(h, attr, &v as *const u32 as *const _, 4).ok();
        set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
        set(DWMWA_BORDER_COLOR, colorref(COLORS.frame));
        let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = menu_proc;
        let own = SetWindowLongPtrW(h, GWLP_WNDPROC, proc as usize as isize);
        if own != 0 {
            PROCS.with(|p| p.borrow_mut().push((h.0 as isize, own)));
        }
    }
}

unsafe extern "system" fn menu_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let own = PROCS.with(|p| p.borrow().iter().find(|(w, _)| *w == h.0 as isize).map(|&(_, f)| f));
    let Some(own) = own else { return unsafe { DefWindowProcW(h, msg, wp, lp) } };
    // SAFETY: what GWLP_WNDPROC held before `frame`: the menu class's own proc.
    let own: WNDPROC = Some(unsafe { std::mem::transmute::<*const (), unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(std::ptr::with_exposed_provenance(own as usize)) });
    let r = unsafe { CallWindowProcW(own, h, msg, wp, lp) };
    match msg {
        WM_NCPAINT => unsafe {
            let dc = GetWindowDC(Some(h));
            paint_frame(h, dc);
            ReleaseDC(Some(h), dc);
        },
        // Its picture for the fade in (the menu animation draws it off screen first).
        WM_PRINT if lp.0 & PRF_NONCLIENT as isize != 0 => paint_frame(h, HDC(wp.0 as *mut _)),
        WM_NCDESTROY => PROCS.with(|p| p.borrow_mut().retain(|(w, _)| *w != h.0 as isize)),
        _ => {}
    }
    r
}

/// A menu window's non-client strip in her black, her dark red line around it (a line's width at
/// its DPI): the system's is light grey.
fn paint_frame(h: HWND, dc: HDC) {
    let (mut w, mut c, mut o) = (RECT::default(), RECT::default(), POINT::default());
    unsafe {
        if GetWindowRect(h, &mut w).is_err() || GetClientRect(h, &mut c).is_err() || !ClientToScreen(h, &mut o).as_bool() {
            return;
        }
        let (ww, wh, cx, cy) = (w.right - w.left, w.bottom - w.top, o.x - w.left, o.y - w.top);
        let line = menulook::metrics(GetDpiForWindow(h)).line;
        let saved = SaveDC(dc);
        ExcludeClipRect(dc, cx, cy, cx + c.right, cy + c.bottom);
        fill(dc, RECT { left: 0, top: 0, right: ww, bottom: wh }, COLORS.frame);
        fill(dc, RECT { left: line, top: line, right: ww - line, bottom: wh - line }, COLORS.bg);
        let _ = RestoreDC(dc, saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{self, State as Menu};
    use crate::settings::Attention;
    use ::windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLR_INVALID, CreateDIBSection, DIB_RGB_COLORS, GdiFlush, GetPixel, HBITMAP};

    /// A row's words as Windows keeps them (`MIIM_STRING`), owner-drawn or not.
    fn words(m: HMENU, pos: u32) -> String {
        let mut b = [0u16; 128];
        let mut mi = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_STRING, dwTypeData: PWSTR(b.as_mut_ptr()), cch: b.len() as u32, ..Default::default() };
        unsafe { GetMenuItemInfoW(m, pos, true, &mut mi) }.unwrap();
        String::from_utf16_lossy(&b[..mi.cch as usize])
    }

    /// Her row by its item data.
    fn at(lines: &[Line], data: usize) -> &Line {
        lines.iter().find(|l| l.name.data() == data).expect("one of her rows")
    }

    fn data(m: HMENU, pos: u32) -> usize {
        let mut mi = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_DATA, ..Default::default() };
        unsafe { GetMenuItemInfoW(m, pos, true, &mut mi) }.unwrap();
        mi.dwItemData
    }

    fn ftype(m: HMENU, pos: u32) -> u32 {
        let mut mi = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_FTYPE, ..Default::default() };
        unsafe { GetMenuItemInfoW(m, pos, true, &mut mi) }.unwrap();
        mi.fType.0
    }

    /// Its flags (a submenu's count is in the high byte: its flags read in the low one).
    fn state(m: HMENU, pos: u32) -> u32 {
        unsafe { GetMenuState(m, pos, MF_BYPOSITION) }
    }

    #[test]
    fn the_menu_is_drawn_as_her_model_lays_it_out() {
        let why = crate::line::MenuSave::Refused.says();
        let s = Menu { hidden: true, dnd: Some(false), attention: Some(Attention::HangOnly), handsfree: Some(true), unsaved: why, ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let mut lines = Vec::new();
        let m = build(&items, &mut lines, false).unwrap();
        unsafe {
            assert_eq!(GetMenuItemCount(Some(m)), items.len() as i32);
            for (i, it) in items.iter().enumerate() {
                let (pos, st) = (i as u32, state(m, i as u32));
                assert!(ftype(m, pos) & MFT_OWNERDRAW.0 != 0, "row {i}: hers to draw");
                let line = at(&lines, data(m, pos));
                match it {
                    TrayItem::Item { id, label, mark, .. } => {
                        assert_eq!((GetMenuItemID(m, i as i32), words(m, pos), &line.label), (*id, label.clone(), label));
                        assert_eq!(st & MF_CHECKED.0 != 0, mark.on(), "{label}: checked as its mark says");
                        assert_eq!(line.mark, *mark);
                    }
                    TrayItem::Sub { label, items, .. } => {
                        assert_eq!(words(m, pos), *label);
                        assert!(line.opens);
                        let sub = GetSubMenu(m, i as i32);
                        assert!(!sub.is_invalid());
                        assert_eq!(GetMenuItemCount(Some(sub)), items.len() as i32);
                        let checked: Vec<bool> = (0..3).map(|j| state(sub, j) & MF_CHECKED.0 != 0).collect();
                        assert_eq!(checked, [false, true, false], "HANG ONLY in force");
                        assert!((0..3).all(|j| matches!(at(&lines, data(sub, j)).mark, Mark::Choice(_))), "a dot, not a check");
                        assert!((0..3).all(|j| ftype(sub, j) & MFT_RADIOCHECK.0 != 0), "a radio choice, for a screen reader");
                        // Its last pick didn't save: why, under a rule, greyed (no pick).
                        assert!(state(sub, 3) & MF_SEPARATOR.0 != 0);
                        assert!(state(sub, 4) & MF_GRAYED.0 != 0);
                        assert_eq!((GetMenuItemID(sub, 4), words(sub, 4)), (0, why.unwrap().to_string()));
                    }
                    TrayItem::Rule => assert!(st & MF_SEPARATOR.0 != 0 && line.rule),
                }
            }
            // Every row, the submenu's too, has its line.
            assert_eq!(lines.len(), items.len() + 5);
            let checked: Vec<&str> = (0..items.len() as u32).filter(|&p| state(m, p) & MF_CHECKED.0 != 0).map(|p| at(&lines, data(m, p)).label.as_str()).collect();
            assert!(checked.is_empty(), "a switch has no tick, on or off (END HANDS-FREE says what a click does): {checked:?}");
            assert_eq!(GetMenuDefaultItem(m, 0, GET_MENU_DEFAULT_ITEM_FLAGS(0)), menu::Item::ShowHide.id(), "SHOW leads while she is hidden");
            DestroyMenu(m).unwrap();
        }
    }

    /// Each row's words as a proxy that reads an owner-drawn row by its item data does
    /// (dotnet/wpf's `WindowsMenu.cs`): an `MSAAMENUINFO`, MSAA_MENU_SIG first, its length, its
    /// words ended by a 0; ATTENTION's choices and the note under them too. Rules have none.
    #[test]
    fn a_screen_reader_reads_her_rows_words() {
        let why = crate::line::MenuSave::Refused.says().unwrap();
        let s = Menu { dnd: Some(true), attention: Some(Attention::Mix), handsfree: Some(false), unsaved: Some(why), ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let mut lines = Vec::new();
        let m = build(&items, &mut lines, false).unwrap();
        let read = |m: HMENU, pos: u32| {
            // SAFETY: her row's item data: its `Named`, alive while `lines` is (as her memory is
            // to the proxy while the menu's up).
            let info = unsafe { &*std::ptr::with_exposed_provenance::<MSAAMENUINFO>(data(m, pos)) };
            assert_eq!(info.dwMSAASignature, MSAA_MENU_SIG as u32, "row {pos}: MSAA_MENU_SIG first");
            let w = unsafe { std::slice::from_raw_parts(info.pszWText.0, info.cchWText as usize + 1) };
            assert_eq!(w.last(), Some(&0), "row {pos}: its words ended by a 0");
            String::from_utf16(&w[..w.len() - 1]).unwrap()
        };
        let names = |m: HMENU| -> Vec<String> {
            let n = unsafe { GetMenuItemCount(Some(m)) }.max(0) as u32;
            (0..n).filter(|&p| state(m, p) & MF_SEPARATOR.0 == 0).map(|p| read(m, p)).collect()
        };
        assert_eq!(names(m), ["RESET POSITION", "DND OFF", "ATTENTION: MIX", "HANDS-FREE TALK", "COPY LAST REPLY", "HIDE", "SETTINGS", "QUIT"]);
        let sub = unsafe { GetSubMenu(m, 2) };
        assert_eq!(names(sub), ["MIX", "HANG ONLY", "OFF", why]);
        unsafe { DestroyMenu(m).unwrap() };
    }

    /// While she quits, every row but QUIT is greyed (ATTENTION's choices with it).
    #[test]
    fn what_cant_act_now_is_greyed_in_the_tray() {
        let s = Menu { quitting: true, dnd: Some(false), attention: Some(Attention::Mix), handsfree: Some(false), ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let m = build(&items, &mut Vec::new(), false).unwrap();
        unsafe {
            let mut greyed = Vec::new();
            for (i, it) in items.iter().enumerate() {
                let g = state(m, i as u32) & MF_GRAYED.0 != 0;
                match it {
                    TrayItem::Item { label, .. } => greyed.push((label.clone(), g)),
                    TrayItem::Sub { label, .. } => {
                        greyed.push((label.clone(), g));
                        let sub = GetSubMenu(m, i as i32);
                        assert!((0..3).all(|j| state(sub, j) & MF_GRAYED.0 != 0));
                    }
                    TrayItem::Rule => {}
                }
            }
            let live: Vec<&str> = greyed.iter().filter(|(_, g)| !g).map(|(l, _)| l.as_str()).collect();
            assert_eq!(live, ["QUIT"], "{greyed:?}");
            DestroyMenu(m).unwrap();
        }
    }

    /// Under high contrast (`hc`): the stock menu, in his theme's colours. No row of hers to draw,
    /// rules real separators, the same words, checks, default and greyed rows; ATTENTION's choices
    /// radio checks (Windows' dot). Each row still points at its line (a typed letter).
    #[test]
    fn under_high_contrast_it_is_the_stock_menu() {
        let s = Menu { hidden: true, dnd: Some(true), attention: Some(Attention::Mix), handsfree: Some(true), intro: true, ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let mut lines = Vec::new();
        let m = build(&items, &mut lines, true).unwrap();
        let stock = |m: HMENU, pos: u32, rule: bool| {
            let t = ftype(m, pos);
            assert_eq!(t & MFT_OWNERDRAW.0, 0, "row {pos}: the system's to draw");
            assert_eq!(t & MFT_SEPARATOR.0 != 0, rule, "row {pos}: a separator only for a rule");
            t
        };
        unsafe {
            assert_eq!(GetMenuItemCount(Some(m)), items.len() as i32);
            for (i, it) in items.iter().enumerate() {
                let (pos, st) = (i as u32, state(m, i as u32));
                let t = stock(m, pos, matches!(it, TrayItem::Rule));
                match it {
                    TrayItem::Item { id, label, enabled, .. } => {
                        assert_eq!((GetMenuItemID(m, i as i32), words(m, pos), &at(&lines, data(m, pos)).label), (*id, label.clone(), label));
                        assert_eq!((t & MFT_RADIOCHECK.0, st & MF_CHECKED.0), (0, 0), "{label}: no tick on a switch");
                        assert_eq!(st & MF_GRAYED.0 == 0, *enabled, "{label}");
                    }
                    TrayItem::Sub { label, .. } => {
                        assert_eq!(words(m, pos), *label);
                        let sub = GetSubMenu(m, i as i32);
                        assert!(!sub.is_invalid());
                        for j in 0..3 {
                            assert!(stock(sub, j, false) & MFT_RADIOCHECK.0 != 0, "choice {j}: Windows' dot");
                        }
                        let checked: Vec<bool> = (0..3).map(|j| state(sub, j) & MF_CHECKED.0 != 0).collect();
                        assert_eq!(checked, [true, false, false], "MIX in force");
                    }
                    TrayItem::Rule => {}
                }
            }
            // Her intro: RESET POSITION greyed (END HANDS-FREE still acts), in his colours as in hers.
            let greyed: Vec<String> = (0..items.len()).filter(|&i| !matches!(items[i], TrayItem::Rule) && state(m, i as u32) & MF_GRAYED.0 != 0).map(|i| words(m, i as u32)).collect();
            assert_eq!(greyed, ["RESET POSITION"]);
            assert_eq!(GetMenuDefaultItem(m, 0, GET_MENU_DEFAULT_ITEM_FLAGS(0)), menu::Item::ShowHide.id(), "SHOW leads while she is hidden");
            DestroyMenu(m).unwrap();
        }
        // Asked as Windows answers it: an answer, on or off (the size it's asked with is right).
        assert!(hc().is_some(), "SPI_GETHIGHCONTRAST answered");
    }

    /// A 32-bit canvas `w`×`h` to draw rows on, and its pixels as 0xRRGGBB, read from its own
    /// memory: a drawn row closes itself to the DC (its clip), and `GetPixel` through that DC
    /// answers CLR_INVALID (white, read as a colour) for every pixel of it.
    struct Canvas {
        dc: HDC,
        bmp: HBITMAP,
        bits: *const u32,
        w: i32,
        h: i32,
    }

    impl Canvas {
        fn new(w: i32, h: i32) -> Canvas {
            let header = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
            let bmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            unsafe {
                let dc = CreateCompatibleDC(None);
                let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
                SelectObject(dc, bmp.into());
                fill(dc, RECT { left: 0, top: 0, right: w, bottom: h }, 0xffffff);
                Canvas { dc, bmp, bits: bits as *const u32, w, h }
            }
        }
        /// Top-down BI_RGB: a pixel is 0x__RRGGBB in memory order (blue first).
        fn px(&self, x: i32, y: i32) -> u32 {
            assert!((0..self.w).contains(&x) && (0..self.h).contains(&y), "({x}, {y}) off the canvas");
            unsafe {
                let _ = GdiFlush();
                *self.bits.add((y * self.w + x) as usize) & 0xff_ffff
            }
        }
        /// Closed to the DC's drawing at (x, y): outside its clip.
        fn closed(&self, x: i32, y: i32) -> bool {
            unsafe { GetPixel(self.dc, x, y) }.0 == CLR_INVALID
        }
        /// Every pixel of `r`.
        fn all(&self, r: RECT) -> Vec<u32> {
            (r.top..r.bottom).flat_map(|y| (r.left..r.right).map(move |x| (x, y))).map(|(x, y)| self.px(x, y)).collect()
        }
    }

    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.dc);
                let _ = DeleteObject(self.bmp.into());
            }
        }
    }

    fn red(c: u32) -> bool {
        let (r, g, b) = (c >> 16, (c >> 8) & 0xff, c & 0xff);
        r >= 0xc0 && g < 0x60 && b < 0x80
    }

    fn dark(c: u32) -> bool {
        (c >> 16) < 0x40 && ((c >> 8) & 0xff) < 0x40 && (c & 0xff) < 0x40
    }

    fn grey(c: u32) -> bool {
        let (r, g, b) = ((c >> 16) as i32, ((c >> 8) & 0xff) as i32, (c & 0xff) as i32);
        r > 0x70 && (r - g).abs() < 0x30 && (r - b).abs() < 0x30
    }

    /// Her rows on a canvas, as her menu draws them in its window: black, her red words; lit, ink
    /// on her red blade (its corner cut); can't act, dim words and no blade (lit by the arrow keys,
    /// a dim line around it); the rule her dark red; a dot on the choice in force, round, none on
    /// the others; the arrow on a row that opens. Nothing goes over a row after.
    #[test]
    fn her_rows_are_red_on_black() {
        let row = |label: &str, mark: Mark, opens: bool| Line { label: label.into(), mark, opens, ..Line::default() };
        let pen = Pen::new(vec![row("RESET POSITION", Mark::None, false), Line { rule: true, ..Line::default() }, row("MIX", Mark::Choice(true), false), row("ATTENTION: MIX", Mark::None, true), row("HANG ONLY", Mark::Choice(false), false)], 96);
        let m = pen.m;
        let (w, h) = (260, m.row);
        let r = RECT { left: 0, top: 0, right: w, bottom: h };
        let words = RECT { left: m.pad + m.mark, top: 0, right: w - m.pad - m.arrow, bottom: h };
        let mark = RECT { left: m.pad, top: 0, right: m.pad + m.mark, bottom: h };
        let arrow = RECT { left: w - m.pad - m.arrow, top: 0, right: w - m.pad, bottom: h };
        let drawn = |i: usize, state: u32| {
            let c = Canvas::new(w, h);
            paint_line(&pen, &pen.lines[i], c.dc, r, state);
            // The system's arrow and check come after a row is drawn: none lands on it.
            fill(c.dc, r, 0xffffff);
            c
        };

        let c = drawn(0, 0);
        assert!(c.closed(1, 1) && c.closed(w - 1, h - 1), "a drawn row is closed to the system's drawing (its clip)");
        assert_eq!(c.px(1, 1), COLORS.bg, "her black");
        assert!(c.all(words).into_iter().filter(|&p| red(p)).count() > 20, "her red words");
        assert!(c.all(mark).into_iter().all(|p| p == COLORS.bg), "no mark");

        let c = drawn(0, ODS_SELECTED);
        assert_eq!(c.px(1, 1), COLORS.blade, "lit: her red blade");
        assert_eq!(c.px(w - 2, h - 1), COLORS.bg, "its bottom right corner cut");
        assert!(c.all(words).into_iter().filter(|&p| dark(p)).count() > 20, "ink words on it");

        // Its words, clear of a line around the row.
        let inner = RECT { top: m.line, bottom: h - m.line, ..words };
        for greyed in [ODS_SELECTED | ODS_GRAYED, ODS_SELECTED | ODS_DISABLED, ODS_GRAYED] {
            let c = drawn(0, greyed);
            assert_eq!(c.px(m.line, m.line), COLORS.bg, "can't act: no blade, lit or not");
            assert!(c.all(inner).into_iter().any(grey) && !c.all(inner).into_iter().any(red), "dim words");
            let edge = [c.px(0, 0), c.px(w / 2, 0), c.px(w - 1, h - 1), c.px(0, h / 2)];
            let want = if greyed & ODS_SELECTED != 0 { COLORS.dim } else { COLORS.bg };
            assert_eq!(edge, [want; 4], "lit by the arrow keys: a dim line around it ({greyed:#x}); else none");
        }

        let c = drawn(1, 0);
        let y = (h - m.line) / 2;
        assert_eq!((c.px(1, y), c.px(m.pad + 4, y), c.px(m.pad + 4, 0)), (COLORS.bg, COLORS.rule, COLORS.bg), "the rule: her dark red line, inset");
        assert_eq!(drawn(1, ODS_SELECTED).px(1, 1), COLORS.bg, "a rule never lights");

        // The choice in force: a dot, round (as wide as tall, give or take its smoothing).
        let c = drawn(2, 0);
        let dot: Vec<(i32, i32)> = (mark.top..mark.bottom).flat_map(|y| (mark.left..mark.right).map(move |x| (x, y))).filter(|&(x, y)| red(c.px(x, y))).collect();
        assert!(dot.len() >= 4, "the choice in force: its dot ({} px)", dot.len());
        let span = |k: fn(&(i32, i32)) -> i32| dot.iter().map(k).max().unwrap() - dot.iter().map(k).min().unwrap() + 1;
        let (dw, dh) = (span(|p| p.0), span(|p| p.1));
        assert!((dw - dh).abs() <= 2, "a dot, not a check: {dw}x{dh}");
        assert!(drawn(4, 0).all(mark).into_iter().all(|p| p == COLORS.bg), "a choice not in force: no dot");
        assert!(drawn(3, 0).all(arrow).into_iter().any(red), "her arrow on the row that opens");
        assert!(drawn(0, 0).all(arrow).into_iter().all(|p| p == COLORS.bg));
    }

    /// Sizes as Windows asks for them: a row's height her row's at the menu's DPI, wider at 150 %,
    /// a rule its own.
    #[test]
    fn rows_are_measured_at_the_menus_dpi() {
        let lines = || vec![Line { label: "COPY LAST REPLY".into(), ..Line::default() }, Line { rule: true, ..Line::default() }];
        let ask = |pen: &Pen, i: usize| {
            let mut mi = MeasureItem { ctl_type: ODT_MENU, _ctl_id: 0, _item_id: 0, item_width: 0, item_height: 0, item_data: pen.lines[i].name.data() };
            assert!(measure(pen, &mut mi));
            (mi.item_width as i32, mi.item_height as i32)
        };
        let (a, b) = (Pen::new(lines(), 96), Pen::new(lines(), 144));
        let ((wa, ha), (wb, hb)) = (ask(&a, 0), ask(&b, 0));
        assert_eq!((ha, hb), (28, 42));
        assert!(wb > wa * 13 / 10 && wa >= a.m.min_w / 2, "{wa} at 100 %, {wb} at 150 %");
        assert_eq!((ask(&a, 1).1, ask(&b, 1).1), (9, 14));
        let mut stray = MeasureItem { ctl_type: ODT_MENU, _ctl_id: 0, _item_id: 0, item_width: 0, item_height: 0, item_data: 9 };
        assert!(!measure(&a, &mut stray), "not one of hers");
    }

    /// A typed letter in her open menu (WM_MENUCHAR, as Windows sends it to her tray's window): R
    /// picks RESET POSITION, Q QUIT; H lights HANDS-FREE TALK (HIDE starts with H too); A lights
    /// ATTENTION (it opens); Z: Windows' own (a beep).
    #[test]
    fn a_typed_letter_goes_to_her_row() {
        let s = Menu { dnd: Some(false), attention: Some(Attention::Mix), handsfree: Some(false), ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let mut lines = Vec::new();
        let m = build(&items, &mut lines, false).unwrap();
        PEN.with(|p| *p.borrow_mut() = Some(Pen::new(lines, 96)));
        let key = |c: char| on_message(WM_MENUCHAR, WPARAM(c as usize), LPARAM(m.0 as isize)).map(|r| ((r.0 >> 16) as u32, (r.0 & 0xffff) as usize));
        assert_eq!(key('r'), Some((MNC_EXECUTE, 0)));
        assert_eq!(key('Q'), Some((MNC_EXECUTE, items.len() - 1)));
        assert_eq!(key('h'), Some((MNC_SELECT, 3)));
        assert_eq!(key('a'), Some((MNC_SELECT, 2)));
        assert_eq!(key('z'), None);
        PEN.with(|p| p.borrow_mut().take());
        assert_eq!(key('r'), None, "no menu of hers up: Windows' own");
        unsafe { DestroyMenu(m).unwrap() };
    }
}
