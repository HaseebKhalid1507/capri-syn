//! Her tray menu's look, as the numbers a desktop draws it by (Windows owner-draws it:
//! `platform::windows::traymenu`): her own menu's colours (`menuview::menu_rows`, from
//! `theme::Pal`), its sizes at a DPI in whole pixels (crisp at 100, 150 and 200 %), and which row a
//! typed letter goes to. Plain numbers, so they are tested on every OS.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::theme::Pal;

/// Her menu's colours, 0xRRGGBB: her rows red on jet black, the hovered one ink on a red blade,
/// one that can't act now dim (no blade), the rule and the frame her dark reds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub bg: u32,
    pub item: u32,
    pub blade: u32,
    pub on_blade: u32,
    pub dim: u32,
    pub rule: u32,
    pub frame: u32,
}

pub const COLORS: Colors = {
    let p = Pal::SHARD;
    Colors { bg: p.bg, item: p.accent, blade: p.accent, on_blade: p.ink, dim: p.dim, rule: p.rule, frame: p.border }
};

/// Her menu's sizes at a DPI, device px (each a whole number: no row lands between pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// A row's height.
    pub row: i32,
    /// A rule's height (its line in the middle).
    pub rule: i32,
    /// Room left of the mark and right of the arrow.
    pub pad: i32,
    /// The mark's column (a check, a dot), left of the words.
    pub mark: i32,
    /// The submenu arrow's column, right of the words (kept on every row: the words line up).
    pub arrow: i32,
    /// The words' em, and the marks'.
    pub font: i32,
    pub glyph: i32,
    /// Space added after each letter (her rows are tracked).
    pub track: i32,
    /// The blade's cut: its bottom right corner, as her menu's (`Cut::Tab`).
    pub cut: i32,
    /// A line's thickness (the rule, the frame).
    pub line: i32,
    /// The least a row is wide.
    pub min_w: i32,
}

/// Her menu's sizes at `dpi` (96: 100 %).
pub fn metrics(dpi: u32) -> Metrics {
    let px = |v: f32| ((v * dpi.max(96) as f32 / 96.0).round() as i32).max(1);
    Metrics {
        row: px(28.0),
        rule: px(9.0),
        pad: px(10.0),
        mark: px(20.0),
        arrow: px(18.0),
        font: px(13.0),
        glyph: px(12.0),
        track: px(1.0),
        cut: px(8.0),
        line: px(1.0),
        min_w: px(168.0),
    }
}

/// A row's width for words `text_w` px wide: its mark, its words, the arrow's room.
pub fn row_width(text_w: i32, m: &Metrics) -> i32 {
    (m.pad + m.mark + text_w.max(0) + m.arrow + m.pad).max(m.min_w)
}

/// A row as a typed letter sees it.
#[derive(Clone, Copy, Debug)]
pub struct Keyed<'a> {
    pub label: &'a str,
    /// It can act now (and isn't a rule).
    pub live: bool,
    /// It opens a submenu.
    pub opens: bool,
}

/// What a typed letter does in her menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typed {
    Nothing,
    /// Light this row (more than one starts with it: the next after the lit one; or it opens a
    /// submenu: Enter or → opens it).
    Select(usize),
    /// Pick this row (the only one that starts with it).
    Run(usize),
}

/// The row a typed letter `c` goes to: a live row's first letter, any case. `lit`: the row lit now.
pub fn typed(rows: &[Keyed], c: char, lit: Option<usize>) -> Typed {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let want = fold(c);
    let hits: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.live && r.label.chars().find(|c| c.is_alphanumeric()).map(fold) == Some(want))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Typed::Nothing,
        [one] if !rows[*one].opens => Typed::Run(*one),
        _ => Typed::Select(hits.iter().copied().find(|&i| lit.is_some_and(|l| i > l)).unwrap_or(hits[0])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100, 150 and 200 %: every size a whole number of pixels, in proportion (150 % rounds
    /// half up), and the rows tall enough to hit.
    #[test]
    fn her_menu_is_whole_pixels_at_every_scale() {
        let (a, b, c) = (metrics(96), metrics(144), metrics(192));
        assert_eq!((a.row, b.row, c.row), (28, 42, 56));
        assert_eq!((a.font, b.font, c.font), (13, 20, 26));
        assert_eq!((a.line, b.line, c.line), (1, 2, 2));
        assert_eq!((a.cut, b.cut, c.cut), (8, 12, 16));
        assert_eq!((a.track, b.track, c.track), (1, 2, 2));
        assert_eq!(metrics(0), a, "no DPI: 100 %");
        for m in [a, b, c] {
            assert!(m.row >= m.font * 2, "{m:?}: room above and below the words");
            assert!(m.glyph <= m.mark && m.glyph <= m.arrow, "{m:?}: a mark fits its column");
            assert!(m.rule > m.line * 2, "{m:?}");
        }
        // 125 % and 175 % too (the runner's and many laptops').
        assert_eq!((metrics(120).row, metrics(168).row), (35, 49));
    }

    #[test]
    fn a_row_is_its_mark_its_words_and_the_arrows_room() {
        let m = metrics(96);
        assert_eq!(row_width(200, &m), 10 + 20 + 200 + 18 + 10);
        assert_eq!(row_width(10, &m), m.min_w, "a short row: the least width");
        assert_eq!(row_width(-5, &m), m.min_w);
        assert!(row_width(200, &metrics(144)) > row_width(200, &m));
    }

    fn rows<'a>(labels: &[&'a str]) -> Vec<Keyed<'a>> {
        labels.iter().map(|&label| Keyed { label, live: !label.is_empty(), opens: label.starts_with("ATTENTION") }).collect()
    }

    /// Her rows have no `&`: a typed letter goes to the first one it starts, as on her own menu's
    /// words. One: picked; more (HANDS-FREE TALK, HIDE): lit in turn; a submenu: lit.
    #[test]
    fn a_typed_letter_goes_to_her_row() {
        let r = rows(&["RESET POSITION", "DND 1 HOUR", "ATTENTION: MIX", "HANDS-FREE TALK", "COPY LAST REPLY", "HIDE", "SETTINGS", "", "QUIT"]);
        assert_eq!(typed(&r, 'r', None), Typed::Run(0));
        assert_eq!(typed(&r, 'Q', None), Typed::Run(8), "any case");
        assert_eq!(typed(&r, 'a', None), Typed::Select(2), "ATTENTION opens its choices: lit, not picked");
        assert_eq!(typed(&r, 'h', None), Typed::Select(3));
        assert_eq!(typed(&r, 'h', Some(3)), Typed::Select(5), "the next H");
        assert_eq!(typed(&r, 'h', Some(5)), Typed::Select(3), "round again");
        assert_eq!(typed(&r, 'z', None), Typed::Nothing);
        assert_eq!(typed(&r, '1', None), Typed::Nothing, "a word's first letter, not any");
        // What can't act now is skipped: HIDE greyed leaves HANDS-FREE TALK the only H.
        let mut g = r.clone();
        g[5].live = false;
        assert_eq!(typed(&g, 'h', None), Typed::Run(3));
        // ATTENTION's choices: MIX, HANG ONLY, OFF.
        let c = rows(&["MIX", "HANG ONLY", "OFF"]);
        assert_eq!((typed(&c, 'm', None), typed(&c, 'o', None)), (Typed::Run(0), Typed::Run(2)));
    }

    #[test]
    fn her_menu_wears_her_own_menus_colours() {
        let p = Pal::SHARD;
        assert_eq!((COLORS.bg, COLORS.item, COLORS.blade, COLORS.on_blade), (0x0a0a0b, 0xff1438, 0xff1438, p.ink));
        assert_eq!((COLORS.dim, COLORS.rule, COLORS.frame), (p.dim, p.rule, p.border));
    }
}
