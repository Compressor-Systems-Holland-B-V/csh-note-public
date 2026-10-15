//! What Word takes from the laid-out page: how each piece of text looks and where it sits, where
//! the body of each page is, and which pages are designed rather than written.
//!
//! Text in the HTML compile is found again here by its span: both compiles read the same source,
//! so a piece of text has the same span in both.

use std::collections::{HashMap, HashSet};

use typst::introspection::{Location, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point, Rect};
use typst::model::FigureElem;
use typst::syntax::Span;
use typst::text::{FontInstance, TextItem};
use typst::utils::hash128;
use typst::visualize::{Geometry, Paint};
use typst_layout::{Page, PagedDocument};

use crate::export::face;

/// Share of a page that filled shapes with text on them must cover for the page to count as
/// designed (a cover, a poster) and be copied as it is rather than written as flowing text.
const DESIGNED: f64 = 0.4;
/// Elements whose text Word makes one paragraph of.
const BLOCKS: [&str; 9] = [
    "par", "heading", "title", "caption", "raw", "entry", "cell", "item", "quote",
];

#[derive(Clone)]
pub struct Look {
    pub family: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub colour: [u8; 3],
    /// Extra space after each character, in points.
    pub tracking: f64,
    pub font: FontInstance,
}

/// A run of text on a page that came from one span.
pub struct Occurrence {
    pub page: usize,
    pub x: f64,
    pub right: f64,
    pub baseline: f64,
    pub ascent: f64,
    pub look: Look,
    /// The paragraph-like element it sits in, as an index into [`Paged::blocks`].
    pub block: Option<usize>,
    /// In a header, footer or background rather than the page's body.
    pub chrome: bool,
    /// Nothing but dots, as an outline's leader.
    pub dots: bool,
    /// The text, for finding text that has no span (such as a counter or a template's argument).
    pub text: String,
    order: usize,
    group: Option<usize>,
}

pub struct Block {
    pub kind: &'static str,
    pub occurrences: Vec<usize>,
}

/// A filled area or a line on a page's body, for table cells and borders.
pub struct Fill {
    pub rect: Rect,
    pub colour: [u8; 3],
}

pub struct Stroke {
    pub rect: Rect,
    pub colour: [u8; 3],
    pub thickness: f64,
}

pub struct Sheet {
    pub number: u64,
    pub width: f64,
    pub height: f64,
    /// Where the page's body sits; none when the page has no body of its own.
    pub body: Option<Rect>,
    /// Per item of the page frame, whether it is part of the body: Typst puts header, body and
    /// footer in that order, the body as one group or (when nothing comes before it, or it is
    /// small) as its own items.
    body_items: Vec<bool>,
    /// Copied as it is rather than written as flowing text.
    pub designed: bool,
    /// The body is a group, so its place is exact rather than estimated.
    exact: bool,
    pub fills: Vec<Fill>,
    pub strokes: Vec<Stroke>,
}

/// Where an image or a shape sits on a page.
pub struct Placement {
    pub page: usize,
    pub rect: Rect,
    group: Option<usize>,
    chrome: bool,
}

impl Placement {
    pub fn at(page: usize, rect: Rect) -> Self {
        Placement {
            page,
            rect,
            group: None,
            chrome: false,
        }
    }
}

pub struct Paged {
    pub occurrences: Vec<Occurrence>,
    pub blocks: Vec<Block>,
    pub sheets: Vec<Sheet>,
    by_span: HashMap<Span, Vec<usize>>,
    images: HashMap<Span, Vec<Placement>>,
    /// Where each figure's content is (its caption aside), by [`figure_key`].
    figures: HashMap<u128, Vec<Placement>>,
}

/// Identifies a figure across the paged and the HTML compile: its body, spans included.
pub fn figure_key(content: &typst::foundations::Content) -> Option<u128> {
    content
        .to_packed::<FigureElem>()
        .map(|figure| hash128(&figure.body))
}

fn solid(paint: &Paint) -> Option<[u8; 3]> {
    match paint {
        Paint::Solid(colour) => {
            let [red, green, blue, _] = colour.to_vec4_u8();
            Some([red, green, blue])
        }
        _ => None,
    }
}

pub fn look(text: &TextItem) -> Option<Look> {
    let colour = solid(&text.fill)?;
    let own = face(&text.font);
    let size = text.size.to_pt();
    let mut extra: Vec<f64> = text
        .glyphs
        .iter()
        .filter(|glyph| {
            text.text
                .get(glyph.range())
                .is_some_and(|part| !part.trim().is_empty())
        })
        .filter_map(|glyph| {
            let natural = text.font.x_advance(glyph.id)?;
            Some((glyph.x_advance - natural).at(text.size).to_pt())
        })
        .collect();
    extra.sort_by(f64::total_cmp);
    let tracking = extra.get(extra.len() / 2).copied().unwrap_or(0.0);
    Some(Look {
        family: own.family,
        size,
        bold: own.bold,
        italic: own.italic,
        colour,
        tracking: if tracking.abs() > 0.03 * size {
            tracking
        } else {
            0.0
        },
        font: text.font.clone(),
    })
}

struct Walk<'a> {
    page: usize,
    sheet: &'a mut Sheet,
    paged: &'a mut Paged,
    open: Vec<(Location, usize)>,
    locations: HashMap<Location, usize>,
    /// Filled areas (with the page item they are in) and the text boxes on them, for telling
    /// designed pages.
    painted: Vec<(Rect, Option<usize>)>,
    texts: Vec<Rect>,
    /// Figures open at this point: their end, key and the area drawn so far.
    figures: Vec<(Location, u128, Option<Rect>)>,
}

impl Walk<'_> {
    fn drawn(&mut self, rect: Rect) {
        for (_, _, area) in &mut self.figures {
            *area = Some(area.map_or(rect, |area| {
                Rect::new(area.min.min(rect.min), area.max.max(rect.max))
            }));
        }
    }

    fn text(&mut self, text: &TextItem, at: Point, group: Option<usize>) {
        let Some(look) = look(text) else {
            return;
        };
        let metrics = text.font.metrics();
        let ascent = metrics.ascender.at(text.size).to_pt();
        let descent = -metrics.descender.at(text.size).to_pt();
        let own = Rect::new(
            at - Point::with_y(Abs::pt(ascent)),
            at + Point::new(text.width(), Abs::pt(descent)),
        );
        self.texts.push(own);
        let block = self.open.last().map(|(_, block)| *block);
        if block.is_none_or(|block| self.paged.blocks[block].kind != "caption") {
            self.drawn(own);
        }
        let mut x = at.x.to_pt();
        let mut start = 0;
        while start < text.glyphs.len() {
            let span = text.glyphs[start].span.0;
            let mut end = start;
            let mut width = 0.0;
            // Up to the last glyph that is not a space: justification stretches spaces.
            let mut inked = 0.0;
            while end < text.glyphs.len() && text.glyphs[end].span.0 == span {
                let glyph = &text.glyphs[end];
                width += glyph.x_advance.at(text.size).to_pt();
                if text
                    .text
                    .get(glyph.range())
                    .is_some_and(|part| !part.trim().is_empty())
                {
                    inked = width;
                }
                end += 1;
            }
            let content = text
                .glyphs
                .get(start..end)
                .and_then(|glyphs| {
                    let from = glyphs.first()?.range().start;
                    let to = glyphs.last()?.range().end;
                    text.text.get(from.min(to)..to.max(from))
                })
                .unwrap_or_default();
            {
                let index = self.paged.occurrences.len();
                self.paged.occurrences.push(Occurrence {
                    page: self.page,
                    x,
                    right: x + inked,
                    baseline: at.y.to_pt(),
                    ascent,
                    look: look.clone(),
                    block,
                    chrome: false,
                    dots: !content.trim().is_empty() && content.trim().chars().all(|c| c == '.'),
                    text: content.to_string(),
                    order: index,
                    group,
                });
                if !span.is_detached() {
                    self.paged.by_span.entry(span).or_default().push(index);
                }
                if let Some(block) = block {
                    self.paged.blocks[block].occurrences.push(index);
                }
            }
            x += width;
            start = end;
        }
    }

    /// `group` is the item of the page frame that `frame` sits in; none for the page frame itself,
    /// whose items are told apart to find the body among them.
    fn frame(&mut self, frame: &Frame, origin: Point, group: Option<usize>, top: bool) {
        for (index, (position, item)) in frame.items().enumerate() {
            let at = origin + *position;
            let group = if top { Some(index) } else { group };
            match item {
                FrameItem::Group(inner) => self.frame(
                    &inner.frame,
                    at + Point::new(inner.transform.tx, inner.transform.ty),
                    group,
                    false,
                ),
                FrameItem::Text(text) => self.text(text, at, group),
                FrameItem::Shape(shape, span) => {
                    let own = shape.bbox(true);
                    let rect = Rect::new(own.min + at, own.max + at);
                    self.drawn(rect);
                    let body = self.sheet.holds(group);
                    if let Some(colour) = shape.fill.as_ref().and_then(solid) {
                        self.painted.push((rect, group));
                        if body && matches!(shape.geometry, Geometry::Rect(_)) {
                            self.sheet.fills.push(Fill { rect, colour });
                        }
                    }
                    if !span.is_detached() {
                        self.paged.images.entry(*span).or_default().push(Placement {
                            page: self.page,
                            rect,
                            group,
                            chrome: false,
                        });
                    }
                    if let Some(stroke) = &shape.stroke
                        && let Some(colour) = solid(&stroke.paint)
                        && body
                    {
                        self.sheet.strokes.push(Stroke {
                            rect,
                            colour,
                            thickness: stroke.thickness.to_pt(),
                        });
                    }
                }
                FrameItem::Image(_, size, span) => {
                    let rect = Rect::new(at, at + size.to_point());
                    self.drawn(rect);
                    self.painted.push((rect, group));
                    if !span.is_detached() {
                        self.paged.images.entry(*span).or_default().push(Placement {
                            page: self.page,
                            rect,
                            group,
                            chrome: false,
                        });
                    }
                }
                FrameItem::Tag(Tag::Start(content, _)) => {
                    if let (Some(key), Some(location)) = (figure_key(content), content.location()) {
                        self.figures.push((location, key, None));
                    }
                    if let Some(location) = content.location()
                        && let Some(kind) =
                            BLOCKS.iter().find(|kind| **kind == content.elem().name())
                    {
                        let block = *self.locations.entry(location).or_insert_with(|| {
                            self.paged.blocks.push(Block {
                                kind,
                                occurrences: Vec::new(),
                            });
                            self.paged.blocks.len() - 1
                        });
                        self.open.push((location, block));
                    }
                }
                FrameItem::Tag(Tag::End(location, ..)) => {
                    if let Some(index) = self.open.iter().rposition(|(open, _)| open == location) {
                        self.open.remove(index);
                    }
                    if let Some(index) = self
                        .figures
                        .iter()
                        .rposition(|(open, _, _)| open == location)
                    {
                        let (_, key, area) = self.figures.remove(index);
                        if let Some(rect) = area {
                            self.paged.figures.entry(key).or_default().push(Placement {
                                page: self.page,
                                rect,
                                group,
                                chrome: false,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Sheet {
    fn new(page: &Page) -> Self {
        Sheet {
            number: page.number,
            width: page.frame.width().to_pt(),
            height: page.frame.height().to_pt(),
            body: None,
            body_items: Vec::new(),
            designed: false,
            exact: false,
            fills: Vec::new(),
            strokes: Vec::new(),
        }
    }

    /// Whether the page frame's item `item` is part of the body.
    fn holds(&self, item: Option<usize>) -> bool {
        item.is_some_and(|item| self.body_items.get(item).copied().unwrap_or(false))
    }
}

fn bounds(item: &FrameItem, at: Point) -> Option<Rect> {
    match item {
        FrameItem::Group(group) => {
            let at = at + Point::new(group.transform.tx, group.transform.ty);
            Some(Rect::new(at, at + group.frame.size().to_point()))
        }
        FrameItem::Text(text) => Some(Rect::new(
            at - Point::with_y(text.size * 0.8),
            at + Point::new(text.width(), text.size * 0.2),
        )),
        FrameItem::Shape(shape, _) => {
            let own = shape.bbox(true);
            Some(Rect::new(own.min + at, own.max + at))
        }
        FrameItem::Image(_, size, _) => Some(Rect::new(at, at + size.to_point())),
        _ => None,
    }
}

fn area(rect: &Rect) -> f64 {
    ((rect.max.x - rect.min.x).to_pt() * (rect.max.y - rect.min.y).to_pt()).max(0.0)
}

fn contains(rect: &Rect, point: Point) -> bool {
    rect.min.x <= point.x && point.x <= rect.max.x && rect.min.y <= point.y && point.y <= rect.max.y
}

impl Paged {
    /// `spans` are those of the text the HTML compile has, which tells a page's body from its
    /// header, footer and background.
    pub fn new(document: &PagedDocument, spans: &HashSet<Span>) -> Self {
        let mut paged = Paged {
            occurrences: Vec::new(),
            blocks: Vec::new(),
            sheets: Vec::new(),
            by_span: HashMap::new(),
            images: HashMap::new(),
            figures: HashMap::new(),
        };
        // First pass: per page, how much of the HTML's text each of its items holds.
        let counts: Vec<Vec<usize>> = document
            .pages()
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let mut sheet = Sheet::new(page);
                let mut probe = Paged {
                    occurrences: Vec::new(),
                    blocks: Vec::new(),
                    sheets: Vec::new(),
                    by_span: HashMap::new(),
                    images: HashMap::new(),
                    figures: HashMap::new(),
                };
                Walk {
                    page: index,
                    sheet: &mut sheet,
                    paged: &mut probe,
                    open: Vec::new(),
                    locations: HashMap::new(),
                    painted: Vec::new(),
                    texts: Vec::new(),
                    figures: Vec::new(),
                }
                .frame(&page.frame, Point::zero(), None, true);
                let mut counts = vec![0; page.frame.items().count()];
                for (span, found) in &probe.by_span {
                    if spans.contains(span) {
                        for &occurrence in found {
                            if let Some(item) = probe.occurrences[occurrence].group {
                                counts[item] += 1;
                            }
                        }
                    }
                }
                counts
            })
            .collect();
        // A group holding the page's text is its body, margins and all.
        let groups: Vec<Option<(usize, Rect)>> = document
            .pages()
            .iter()
            .zip(&counts)
            .map(|(page, counts)| {
                let total: usize = counts.iter().sum();
                let (item, count) = counts.iter().enumerate().max_by_key(|(_, count)| **count)?;
                match page.frame.items().nth(item) {
                    // Nearly all of it: a header that repeats a heading may hold a little.
                    // Typst lays the body out as tall as the page minus its margins, which tells
                    // it from a block (a centred title) that happens to hold all the text.
                    Some((at, FrameItem::Group(group)))
                        if *count > 0
                            && count * 10 >= total * 9
                            && group.frame.height() >= page.frame.height() * 0.4 =>
                    {
                        let at = *at + Point::new(group.transform.tx, group.transform.ty);
                        Some((item, Rect::new(at, at + group.frame.size().to_point())))
                    }
                    _ => None,
                }
            })
            .collect();

        let mut locations = HashMap::new();
        for (index, page) in document.pages().iter().enumerate() {
            let size = page.frame.size();
            let first = paged.occurrences.len();
            let mut sheet = Sheet::new(page);
            let items = page.frame.items().count();
            if let Some((item, rect)) = groups[index] {
                sheet.body_items = (0..items).map(|other| other == item).collect();
                sheet.body = Some(rect);
                sheet.exact = true;
            } else if let Some(last) = counts[index].iter().rposition(|count| *count > 0) {
                // Inlined: Typst does that when nothing came before the body (or it is small), so
                // it runs to the last item with text, after any header or background (a group
                // at the top edge without the body's text).
                let entries: Vec<(Point, &FrameItem)> =
                    page.frame.items().map(|(at, item)| (*at, item)).collect();
                let lead = entries
                    .iter()
                    .zip(&counts[index])
                    .take_while(|((at, item), count)| {
                        matches!(item, FrameItem::Tag(_))
                            || (matches!(item, FrameItem::Group(_))
                                && at.y <= Abs::zero()
                                && **count == 0)
                    })
                    .count();
                // Drawings after the last text are body too, as far as they stay above where the
                // footer would start (the bottom margin taken as deep as the top one).
                let top = entries
                    .iter()
                    .enumerate()
                    .filter(|(item, _)| *item >= lead && *item <= last)
                    .filter_map(|(_, (at, item))| bounds(item, *at))
                    .map(|own| own.min.y)
                    .fold(size.y, Abs::min);
                let last = last
                    + entries[last + 1..]
                        .iter()
                        .take_while(|(at, item)| {
                            bounds(item, *at).is_none_or(|own| own.min.y < size.y - top)
                        })
                        .count();
                sheet.body_items = (0..items)
                    .map(|item| item >= lead && item <= last)
                    .collect();
                // The margins are not kept: they come from where the content reaches, even on
                // both sides where it falls short.
                sheet.body = entries
                    .iter()
                    .zip(&sheet.body_items)
                    .filter(|(_, inside)| **inside)
                    .filter_map(|((at, item), _)| bounds(item, *at))
                    .reduce(|a, b| Rect::new(a.min.min(b.min), a.max.max(b.max)))
                    .map(|reach| {
                        Rect::new(
                            reach.min,
                            Point::new(
                                reach.max.x.max(size.x - reach.min.x),
                                reach.max.y.max(size.y - reach.min.y),
                            ),
                        )
                    });
            }

            let mut walk = Walk {
                page: index,
                sheet: &mut sheet,
                paged: &mut paged,
                open: Vec::new(),
                locations: std::mem::take(&mut locations),
                painted: Vec::new(),
                texts: Vec::new(),
                figures: Vec::new(),
            };
            walk.frame(&page.frame, Point::zero(), None, true);
            locations = std::mem::take(&mut walk.locations);
            let (painted, texts) = (walk.painted, walk.texts);
            // Designed: no body of its own, or a body mostly made of filled shapes with text on them.
            let covered: f64 = painted
                .iter()
                .filter(|(_, group)| sheet.body.is_none() || sheet.holds(*group))
                .map(|(fill, _)| fill)
                .filter(|fill| {
                    texts.iter().any(|text| {
                        contains(
                            fill,
                            Point::new((text.min.x + text.max.x) / 2.0, text.max.y),
                        )
                    })
                })
                .map(area)
                .sum();
            sheet.designed = (sheet.body.is_none() && !texts.is_empty())
                || covered >= DESIGNED * area(&Rect::new(Point::zero(), size.to_point()));
            let chrome = |sheet: &Sheet, group: Option<usize>| {
                !sheet.designed && sheet.body.is_some() && !sheet.holds(group)
            };
            for placement in paged.images.values_mut().flatten() {
                if placement.page == index {
                    placement.chrome = chrome(&sheet, placement.group);
                }
            }
            for occurrence in &mut paged.occurrences[first..] {
                occurrence.chrome = chrome(&sheet, occurrence.group);
            }
            paged.sheets.push(sheet);
        }
        // An estimated body (a title page's centred lines reach only so far) takes the exact one
        // of a page of the same size, or else what all such pages reach together.
        let sizes: Vec<(f64, f64)> = paged
            .sheets
            .iter()
            .map(|sheet| (sheet.width, sheet.height))
            .collect();
        for index in 0..paged.sheets.len() {
            if paged.sheets[index].exact || paged.sheets[index].designed {
                continue;
            }
            let alike =
                |other: &(usize, &Sheet)| sizes[other.0] == sizes[index] && !other.1.designed;
            let exact = paged
                .sheets
                .iter()
                .enumerate()
                .filter(alike)
                .find(|(_, other)| other.exact);
            let union = paged
                .sheets
                .iter()
                .enumerate()
                .filter(alike)
                .filter_map(|(_, other)| other.body)
                .reduce(|a, b| Rect::new(a.min.min(b.min), a.max.max(b.max)));
            if let Some(body) = exact.and_then(|(_, other)| other.body).or(union) {
                paged.sheets[index].body = Some(body);
            }
        }
        paged
    }

    /// The occurrence of `span` that comes next after `cursor`, or else its first; never one in a
    /// header or footer. With `kinds`, one in such a block comes first: the number of a heading
    /// is also in the outline, with the same span.
    pub fn find(&self, span: Span, cursor: usize, kinds: &[&str]) -> Option<usize> {
        let found = self.by_span.get(&span)?;
        let body = || {
            found
                .iter()
                .copied()
                .filter(|&index| !self.occurrences[index].chrome)
        };
        let fits = |index: &usize| {
            kinds.is_empty()
                || self.occurrences[*index]
                    .block
                    .is_some_and(|block| kinds.contains(&self.blocks[block].kind))
        };
        let next = |index: &usize| self.occurrences[*index].order >= cursor;
        body()
            .find(|index| next(index) && fits(index))
            .or_else(|| body().find(next))
            .or_else(|| body().find(fits))
            .or_else(|| body().next())
    }

    /// The next occurrence after `cursor` that holds `text` (or the start of it): for text without
    /// a span. Looks only a little ahead, so a short text like a number is not taken from afar.
    pub fn find_text(&self, text: &str, cursor: usize, kinds: &[&str]) -> Option<usize> {
        let wanted: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if wanted.is_empty() {
            return None;
        }
        let fits = |occurrence: &Occurrence| {
            kinds.is_empty()
                || occurrence
                    .block
                    .is_some_and(|block| kinds.contains(&self.blocks[block].kind))
        };
        let matches = |occurrence: &Occurrence| {
            let own: String = occurrence
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            !own.is_empty()
                && (own == wanted
                    || (own.chars().count() > 3 && wanted.starts_with(&own))
                    || (wanted.chars().count() > 3 && own.starts_with(&wanted)))
        };
        let ahead = || {
            self.occurrences
                .iter()
                .skip(cursor)
                .filter(|occurrence| {
                    !occurrence.chrome && !occurrence.text.trim().chars().all(|c| c == '.')
                })
                .take(80)
                .filter(|occurrence| matches(occurrence))
        };
        ahead()
            .find(|occurrence| fits(occurrence))
            .or_else(|| ahead().next())
            .map(|occurrence| occurrence.order)
    }

    /// Where the figure with `key` is drawn, the `seen`-th time.
    pub fn figure(&self, key: u128, seen: usize) -> Option<&Placement> {
        let found = self.figures.get(&key)?;
        found.get(seen).or(found.last())
    }

    /// Where the image with `span` is, the next time after `cursor` if it is placed more than once.
    pub fn image(&self, span: Span, seen: usize) -> Option<&Placement> {
        let found: Vec<&Placement> = self
            .images
            .get(&span)?
            .iter()
            .filter(|placement| !placement.chrome)
            .collect();
        found.get(seen).or(found.last()).copied()
    }

    /// Like [`find`](Self::find), preferring the occurrence in a heading over one in an outline.
    pub fn heading(&self, span: Span) -> Option<usize> {
        let found = self.by_span.get(&span)?;
        found
            .iter()
            .copied()
            .find(|&index| {
                self.occurrences[index]
                    .block
                    .is_some_and(|block| matches!(self.blocks[block].kind, "heading" | "title"))
            })
            .or_else(|| found.last().copied())
    }

    /// The later lines of the text that `occurrence` starts: the same span in the same block.
    pub fn continuation(&self, occurrence: usize, span: Span) -> Vec<usize> {
        let block = self.occurrences[occurrence].block;
        self.by_span.get(&span).map_or_else(Vec::new, |found| {
            found
                .iter()
                .copied()
                .filter(|&index| {
                    index > occurrence && block.is_some() && self.occurrences[index].block == block
                })
                .collect()
        })
    }

    pub fn order(&self, occurrence: usize) -> usize {
        self.occurrences[occurrence].order
    }

    /// The page without its body: header, footer and background.
    pub fn chrome(&self, page: &Page, index: usize) -> Page {
        let mut chrome = page.clone();
        let mut frame = Frame::hard(page.frame.size());
        for (item_index, (at, item)) in page.frame.items().enumerate() {
            if !self.sheets[index].holds(Some(item_index)) || self.sheets[index].body.is_none() {
                frame.push(*at, item.clone());
            }
        }
        chrome.frame = frame;
        chrome
    }
}

/// The width text runs in on most pages: Typst lays the header, the body and the footer out
/// that wide. Used before the body is known, to size images in the HTML compile.
pub fn text_width(document: &PagedDocument) -> Abs {
    let mut widths: HashMap<i64, usize> = HashMap::new();
    let mut page_width = Abs::pt(595.0);
    for page in document.pages() {
        page_width = page.frame.width();
        for (_, item) in page.frame.items() {
            if let FrameItem::Group(group) = item {
                let width = group.frame.width();
                if width < page.frame.width() - Abs::pt(1.0) && width > page.frame.width() / 3.0 {
                    *widths
                        .entry((width.to_pt() * 10.0).round() as i64)
                        .or_default() += 1;
                }
            }
        }
    }
    widths
        .into_iter()
        .max_by_key(|(width, count)| (*count, *width))
        .map_or(page_width * 0.75, |(width, _)| Abs::pt(width as f64 / 10.0))
}

pub fn surface(rect: &Rect) -> f64 {
    area(rect)
}
