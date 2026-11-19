//! Her menu, as one model. Her right-click menu (painted by the view, `menuview.rs`) and her
//! tray's menu (drawn by the desktop, `platform::Tray`) read the same rows, in the same order,
//! with the same words, and a pick on either goes through the same dispatch
//! (`Avatar::menu_do`). A surface only draws: what it can't show its own way (ATTENTION's
//! choices) it shows as the model gives it.

use crate::control::Command;
use crate::platform::{TrayEv, TrayItem};
use crate::settings::Attention;

/// One of her menu's items, as a pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// HIDE her (she glitches out, the process keeps running), or SHOW her again.
    ShowHide,
    /// RESET POSITION: home on the floor (shown again first, if hidden).
    Reset,
    /// DND 1 HOUR, or DND OFF.
    Dnd,
    /// ATTENTION: the next one (her menu's row cycles MIX → HANG ONLY → OFF), or this one (the
    /// tray's choices).
    Attention(Option<Attention>),
    /// HANDS-FREE TALK, or END HANDS-FREE.
    HandsFree,
    /// COPY LAST REPLY.
    Copy,
    /// SETTINGS.
    Settings,
    /// QUIT (her own menu asks twice).
    Quit,
}

/// Every pick, each once.
const ALL: [Item; 11] = [
    Item::ShowHide,
    Item::Reset,
    Item::Dnd,
    Item::Attention(None),
    Item::Attention(Some(Attention::Mix)),
    Item::Attention(Some(Attention::HangOnly)),
    Item::Attention(Some(Attention::Off)),
    Item::HandsFree,
    Item::Copy,
    Item::Settings,
    Item::Quit,
];

impl Item {
    /// Its id in a tray's menu (never 0: Windows' "nothing picked").
    pub fn id(self) -> u32 {
        match self {
            Item::ShowHide => 1,
            Item::Reset => 2,
            Item::Dnd => 3,
            Item::Attention(None) => 4,
            Item::Attention(Some(Attention::Mix)) => 5,
            Item::Attention(Some(Attention::HangOnly)) => 6,
            Item::Attention(Some(Attention::Off)) => 7,
            Item::HandsFree => 8,
            Item::Copy => 9,
            Item::Settings => 10,
            Item::Quit => 11,
        }
    }

    pub fn from_id(id: u32) -> Option<Item> {
        ALL.into_iter().find(|i| i.id() == id)
    }

    /// Its row's name in `state`'s buttons (`menu-attention`: a shot clicks it there).
    pub fn probe(self) -> &'static str {
        match self {
            Item::ShowHide => "menu-hide",
            Item::Reset => "menu-reset",
            Item::Dnd => "menu-dnd",
            Item::Attention(_) => "menu-attention",
            Item::HandsFree => "menu-handsfree",
            Item::Copy => "menu-copy",
            Item::Settings => "menu-settings",
            Item::Quit => "menu-quit",
        }
    }

    /// Its element id on her menu (`hide`, `attention`, ...).
    pub fn key(self) -> &'static str {
        &self.probe()["menu-".len()..]
    }
}

/// Where a pick came from: her own menu (open by her; it stays up where a row says so, and
/// QUIT asks twice) or her tray's (closed once picked: QUIT quits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Her,
    Tray,
}

/// What her menu's rows depend on, read off her each time (`Avatar::menu_state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Hidden (HIDE): SHOW leads.
    pub hidden: bool,
    /// DND on; None: no brain, no row.
    pub dnd: Option<bool>,
    /// Her attention; None: no row.
    pub attention: Option<Attention>,
    /// Hands-free on; None: no row (voice off, or `[voice] handsfree` off).
    pub handsfree: Option<bool>,
    /// QUIT was clicked once on her menu: the next click quits.
    pub quit_armed: bool,
}

/// One row of her menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub item: Item,
    pub label: &'static str,
    /// Its choices (ATTENTION's): each one's pick, its word, and whether it is the one in force.
    pub choices: Vec<(Item, &'static str, bool)>,
    /// A rule above it.
    pub rule: bool,
    /// It acts on the next click (QUIT? CLICK AGAIN): danger.
    pub armed: bool,
    /// The row that matters most now (SHOW while she is hidden: the way back).
    pub lead: bool,
}

impl Row {
    fn new(item: Item, label: &'static str) -> Row {
        Row { item, label, choices: Vec::new(), rule: false, armed: false, lead: false }
    }
}

/// Her menu, top to bottom.
pub fn rows(s: &State) -> Vec<Row> {
    let mut v = vec![Row::new(Item::Reset, "RESET POSITION")];
    if let Some(on) = s.dnd {
        v.push(Row::new(Item::Dnd, if on { "DND OFF" } else { "DND 1 HOUR" }));
    }
    if let Some(a) = s.attention {
        let label = match a {
            Attention::Mix => "ATTENTION: MIX",
            Attention::HangOnly => "ATTENTION: HANG ONLY",
            Attention::Off => "ATTENTION: OFF",
        };
        let choices = [Attention::Mix, Attention::HangOnly, Attention::Off].into_iter().map(|c| (Item::Attention(Some(c)), c.label(), c == a)).collect();
        v.push(Row { choices, ..Row::new(Item::Attention(None), label) });
    }
    if let Some(on) = s.handsfree {
        v.push(Row::new(Item::HandsFree, if on { "END HANDS-FREE" } else { "HANDS-FREE TALK" }));
    }
    v.push(Row::new(Item::Copy, "COPY LAST REPLY"));
    v.push(Row { lead: s.hidden, ..Row::new(Item::ShowHide, if s.hidden { "SHOW" } else { "HIDE" }) });
    v.push(Row::new(Item::Settings, "SETTINGS"));
    let quit = if s.quit_armed { "QUIT? CLICK AGAIN" } else { "QUIT" };
    v.push(Row { rule: true, armed: s.quit_armed, ..Row::new(Item::Quit, quit) });
    v
}

/// The rows as a tray draws them: a row with choices opens them (the tray's own arrow), one
/// checked.
pub fn tray_items(rows: &[Row]) -> Vec<TrayItem> {
    let mut v = Vec::new();
    for r in rows {
        if r.rule {
            v.push(TrayItem::Rule);
        }
        v.push(if r.choices.is_empty() {
            TrayItem::Item { id: r.item.id(), label: r.label.to_string(), checked: false, lead: r.lead }
        } else {
            let items = r.choices.iter().map(|&(i, l, on)| TrayItem::Item { id: i.id(), label: l.to_string(), checked: on, lead: false }).collect();
            TrayItem::Sub { label: r.label.to_string(), items }
        });
    }
    v
}

/// What a tray event asks of her: a click on the icon is her summon key's tap (the Line), a
/// pick is that item through `menu_do`.
pub fn command_for(ev: TrayEv) -> Option<Command> {
    match ev {
        TrayEv::Open => Some(Command::TalkTap),
        TrayEv::Pick(id) => Item::from_id(id).map(Command::MenuPick),
    }
}

/// Her menu's steps (MIX → HANG ONLY → OFF → MIX) from `from` to `to`.
pub fn attention_steps(from: Attention, to: Attention) -> usize {
    let mut a = from;
    let mut n = 0;
    while a != to && n < 3 {
        a = a.next();
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(s: &State) -> Vec<&'static str> {
        rows(s).iter().map(|r| r.label).collect()
    }

    #[test]
    fn her_menu_has_one_order_and_one_set_of_words() {
        let full = State { dnd: Some(false), attention: Some(Attention::Mix), handsfree: Some(false), ..State::default() };
        assert_eq!(labels(&full), ["RESET POSITION", "DND 1 HOUR", "ATTENTION: MIX", "HANDS-FREE TALK", "COPY LAST REPLY", "HIDE", "SETTINGS", "QUIT"]);
        // No brain, no hands-free: those rows go, the rest keep their order.
        assert_eq!(labels(&State::default()), ["RESET POSITION", "COPY LAST REPLY", "HIDE", "SETTINGS", "QUIT"]);
        let on = State { dnd: Some(true), attention: Some(Attention::HangOnly), handsfree: Some(true), quit_armed: true, ..full };
        assert_eq!(labels(&on), ["RESET POSITION", "DND OFF", "ATTENTION: HANG ONLY", "END HANDS-FREE", "COPY LAST REPLY", "HIDE", "SETTINGS", "QUIT? CLICK AGAIN"]);
        let r = rows(&on);
        let quit = r.last().unwrap();
        assert!(quit.rule && quit.armed, "QUIT sits under a rule, armed after one click");
        assert!(r.iter().filter(|r| r.rule).count() == 1 && r.iter().all(|r| !r.lead));
    }

    #[test]
    fn hidden_she_is_shown_from_the_same_row() {
        let r = rows(&State { hidden: true, ..State::default() });
        let show = r.iter().find(|r| r.item == Item::ShowHide).unwrap();
        assert_eq!((show.label, show.lead), ("SHOW", true));
        assert_eq!(r.iter().filter(|r| r.lead).count(), 1);
        assert_eq!(r.iter().position(|r| r.item == Item::ShowHide), rows(&State::default()).iter().position(|r| r.item == Item::ShowHide), "it keeps its place");
    }

    #[test]
    fn attention_cycles_on_her_menu_and_is_picked_in_the_tray() {
        let r = rows(&State { attention: Some(Attention::HangOnly), ..State::default() });
        let a = r.iter().find(|r| r.item == Item::Attention(None)).unwrap();
        assert_eq!(a.choices, [(Item::Attention(Some(Attention::Mix)), "MIX", false), (Item::Attention(Some(Attention::HangOnly)), "HANG ONLY", true), (Item::Attention(Some(Attention::Off)), "OFF", false)]);
        assert_eq!(attention_steps(Attention::Mix, Attention::Mix), 0);
        assert_eq!(attention_steps(Attention::Mix, Attention::Off), 2);
        assert_eq!(attention_steps(Attention::Off, Attention::Mix), 1);
        assert_eq!(attention_steps(Attention::HangOnly, Attention::Mix), 2);
    }

    #[test]
    fn the_tray_menu_is_her_menu() {
        let s = State { dnd: Some(false), attention: Some(Attention::Off), handsfree: Some(true), hidden: true, ..State::default() };
        let items = tray_items(&rows(&s));
        let item = |id: u32, label: &str, lead: bool| TrayItem::Item { id, label: label.into(), checked: false, lead };
        let pick = |a: Attention, on: bool| TrayItem::Item { id: Item::Attention(Some(a)).id(), label: a.label().into(), checked: on, lead: false };
        assert_eq!(
            items,
            [
                item(2, "RESET POSITION", false),
                item(3, "DND 1 HOUR", false),
                TrayItem::Sub { label: "ATTENTION: OFF".into(), items: vec![pick(Attention::Mix, false), pick(Attention::HangOnly, false), pick(Attention::Off, true)] },
                item(8, "END HANDS-FREE", false),
                item(9, "COPY LAST REPLY", false),
                item(1, "SHOW", true),
                item(10, "SETTINGS", false),
                TrayItem::Rule,
                item(11, "QUIT", false),
            ]
        );
    }

    #[test]
    fn every_pick_has_its_own_id_and_comes_back() {
        let ids: std::collections::HashSet<u32> = ALL.iter().map(|i| i.id()).collect();
        assert_eq!(ids.len(), ALL.len());
        assert!(!ids.contains(&0));
        for i in ALL {
            assert_eq!(Item::from_id(i.id()), Some(i));
            assert!(matches!(command_for(TrayEv::Pick(i.id())), Some(Command::MenuPick(p)) if p == i));
        }
        assert_eq!(Item::from_id(0), None);
        assert_eq!((Item::Attention(Some(Attention::Off)).key(), Item::ShowHide.probe()), ("attention", "menu-hide"));
        assert!(command_for(TrayEv::Pick(99)).is_none(), "an id that isn't hers does nothing");
        assert!(matches!(command_for(TrayEv::Open), Some(Command::TalkTap)), "a click is her summon key");
    }
}
