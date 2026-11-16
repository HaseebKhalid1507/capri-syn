//! The READER: a long reply (or one with code) in its own panel beside her. Scrollable, in
//! Exo 2 at a readable size, with real code blocks in a monospace face, bullets and headings,
//! and copy buttons (the whole reply, each code block). Esc, a click outside or ✕ closes it.
//! A plain opaque rectangle (no shape: nothing to slab), drawn only when something changes.

use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

/// Text size and line height, logical px.
pub const FONT: f32 = 17.0;
pub const LINE: f32 = 25.0;
pub const CODE_FONT: f32 = 14.5;
pub const CODE_LINE: f32 = 20.0;
const PAD: f32 = 18.0;
const HEADER: f32 = 34.0;
/// The monospace face for code (the first one the system has).
const MONO: &str = "DejaVu Sans Mono";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    /// Indent level, marker (`•` or `1.`), text.
    Bullet(u8, String, String),
    Code { lang: String, text: String },
    Rule,
}

/// Markdown-lite: fenced code, ATX headings, bullets and numbered items, rules, paragraphs
/// (soft-wrapped lines joined). Inline marks are handled by [`inline`].
pub fn parse(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut code: Option<(String, String, Vec<String>)> = None; // (fence, lang, lines)
    let flush = |para: &mut Vec<String>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block::Para(para.join(" ")));
            para.clear();
        }
    };
    for line in md.split('\n') {
        let t = line.trim_start();
        if let Some((fence, lang, lines)) = &mut code {
            if t.starts_with(fence.as_str()) && t.trim_start_matches(fence.chars().next().unwrap_or('`')).trim().is_empty() {
                out.push(Block::Code { lang: lang.clone(), text: lines.join("\n") });
                code = None;
            } else {
                lines.push(line.trim_end().to_string());
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut para, &mut out);
            let c = t.chars().next().unwrap_or('`');
            let n = t.chars().take_while(|&x| x == c).count();
            code = Some((std::iter::repeat_n(c, n).collect(), t[n..].trim().to_string(), Vec::new()));
            continue;
        }
        if t.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            flush(&mut para, &mut out);
            out.push(Block::Heading(hashes as u8, t[hashes..].trim().trim_end_matches('#').trim().to_string()));
            continue;
        }
        if t.len() >= 3 && ["---", "***", "___"].iter().any(|r| t.chars().all(|c| c == r.chars().next().unwrap() || c == ' ') && t.starts_with(&r[..1])) {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            continue;
        }
        let indent = ((line.len() - t.len()) / 2).min(3) as u8;
        if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|b| t.strip_prefix(b)) {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(indent, "•".into(), rest.trim().to_string()));
            continue;
        }
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && digits <= 3 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(indent, format!("{}.", &t[..digits]), t[digits + 2..].trim().to_string()));
            continue;
        }
        para.push(t.trim_end().to_string());
    }
    if let Some((_, lang, lines)) = code {
        // An unclosed fence (a reply still streaming): show what came.
        out.push(Block::Code { lang, text: lines.join("\n") });
    }
    flush(&mut para, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Code,
    /// `*word*`: her stressed word (the speech markup), shown in italics, never as asterisks.
    Em,
}

/// Inline marks: `**bold**`/`__bold__`, `*em*`, `` `code` ``, `[text](url)` → `text (url)`. The
/// plain text and the marked byte ranges in it.
pub fn inline(s: &str) -> (String, Vec<(Range<usize>, Mark)>) {
    let mut out = String::with_capacity(s.len());
    let mut marks = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            let a = out.len();
            out.push_str(&r[..end]);
            marks.push((a..out.len(), Mark::Code));
            rest = &r[end + 1..];
            continue;
        }
        if let Some((open, r)) = rest.strip_prefix("**").map(|r| ("**", r)).or_else(|| rest.strip_prefix("__").map(|r| ("__", r)))
            && let Some(end) = r.find(open)
            && end > 0
        {
            let a = out.len();
            out.push_str(&r[..end]);
            marks.push((a..out.len(), Mark::Bold));
            rest = &r[end + 2..];
            continue;
        }
        if crate::voice::em_opens(out.chars().next_back(), rest)
            && let Some(end) = crate::voice::em_close(&rest[1..])
        {
            let a = out.len();
            out.push_str(&rest[1..1 + end]);
            marks.push((a..out.len(), Mark::Em));
            rest = &rest[end + 2..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('[')
            && let Some(close) = r.find("](")
            && let Some(end) = r[close + 2..].find(')')
        {
            let (text, url) = (&r[..close], &r[close + 2..close + 2 + end]);
            out.push_str(text);
            if url != text {
                out.push_str(" (");
                out.push_str(url);
                out.push(')');
            }
            rest = &r[close + 3 + end..];
            continue;
        }
        let c = rest.chars().next().expect("non-empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    (out, marks)
}

/// About how tall the blocks lay out at `width` (logical px): for sizing the panel.
pub fn estimate_height(blocks: &[Block], width: f32) -> f32 {
    let per_line = |font: f32, w: f32| ((w / (font * 0.5)).floor() as usize).max(10);
    let wrap = |s: &str, n: usize| s.split('\n').map(|l| l.chars().count().div_ceil(n).max(1)).sum::<usize>();
    let text_w = width - 2.0 * PAD;
    let mut h = HEADER + 2.0 * PAD;
    for b in blocks {
        h += match b {
            Block::Heading(_, t) => wrap(t, per_line(FONT * 1.2, text_w)) as f32 * LINE * 1.15 + 10.0,
            Block::Para(t) => wrap(t, per_line(FONT, text_w)) as f32 * LINE + 10.0,
            Block::Bullet(i, _, t) => wrap(t, per_line(FONT, text_w - 22.0 - *i as f32 * 18.0)) as f32 * LINE + 4.0,
            Block::Code { text, .. } => text.split('\n').count() as f32 * CODE_LINE + 2.0 * 10.0 + 28.0 + 10.0,
            Block::Rule => 18.0,
        };
    }
    h
}

/// The colours (the pack's tones).
#[derive(Clone, Copy)]
pub struct Pal {
    pub frame: u32,
    pub accent: u32,
    pub surface: u32,
    pub text: u32,
    pub dim: u32,
    pub name: u32,
    pub slash: u32,
}

/// Told when the reader closes: `restore` hands the keyboard back.
pub type OnClose = Box<dyn Fn(bool, &mut App)>;

pub struct Reader {
    raw: String,
    blocks: Vec<Block>,
    title: String,
    font: SharedString,
    pal: Pal,
    scroll: ScrollHandle,
    focus: FocusHandle,
    /// Which copy button said COPIED (`usize::MAX`: the whole reply).
    copied: Option<usize>,
    was_active: bool,
    closing: bool,
    on_close: Option<OnClose>,
    _active: Option<Subscription>,
    /// Laid-out buttons (window px), for test clicks via `state`.
    buttons: Probes,
    /// Its side of her and where its tail points (logical px along that edge).
    tail: Option<(crate::beside::Side, f32)>,
}

type Probes = Rc<RefCell<Vec<(SharedString, Bounds<Pixels>)>>>;

/// The reader as `state` shows it: X id, (scroll, max scroll), buttons (id, window px).
pub type ReaderState = (u32, (f32, f32), Vec<(String, [f32; 4])>);

impl Reader {
    pub fn new(raw: String, title: String, font: SharedString, pal: Pal, on_close: OnClose, window: &mut Window, cx: &mut Context<Reader>) -> Reader {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let sub = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.was_active = true;
            } else if this.was_active {
                // A click elsewhere took the keyboard: close, leave the focus where it went.
                this.close(false, window, cx);
            }
        });
        Reader {
            blocks: parse(&raw),
            raw,
            title,
            font,
            pal,
            scroll: ScrollHandle::new(),
            focus,
            copied: None,
            was_active: false,
            closing: false,
            on_close: Some(on_close),
            _active: Some(sub),
            buttons: Rc::default(),
            tail: None,
        }
    }

    /// Where it is beside her (from the avatar, each time it is placed).
    pub fn set_tail(&mut self, tail: Option<(crate::beside::Side, f32)>, cx: &mut Context<Reader>) {
        if self.tail != tail {
            self.tail = tail;
            cx.notify();
        }
    }

    /// A streaming reply grew.
    pub fn set_text(&mut self, raw: String, cx: &mut Context<Reader>) {
        if raw != self.raw {
            self.blocks = parse(&raw);
            self.raw = raw;
            cx.notify();
        }
    }

    pub fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Reader>) {
        if self.closing {
            return;
        }
        self.closing = true;
        if let Some(f) = self.on_close.take() {
            f(restore, cx);
        }
        window.remove_window();
    }

    /// Copy `text` (block `which`): to the clipboard, or to `$SA_CLIP_FILE` under test (so a
    /// test never clobbers the desktop's clipboard).
    fn copy(&mut self, which: usize, text: String, cx: &mut Context<Reader>) {
        match std::env::var_os("SA_CLIP_FILE") {
            Some(p) => {
                std::fs::write(&p, &text).ok();
            }
            None => cx.write_to_clipboard(ClipboardItem::new_string(text.clone())),
        }
        elog!("synaps-avatar: reader: copied {} bytes ({})", text.len(), if which == usize::MAX { "the reply".to_string() } else { format!("code block {which}") });
        self.copied = Some(which);
        cx.notify();
        // One timer after a click, then quiet again (no ticking while it is open).
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1_200)).await;
            this.update(cx, |r, cx| {
                if r.copied == Some(which) {
                    r.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The scroll offset (logical px, down positive) and the content's overflow, for `state`.
    pub fn scroll_info(&self) -> (f32, f32) {
        (-f32::from(self.scroll.offset().y), f32::from(self.scroll.max_offset().y))
    }

    /// Scroll by `dy` logical px (the control socket's synthetic wheel).
    pub fn scroll_by(&mut self, dy: f32, cx: &mut Context<Reader>) {
        let o = self.scroll.offset();
        let max = f32::from(self.scroll.max_offset().y);
        let y = (f32::from(-o.y) + dy).clamp(0.0, max.max(0.0));
        self.scroll.set_offset(point(o.x, px(-y)));
        cx.notify();
    }

    /// Buttons laid out last frame: (id, x, y, w, h) in window logical px.
    pub fn buttons(&self) -> Vec<(String, [f32; 4])> {
        self.buttons
            .borrow()
            .iter()
            .map(|(id, b)| (id.to_string(), [f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)]))
            .collect()
    }

    pub fn copy_block(&mut self, which: Option<usize>, cx: &mut Context<Reader>) -> String {
        let text = match which {
            None => Some(self.raw.clone()),
            Some(n) => self.blocks.iter().filter_map(|b| if let Block::Code { text, .. } = b { Some(text.clone()) } else { None }).nth(n),
        };
        match text {
            Some(t) => {
                let n = t.len();
                self.copy(which.unwrap_or(usize::MAX), t, cx);
                format!("ok copied {n} bytes")
            }
            None => "error: no such code block".into(),
        }
    }
}

fn styled(text: &str, pal: Pal) -> StyledText {
    let (plain, marks) = inline(text);
    let hl: Vec<(Range<usize>, HighlightStyle)> = marks
        .into_iter()
        .map(|(r, m)| {
            let s = match m {
                Mark::Bold => HighlightStyle { font_weight: Some(FontWeight::BOLD), color: Some(rgb(pal.text).into()), ..Default::default() },
                Mark::Code => HighlightStyle { color: Some(rgb(pal.accent).into()), background_color: Some(rgb(0x18242e).into()), ..Default::default() },
                Mark::Em => HighlightStyle { font_style: Some(FontStyle::Italic), color: Some(rgb(pal.text).into()), ..Default::default() },
            };
            (r, s)
        })
        .collect();
    StyledText::new(plain).with_highlights(hl)
}

impl Render for Reader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal;
        self.buttons.borrow_mut().clear();
        let probes = self.buttons.clone();
        let btn = |id: SharedString, label: &str, on: bool| {
            let probes = probes.clone();
            let pid = id.clone();
            div()
                .relative()
                .child(canvas(move |b, _, _| probes.borrow_mut().push((pid.clone(), b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                .id(id)
                .h(px(22.0))
                .px(px(9.0))
                .flex()
                .items_center()
                .border_1()
                .border_color(rgba((pal.frame << 8) | if on { 0xff } else { 0x99 }))
                .bg(rgb(if on { pal.frame } else { pal.surface }))
                .text_color(rgb(if on { pal.surface } else { pal.frame }))
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .cursor_pointer()
                .child(label.to_string())
        };
        let header = div()
            .h(px(HEADER))
            .px(px(PAD))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b_1()
            .border_color(rgba((pal.frame << 8) | 0x55))
            .child(div().text_size(px(12.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.name)).child(self.title.clone()))
            .child(div().text_size(px(12.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.slash)).child("/"))
            .child(div().text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(pal.dim)).child("READER"))
            .child(div().flex_1())
            .child(btn("copy-all".into(), if self.copied == Some(usize::MAX) { "COPIED" } else { "COPY ALL" }, self.copied == Some(usize::MAX)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let t = this.raw.clone();
                    this.copy(usize::MAX, t, cx);
                }),
            ))
            .child(btn("close".into(), "✕", false).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.close(true, window, cx);
                }),
            ));
        let mut body = div().id("reader-body").flex_1().overflow_y_scroll().track_scroll(&self.scroll).px(px(PAD)).py(px(PAD)).flex().flex_col().gap(px(10.0));
        let mut code_n = 0usize;
        for b in &self.blocks {
            let el: AnyElement = match b {
                Block::Heading(level, t) => div()
                    .text_size(px(FONT * if *level <= 1 { 1.3 } else if *level == 2 { 1.18 } else { 1.06 }))
                    .line_height(px(LINE * 1.15))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(pal.name))
                    .child(styled(t, pal))
                    .into_any_element(),
                Block::Para(t) => div().text_size(px(FONT)).line_height(px(LINE)).text_color(rgb(pal.text)).child(styled(t, pal)).into_any_element(),
                Block::Bullet(indent, marker, t) => div()
                    .pl(px(*indent as f32 * 18.0))
                    .flex()
                    .gap(px(8.0))
                    .text_size(px(FONT))
                    .line_height(px(LINE))
                    .text_color(rgb(pal.text))
                    .child(div().flex_none().min_w(px(14.0)).text_color(rgb(pal.frame)).font_weight(FontWeight::BOLD).child(marker.clone()))
                    // min_w(0): a flex item's min width is its content's, so without it a long
                    // item never wraps and runs off the panel's right edge.
                    .child(div().flex_1().min_w(px(0.0)).child(styled(t, pal)))
                    .into_any_element(),
                Block::Rule => div().h(px(1.0)).my(px(6.0)).bg(rgba((pal.frame << 8) | 0x44)).into_any_element(),
                Block::Code { lang, text } => {
                    let n = code_n;
                    code_n += 1;
                    let copied = self.copied == Some(n);
                    let text_c = text.clone();
                    div()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(rgba((pal.frame << 8) | 0x44))
                        .bg(rgb(0x070b10))
                        .child(
                            div()
                                .h(px(28.0))
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .bg(rgb(0x0e161d))
                                .child(div().text_size(px(11.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.dim)).child(if lang.is_empty() { "CODE".to_string() } else { lang.to_uppercase() }))
                                .child(div().flex_1())
                                .child(btn(format!("copy-{n}").into(), if copied { "COPIED" } else { "COPY" }, copied).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.copy(n, text_c.clone(), cx);
                                    }),
                                )),
                        )
                        .child(
                            div()
                                .p(px(10.0))
                                .font_family(MONO)
                                .text_size(px(CODE_FONT))
                                .line_height(px(CODE_LINE))
                                .text_color(rgb(0xd8e6ee))
                                .flex()
                                .flex_col()
                                .children(text.split('\n').map(|l| div().whitespace_nowrap().child(if l.is_empty() { " ".to_string() } else { l.replace('\t', "    ") }))),
                        )
                        .into_any_element()
                }
            };
            body = body.child(el);
        }
        div()
            .id("reader")
            .track_focus(&self.focus)
            .key_context("Reader")
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    this.close(true, window, cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family(self.font.clone())
            .bg(rgb(pal.surface))
            .border_2()
            .border_color(rgb(pal.frame))
            .relative()
            .child(div().absolute().top_0().left(px(26.0)).w(px(48.0)).h(px(2.0)).bg(rgb(pal.accent)))
            .child(header)
            .child(body)
            .children(self.tail.map(|(side, at)| crate::line::tail_overlay(side, at, pal.frame)))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob exports a `test` attribute that would shadow `#[test]`.
    use super::{Block, Mark, estimate_height, inline, parse};

    #[test]
    fn markdown_lite_blocks() {
        let md = "# Plan\n\nFirst line\ncontinues here.\n\n- one\n  - nested `x`\n2. two\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n---\nend";
        let b = parse(md);
        assert_eq!(
            b,
            vec![
                Block::Heading(1, "Plan".into()),
                Block::Para("First line continues here.".into()),
                Block::Bullet(0, "•".into(), "one".into()),
                Block::Bullet(1, "•".into(), "nested `x`".into()),
                Block::Bullet(0, "2.".into(), "two".into()),
                Block::Code { lang: "rust".into(), text: "fn main() {\n    println!(\"hi\");\n}".into() },
                Block::Rule,
                Block::Para("end".into()),
            ]
        );
        // Still streaming: the open fence shows what came.
        assert_eq!(parse("```\nlet a = 1;"), vec![Block::Code { lang: String::new(), text: "let a = 1;".into() }]);
    }

    #[test]
    fn inline_marks() {
        let (t, m) = inline("run **cargo test** then `ls -la` see [docs](https://x.y)");
        assert_eq!(t, "run cargo test then ls -la see docs (https://x.y)");
        assert_eq!(m, vec![(4..14, Mark::Bold), (20..26, Mark::Code)]);
        assert_eq!(inline("a * b ** c").0, "a * b ** c");
        let (t, m) = inline("I'm *not* signing, 2*3*4 and *two words* `*.rs` stay");
        assert_eq!(t, "I'm not signing, 2*3*4 and two words *.rs stay");
        assert_eq!(m, vec![(4..7, Mark::Em), (27..36, Mark::Em), (37..41, Mark::Code)]);
        assert_eq!(inline("ünï `cödé`").1, vec![(6..12, Mark::Code)]);
    }

    #[test]
    fn height_grows_with_content() {
        let short = estimate_height(&parse("hi"), 600.0);
        let long = estimate_height(&parse(&"word ".repeat(400)), 600.0);
        let code = estimate_height(&parse("```\na\nb\nc\nd\n```"), 600.0);
        assert!(short < 150.0 && long > short * 3.0 && code > short, "{short} {long} {code}");
    }
}
