//! Word: text that flows and looks like the page. The structure (headings, lists, tables, links,
//! equations) comes from the page compiled for Typst's HTML target; how each piece of text looks
//! and where it sits comes from the laid-out page, matched by span. Pages that are designed rather
//! than written (a cover) are copied as they are, and every other page's header, footer and
//! background become the Word header of its section.

mod drawing;
mod fonts;
mod paged;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use typst::introspection::{Location, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point, Rect};
use typst::syntax::Span;
use typst::text::FontInstance;
use typst::utils::hash128;
use typst::visualize::Image;
use typst_html::{HtmlDocument, HtmlElement, HtmlFrame, HtmlNode};
use typst_layout::PagedDocument;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use self::drawing::{Field, twips};
use self::paged::{Look, Paged};
use super::layout::{Layers, layers, lines, paragraphs};
use super::math::{self, Equations, attribute, name};
use super::{Media, OFFICE, XML, escape, media, png, relationships};

pub use self::paged::text_width;

/// The class of the `hr` that a page break becomes in the HTML compile (see `prelude`).
pub const PAGE_BREAK: &str = "znote-page-break";

const MAIN: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PLACEMENT: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const COMPATIBILITY: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const WORD: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";
/// Where Word puts the baseline in a line of exact height, as a share of that height from the top.
const BASELINE: f64 = 0.8;
/// An empty paragraph one point high, to hold drawings or a section's end.
const TINY: &str =
    r#"<w:pPr><w:spacing w:before="0" w:after="0" w:line="20" w:lineRule="exact"/></w:pPr>"#;

fn namespaces() -> String {
    format!(
        r#"xmlns:w="{MAIN}" xmlns:r="{OFFICE}" xmlns:m="{}" xmlns:wp="{PLACEMENT}" xmlns:a="{DRAWING}" xmlns:pic="{}" xmlns:wps="{}" xmlns:mc="{COMPATIBILITY}""#,
        math::NAMESPACE,
        drawing::PICTURE,
        drawing::SHAPE
    )
}

/// What the parts of the file share: pictures, drawing ids and the fonts in use.
pub struct Parts {
    media: Vec<(String, Vec<u8>)>,
    ids: usize,
    faces: fonts::Faces,
}

fn link(links: &mut Vec<(&'static str, String)>, kind: &'static str, target: String) -> String {
    links.push((kind, target));
    format!("rId{}", links.len())
}

impl Parts {
    fn id(&mut self) -> usize {
        self.ids += 1;
        self.ids
    }

    fn picture(&mut self, links: &mut Vec<(&'static str, String)>, media: Media) -> String {
        let file = format!("image{}.{}", self.media.len() + 1, media.extension);
        self.media.push((file.clone(), media.bytes));
        link(links, "image", format!("media/{file}"))
    }

    fn font(&mut self, family: &str, bold: bool, italic: bool, font: &FontInstance) {
        self.faces.entry(family.to_string()).or_default()[fonts::slot(bold, italic)]
            .get_or_insert_with(|| font.clone());
    }
}

#[derive(Clone, Default)]
struct Format {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    small_caps: bool,
    highlight: bool,
    code: bool,
    link: bool,
    script: Option<&'static str>,
    colour: Option<String>,
}

/// Where a paragraph sits, for its style.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    /// 0 is the title.
    Heading(usize),
    Caption,
    Quote,
    Code,
    Figure,
}

struct List {
    /// The numbering instance, or none for a list without markers (such as an outline).
    number: Option<usize>,
    /// Where the text of its items starts.
    text: Option<f64>,
}

/// One line of a paragraph on the laid-out page.
#[derive(Clone, Copy)]
struct Line {
    page: usize,
    baseline: f64,
    left: f64,
    right: f64,
    size: f64,
}

/// Where a paragraph's lines are on the laid-out page.
struct Geometry {
    lines: Vec<Line>,
    pitch: Option<f64>,
}

impl Geometry {
    fn first(&self) -> Line {
        self.lines[0]
    }

    fn last(&self) -> Line {
        self.lines[self.lines.len() - 1]
    }
}

/// A paragraph or table of the body, with the page it is on.
struct Out {
    xml: String,
    page: Option<usize>,
    page_break: bool,
}

/// A table cell as written, before the table knows its columns.
struct Cell {
    row: usize,
    column: usize,
    wide: usize,
    tall: usize,
    content: String,
    found: Vec<usize>,
}

struct Writer<'a> {
    paged: &'a Paged,
    parts: &'a mut Parts,
    links: Vec<(&'static str, String)>,
    out: Vec<Out>,
    runs: String,
    /// Occurrences of the paragraph being written; and of the table cell, if in one.
    found: Vec<usize>,
    cell: Option<Vec<usize>>,
    cursor: usize,
    /// The last run's occurrence and look, and whether its text ended in a space.
    previous_run: Option<usize>,
    look: Option<Look>,
    spaced: bool,
    /// Something in the paragraph that a fixed line height would cut off.
    tall: bool,
    /// The last paragraph's last line and line height.
    previous: Option<(Line, f64)>,
    leading: f64,
    roles: Vec<Role>,
    headings: Vec<(Location, usize)>,
    heading_looks: [Option<Look>; 7],
    lists: Vec<List>,
    item: bool,
    bookmarks: Vec<String>,
    marks: usize,
    numbers: Vec<usize>,
    /// Outline entries: the heading the entry links to, and its page number.
    outline: bool,
    target: Option<String>,
    targets: HashMap<String, u64>,
    leader: bool,
    room: f64,
    /// The span of the image whose frame comes next, and how often each image was placed.
    placed: HashMap<Span, usize>,
    justified: bool,
    /// An occurrence only partly written yet, and what of its text is left.
    partial: Option<(usize, String)>,
    /// Footnote texts by the id Typst links them with; and the footnotes written so far, with
    /// their own relationships.
    notes: HashMap<String, HtmlElement>,
    footnotes: Vec<String>,
    note_links: Vec<(&'static str, String)>,
    in_note: bool,
    /// The figure whose content comes next, and how often each figure was taken from its page.
    figure: Option<u128>,
    figures_seen: HashMap<u128, usize>,
    document: &'a PagedDocument,
    /// The space above each kind of paragraph (style and size), learned where Typst set one
    /// below another on the same page.
    spacing: HashMap<String, f64>,
    /// The paragraph being written holds a picture.
    drawing: bool,
    /// Centre the next paragraph whatever its lines say (a display formula, whose lines are only
    /// partly known).
    centre: bool,
}

fn bookmark(id: &str) -> String {
    let name: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(39)
        .collect();
    format!("_{name}")
}

fn css(element: &HtmlElement) -> String {
    let mut css = element.css.to_inline().to_string();
    if let Some(style) = attribute(element, "style") {
        css.push(';');
        css.push_str(style);
    }
    css.replace(' ', "")
}

fn cells(row: &HtmlElement) -> Vec<&HtmlElement> {
    row.children
        .iter()
        .filter_map(|node| match node {
            HtmlNode::Element(cell) if matches!(&*name(cell), "td" | "th") => Some(cell),
            _ => None,
        })
        .collect()
}

/// The span of the first image or shape in a frame, to find where Typst put it.
fn drawn(frame: &Frame) -> Option<Span> {
    frame.items().find_map(|(_, item)| match item {
        FrameItem::Image(_, _, span) | FrameItem::Shape(_, span) if !span.is_detached() => {
            Some(*span)
        }
        FrameItem::Group(group) => drawn(&group.frame),
        _ => None,
    })
}

/// Whether HTML nodes hold anything to show: text, a picture, a table, a formula.
fn holds(nodes: &[HtmlNode]) -> bool {
    nodes.iter().any(|node| match node {
        HtmlNode::Text(text, _) => !text.trim().is_empty(),
        HtmlNode::Frame(_) => true,
        HtmlNode::Element(element) => {
            matches!(&*name(element), "table" | "img" | "math" | "hr") || holds(&element.children)
        }
        HtmlNode::Tag(_) => false,
    })
}

fn single_image(frame: &Frame) -> Option<(&Image, typst::layout::Size)> {
    let mut found = None;
    for (_, item) in frame.items() {
        match item {
            FrameItem::Image(image, size, _) if found.is_none() => found = Some((image, *size)),
            FrameItem::Group(group) if found.is_none() => found = Some(single_image(&group.frame)?),
            FrameItem::Tag(_) => {}
            _ => return None,
        }
    }
    found
}

/// The spans of all text in the HTML compile.
fn spans(nodes: &[HtmlNode], found: &mut HashSet<Span>) {
    for node in nodes {
        match node {
            HtmlNode::Text(_, span) if !span.is_detached() => {
                found.insert(*span);
            }
            HtmlNode::Element(element) => spans(&element.children, found),
            _ => {}
        }
    }
}

fn first_span(nodes: &[HtmlNode]) -> Option<Span> {
    nodes.iter().find_map(|node| match node {
        HtmlNode::Text(text, span) if !span.is_detached() && !text.trim().is_empty() => Some(*span),
        HtmlNode::Element(element) => first_span(&element.children),
        _ => None,
    })
}

/// The footnote texts Typst puts at the end of the HTML, by id.
fn notes(nodes: &[HtmlNode]) -> HashMap<String, HtmlElement> {
    let mut found = HashMap::new();
    for node in nodes {
        let HtmlNode::Element(element) = node else {
            continue;
        };
        if attribute(element, "role") == Some("doc-endnotes") {
            let mut items = Vec::new();
            collect(&element.children, "li", &mut items);
            for item in items {
                if let Some(id) = attribute(item, "id") {
                    found.insert(id.to_string(), item.clone());
                }
            }
        } else {
            found.extend(notes(&element.children));
        }
    }
    found
}

fn collect<'a>(nodes: &'a [HtmlNode], tag: &str, found: &mut Vec<&'a HtmlElement>) {
    for node in nodes {
        if let HtmlNode::Element(element) = node {
            if &*name(element) == tag {
                found.push(element);
            } else {
                collect(&element.children, tag, found);
            }
        }
    }
}

/// The page number of everything with an `id`, for an outline's page numbers. An element without
/// text of its own (a page break that took a heading's id) gets the page of the text after it.
fn targets(
    nodes: &[HtmlNode],
    paged: &Paged,
    waiting: &mut Vec<String>,
    found: &mut HashMap<String, u64>,
) {
    for node in nodes {
        let HtmlNode::Element(element) = node else {
            continue;
        };
        if let Some(id) = attribute(element, "id") {
            waiting.push(id.to_string());
        }
        if !waiting.is_empty()
            && let Some(span) = first_span(&element.children)
            && let Some(occurrence) = paged.heading(span)
        {
            let number = paged.sheets[paged.occurrences[occurrence].page].number;
            for id in waiting.drain(..) {
                found.insert(id, number);
            }
        }
        targets(&element.children, paged, waiting, found);
    }
}

fn colour([red, green, blue]: [u8; 3]) -> String {
    format!("{red:02X}{green:02X}{blue:02X}")
}

impl<'a> Writer<'a> {
    fn region(&self, page: usize) -> Rect {
        let sheet = &self.paged.sheets[page];
        sheet.body.unwrap_or(Rect::new(
            Point::zero(),
            Point::new(Abs::pt(sheet.width), Abs::pt(sheet.height)),
        ))
    }

    /// The smallest filled area on `page` around `point`: a table cell's background.
    fn fill_at(&self, page: usize, point: Point) -> Option<&paged::Fill> {
        self.paged.sheets[page]
            .fills
            .iter()
            .filter(|fill| {
                fill.rect.min.x <= point.x
                    && point.x <= fill.rect.max.x
                    && fill.rect.min.y <= point.y
                    && point.y <= fill.rect.max.y
            })
            .min_by(|a, b| paged::surface(&a.rect).total_cmp(&paged::surface(&b.rect)))
    }

    fn run_properties(&mut self, look: Option<&Look>, format: &Format, extra: f64) -> String {
        let mut xml = String::new();
        if look.is_none() && format.link {
            xml.push_str(r#"<w:rStyle w:val="Hyperlink"/>"#);
        } else if look.is_none() && format.code {
            xml.push_str(r#"<w:rStyle w:val="VerbatimChar"/>"#);
        }
        let (bold, italic) = look.map_or((format.bold, format.italic), |look| {
            (look.bold, look.italic)
        });
        if let Some(look) = look {
            self.parts
                .font(&look.family, look.bold, look.italic, &look.font);
            let family = escape(&look.family);
            let _ = write!(
                xml,
                r#"<w:rFonts w:ascii="{family}" w:hAnsi="{family}" w:eastAsia="{family}" w:cs="{family}"/>"#
            );
        }
        if bold {
            xml.push_str("<w:b/>");
        }
        if italic {
            xml.push_str("<w:i/>");
        }
        if format.small_caps {
            xml.push_str("<w:smallCaps/>");
        }
        if format.strike {
            xml.push_str("<w:strike/>");
        }
        match (look, &format.colour) {
            (Some(look), _) => {
                let _ = write!(xml, r#"<w:color w:val="{}"/>"#, colour(look.colour));
            }
            (None, Some(own)) => {
                let _ = write!(xml, r#"<w:color w:val="{own}"/>"#);
            }
            _ => {}
        }
        let spacing = look.map_or(0.0, |look| look.tracking) + extra;
        if spacing.abs() >= 0.05 {
            let _ = write!(xml, r#"<w:spacing w:val="{}"/>"#, twips(spacing));
        }
        if let Some(look) = look.filter(|_| format.script.is_none()) {
            let size = (look.size * 2.0).round() as i64;
            let _ = write!(xml, r#"<w:sz w:val="{size}"/><w:szCs w:val="{size}"/>"#);
        }
        if format.highlight {
            xml.push_str(r#"<w:highlight w:val="yellow"/>"#);
        }
        if format.underline {
            xml.push_str(r#"<w:u w:val="single"/>"#);
        }
        if let Some(script) = format.script {
            let _ = write!(xml, r#"<w:vertAlign w:val="{script}"/>"#);
        }
        if xml.is_empty() {
            xml
        } else {
            format!("<w:rPr>{xml}</w:rPr>")
        }
    }

    fn geometry(&self) -> Option<Geometry> {
        let paged = self.paged;
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for &occurrence in &self.found {
            if let Some(block) = paged.occurrences[occurrence].block {
                *counts.entry(block).or_default() += 1;
            }
        }
        let block = counts
            .into_iter()
            .max_by_key(|(block, count)| (*count, usize::MAX - block))
            .map(|(block, _)| block);
        let occurrences = match block {
            Some(block) => &paged.blocks[block].occurrences,
            None => &self.found,
        };
        let mut lines: Vec<Line> = Vec::new();
        let mut sorted: Vec<&paged::Occurrence> = occurrences
            .iter()
            .map(|&index| &paged.occurrences[index])
            .filter(|occurrence| !occurrence.chrome)
            .collect();
        sorted.sort_by(|a, b| {
            a.page
                .cmp(&b.page)
                .then(a.baseline.total_cmp(&b.baseline))
                .then(a.x.total_cmp(&b.x))
        });
        for occurrence in sorted {
            // Raised or lowered text (a footnote marker) sits on the line it is in.
            match lines.last_mut() {
                Some(line)
                    if line.page == occurrence.page
                        && (line.baseline - occurrence.baseline).abs()
                            < 0.5 * line.size.max(occurrence.look.size) =>
                {
                    line.left = line.left.min(occurrence.x);
                    line.right = line.right.max(occurrence.right);
                    if occurrence.look.size > line.size {
                        line.baseline = occurrence.baseline;
                        line.size = occurrence.look.size;
                    }
                }
                _ => lines.push(Line {
                    page: occurrence.page,
                    baseline: occurrence.baseline,
                    left: occurrence.x,
                    right: occurrence.right,
                    size: occurrence.look.size,
                }),
            }
        }
        // Only the lines from this paragraph's first text to its last: a block can hold more (a
        // list's item tag covers the whole list).
        let place = |index: &usize| {
            let occurrence = &paged.occurrences[*index];
            (occurrence.page, (occurrence.baseline * 2.0).round() as i64)
        };
        if let (Some(from), Some(to)) = (
            self.found.iter().map(place).min(),
            self.found.iter().map(place).max(),
        ) {
            lines.retain(|line| {
                let at = (line.page, (line.baseline * 2.0).round() as i64);
                from <= at && at <= to
            });
        }
        if lines.is_empty() {
            return None;
        }
        let mut pitches: Vec<f64> = lines
            .windows(2)
            .filter(|pair| pair[0].page == pair[1].page)
            .map(|pair| pair[1].baseline - pair[0].baseline)
            .filter(|pitch| *pitch > 0.0)
            .collect();
        pitches.sort_by(f64::total_cmp);
        Some(Geometry {
            pitch: pitches.get(pitches.len() / 2).copied(),
            lines,
        })
    }

    /// Alignment and indents from where the lines sit in `region`.
    fn arrangement(geometry: &Geometry, region: Rect) -> (&'static str, f64, f64, f64) {
        let (left, right) = (region.min.x.to_pt(), region.max.x.to_pt());
        let centre = (left + right) / 2.0;
        let lines: Vec<&Line> = geometry
            .lines
            .iter()
            .filter(|line| line.page == geometry.first().page)
            .collect();
        let widest = lines.iter().map(|line| line.right).fold(left, f64::max);
        let centred = |line: &&Line| ((line.left + line.right) / 2.0 - centre).abs() < 1.5;
        let starts = lines
            .iter()
            .map(|line| line.left)
            .fold(f64::INFINITY, f64::min);
        if lines.len() > 1 {
            let rest = lines[1..]
                .iter()
                .map(|line| line.left)
                .fold(f64::INFINITY, f64::min);
            let first_line = lines[0].left - rest;
            let indent = (rest - left).max(0.0);
            // Justified: every line but the last ends at the same place, give or take punctuation
            // that Typst lets hang into the margin.
            let mut ends: Vec<f64> = lines[..lines.len() - 1]
                .iter()
                .map(|line| line.right)
                .collect();
            ends.sort_by(f64::total_cmp);
            let end = ends[ends.len() / 2];
            let full = ends.iter().all(|right| (right - end).abs() < 3.0);
            if full && lines[lines.len() - 1].right < end - 3.0 {
                return ("both", indent, first_line, (right - end).max(0.0));
            }
            let lefts_differ = lines.iter().any(|line| (line.left - starts).abs() > 1.5);
            if lefts_differ && lines.iter().all(centred) {
                return ("center", 0.0, 0.0, 0.0);
            }
            if lefts_differ && lines.iter().all(|line| widest - line.right < 1.5) {
                return ("right", 0.0, 0.0, (right - widest).max(0.0));
            }
            return ("left", indent, first_line, 0.0);
        }
        let line = lines[0];
        let (before, after) = (line.left - left, right - line.right);
        // A line across most of the width (an outline entry with its leader) is not aligned
        // anywhere but left.
        if line.right - line.left > 0.6 * (right - left) {
            return ("left", before.max(0.0), 0.0, 0.0);
        }
        if before > 3.0 && centred(&line) {
            ("center", 0.0, 0.0, 0.0)
        } else if before > 3.0 && after < 1.5 {
            ("right", 0.0, 0.0, 0.0)
        } else if before > 0.3 * (right - left) && after < before / 3.0 {
            // Set against the right, inside a block that keeps it off the edge (a quote's
            // attribution).
            ("right", 0.0, 0.0, after.max(0.0))
        } else {
            ("left", before.max(0.0), 0.0, 0.0)
        }
    }

    fn style(&self) -> Option<String> {
        if self.in_note {
            return Some("FootnoteText".into());
        }
        let heading = self.headings.last().map(|(_, level)| *level).or_else(|| {
            self.roles.iter().rev().find_map(|role| match role {
                Role::Heading(level) => Some(*level),
                _ => None,
            })
        });
        match (heading, self.roles.last()) {
            (Some(0), _) => Some("Title".into()),
            (Some(level), _) => Some(format!("Heading{level}")),
            (_, Some(Role::Caption)) => Some("Caption".into()),
            (_, Some(Role::Quote)) => Some("Quote".into()),
            (_, Some(Role::Code)) => Some("SourceCode".into()),
            _ if !self.lists.is_empty() => Some("ListParagraph".into()),
            _ => None,
        }
    }

    fn heading(&self) -> Option<usize> {
        self.headings.last().map(|(_, level)| *level).or_else(|| {
            self.roles.iter().rev().find_map(|role| match role {
                Role::Heading(level) => Some(*level),
                _ => None,
            })
        })
    }

    /// The paragraph properties, from the paragraph's place on the laid-out page.
    fn properties(&mut self, geometry: Option<&Geometry>) -> String {
        let mut xml = String::new();
        if let Some(style) = self.style() {
            let _ = write!(xml, r#"<w:pStyle w:val="{style}"/>"#);
        }
        let in_cell = self.cell.is_some();
        let region = geometry.map(|geometry| {
            let first = geometry.first();
            if in_cell {
                self.fill_at(
                    first.page,
                    Point::new(Abs::pt(first.left + 0.5), Abs::pt(first.baseline - 1.0)),
                )
                .map_or_else(|| self.region(first.page), |fill| fill.rect)
            } else {
                self.region(first.page)
            }
        });
        let numbered =
            self.heading().is_none() && self.lists.last().is_some_and(|list| list.number.is_some());
        let mut indent = String::new();
        if self.heading().is_none()
            && let Some(list) = self.lists.last()
            && list.number.is_some()
        {
            let level = (self.lists.len() - 1).min(8);
            // Where the item's own text starts, not its marker.
            let text = geometry.and_then(|geometry| {
                let first = geometry.first();
                self.found
                    .iter()
                    .map(|&index| &self.paged.occurrences[index])
                    .filter(|found| {
                        found.page == first.page && (found.baseline - first.baseline).abs() < 0.5
                    })
                    .map(|found| found.x)
                    .reduce(f64::min)
            });
            let marker = self
                .lists
                .iter()
                .rev()
                .nth(1)
                .and_then(|parent| parent.text)
                .or(region.map(|region| region.min.x.to_pt()));
            let left = text
                .zip(region)
                .map(|(text, region)| text - region.min.x.to_pt());
            let hanging = text
                .zip(marker)
                .map(|(text, marker)| (text - marker).max(9.0));
            if self.item {
                let _ = write!(
                    xml,
                    r#"<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{}"/></w:numPr>"#,
                    list.number.unwrap_or(1)
                );
            }
            match (left, hanging) {
                (Some(left), Some(hanging)) if self.item => {
                    indent = format!(
                        r#"<w:ind w:left="{}" w:hanging="{}"/>"#,
                        twips(left),
                        twips(hanging)
                    );
                }
                (Some(left), _) => indent = format!(r#"<w:ind w:left="{}"/>"#, twips(left)),
                _ => indent = format!(r#"<w:ind w:left="{}"/>"#, 720 * (level + 1)),
            }
            if let (Some(list), Some(text)) = (self.lists.last_mut(), text)
                && list.text.is_none()
            {
                list.text = Some(text);
            }
        }
        if self.outline && self.target.is_some() {
            let leader = if self.leader {
                r#" w:leader="dot""#
            } else {
                ""
            };
            let stop = region.map_or(self.room, |region| (region.max.x - region.min.x).to_pt());
            let _ = write!(
                xml,
                r#"<w:tabs><w:tab w:val="right"{leader} w:pos="{}"/></w:tabs>"#,
                twips(stop)
            );
        }
        let Some(geometry) = geometry else {
            let _ = write!(
                xml,
                r#"<w:spacing w:before="{}" w:after="0"/>{indent}"#,
                twips(6.0)
            );
            if self.roles.contains(&Role::Figure) {
                xml.push_str(r#"<w:jc w:val="center"/>"#);
            }
            return format!("<w:pPr>{xml}</w:pPr>");
        };
        let region = region.unwrap_or(Rect::new(Point::zero(), Point::zero()));
        let first = geometry.first();
        let mut line = geometry.pitch.unwrap_or(first.size * self.leading);
        // The space above this kind of paragraph where Typst had one above it on the same page.
        let kind = format!("{:?}{}", self.style(), (first.size * 2.0).round());
        // Not seen yet: about a line of its size above a heading, half that above other text.
        let learned = self.spacing.get(&kind).copied().or_else(|| {
            let share = if self.heading().is_some() { 1.0 } else { 0.5 };
            Some(share * first.size)
        });
        // Room from the line above (or the top of the body) down to this baseline.
        let room = match self.previous {
            Some((previous, height)) if previous.page == first.page => {
                Some(first.baseline - previous.baseline - (1.0 - BASELINE) * height)
            }
            _ if in_cell || self.in_note => None,
            // Typst moved it to a new page with room left on the last one (to keep a heading with
            // what follows): Word may not, so it gets the space it has elsewhere. On a full page
            // Word breaks too, and the space is the top of the page.
            Some((previous, _))
                if self.region(previous.page).max.y.to_pt() - previous.baseline > line =>
            {
                learned.map(|before| before + BASELINE * line)
            }
            _ => Some(first.baseline - region.min.y.to_pt()),
        };
        // Less room than the line needs (Typst has no space above a block at the top of a page):
        // a single line gets shorter instead, so its baseline still lands in place.
        if let Some(room) = room
            && room < BASELINE * line
            && geometry.pitch.is_none()
        {
            line = (room / BASELINE).max(line * 0.6);
        }
        let before = room.map_or(0.0, |room| room - BASELINE * line);
        if self
            .previous
            .is_some_and(|(previous, _)| previous.page == first.page)
            && !in_cell
            && !self.in_note
        {
            self.spacing.entry(kind).or_insert(before.max(0.0));
        }
        // A picture sits on the baseline of a line as tall as itself.
        let (rule, line) = if self.drawing {
            ("auto", line)
        } else if self.tall {
            ("atLeast", line)
        } else {
            ("exact", line)
        };
        let _ = write!(
            xml,
            r#"<w:spacing w:before="{}" w:after="0" w:line="{}" w:lineRule="{rule}"/>"#,
            twips(before.clamp(0.0, 400.0)),
            if rule == "auto" { 240 } else { twips(line) },
        );
        let (align, left, first_line, right) = if self.centre {
            ("center", 0.0, 0.0, 0.0)
        } else {
            Self::arrangement(geometry, region)
        };
        if !numbered && !in_cell {
            let mut attributes = String::new();
            if left > 1.0 {
                let _ = write!(attributes, r#" w:left="{}""#, twips(left));
            }
            if right > 1.0 {
                let _ = write!(attributes, r#" w:right="{}""#, twips(right));
            }
            if first_line > 1.0 {
                let _ = write!(attributes, r#" w:firstLine="{}""#, twips(first_line));
            } else if first_line < -1.0 {
                let _ = write!(attributes, r#" w:hanging="{}""#, twips(-first_line));
            }
            if !attributes.is_empty() {
                indent = format!("<w:ind{attributes}/>");
            }
        }
        xml.push_str(&indent);
        if align != "left" {
            let _ = write!(xml, r#"<w:jc w:val="{align}"/>"#);
        }
        self.justified |= align == "both";
        self.previous = Some((geometry.last(), line));
        format!("<w:pPr>{xml}</w:pPr>")
    }

    fn marks(&mut self) -> String {
        let mut xml = String::new();
        for name in std::mem::take(&mut self.bookmarks) {
            self.marks += 1;
            let _ = write!(
                xml,
                r#"<w:bookmarkStart w:id="{0}" w:name="{name}"/><w:bookmarkEnd w:id="{0}"/>"#,
                self.marks
            );
        }
        xml
    }

    fn push(&mut self, xml: String, page: Option<usize>) {
        self.out.push(Out {
            xml,
            page,
            page_break: false,
        });
    }

    fn flush(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        let geometry = self.geometry();
        if let Some(level) = self.heading()
            && self.heading_looks[level].is_none()
        {
            self.heading_looks[level] = self.look.clone();
        }
        let properties = self.properties(geometry.as_ref());
        let marks = self.marks();
        let mut runs = std::mem::take(&mut self.runs);
        if self.outline
            && let Some(target) = self.target.take()
        {
            let number = self
                .targets
                .get(&target)
                .map(u64::to_string)
                .unwrap_or_default();
            let properties =
                self.run_properties(self.look.clone().as_ref(), &Format::default(), 0.0);
            let _ = write!(
                runs,
                r#"<w:r>{properties}<w:tab/></w:r><w:fldSimple w:instr=" PAGEREF {} \h "><w:r>{properties}<w:t>{number}</w:t></w:r></w:fldSimple>"#,
                bookmark(&target)
            );
        }
        let page = geometry.map(|geometry| geometry.first().page);
        self.push(format!("<w:p>{properties}{marks}{runs}</w:p>"), page);
        self.found.clear();
        self.partial = None;
        self.previous_run = None;
        self.spaced = false;
        self.tall = false;
        self.item = false;
    }

    fn block(&mut self, role: Option<Role>, element: &HtmlElement, format: &Format) {
        self.flush();
        self.roles.extend(role);
        self.nodes(&element.children, format);
        self.flush();
        if role.is_some() {
            self.roles.pop();
        }
    }

    fn nodes(&mut self, nodes: &[HtmlNode], format: &Format) {
        for node in nodes {
            match node {
                HtmlNode::Tag(Tag::Start(content, _)) => {
                    let level = if content.is::<typst::model::HeadingElem>() {
                        match content.get_by_name("level") {
                            Ok(typst::foundations::Value::Int(level)) => {
                                Some(level.clamp(1, 6) as usize)
                            }
                            _ => Some(1),
                        }
                    } else if content.is::<typst::model::TitleElem>() {
                        Some(0)
                    } else {
                        None
                    };
                    if let Some(key) = paged::figure_key(content) {
                        self.figure = Some(key);
                    }
                    if let (Some(level), Some(location)) = (level, content.location()) {
                        self.flush();
                        self.headings.push((location, level));
                    }
                }
                HtmlNode::Tag(Tag::End(location, ..)) => {
                    if self
                        .headings
                        .last()
                        .is_some_and(|(open, _)| open == location)
                    {
                        self.flush();
                        self.headings.pop();
                    }
                }
                HtmlNode::Text(text, span) => self.text(text, *span, format),
                HtmlNode::Element(element) => self.element(element, format),
                HtmlNode::Frame(frame) => self.frame(frame),
            }
        }
    }

    /// The kinds of block the text being written is in.
    fn kinds(&self) -> &'static [&'static str] {
        if self.outline {
            &["entry"]
        } else if self.heading().is_some() {
            &["heading", "title"]
        } else if self.cell.is_some() {
            &["cell"]
        } else if self.roles.last() == Some(&Role::Caption) {
            &["caption"]
        } else {
            &[]
        }
    }

    /// The occurrence of a piece of text on the laid-out page: by its span, or else by its text.
    fn find(&mut self, span: Span, text: &str) -> Option<usize> {
        let kinds = self.kinds();
        let wanted: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        // The rest of a run Typst set in one piece and HTML in several ("Figure", " ", "1", ":").
        let rest = self.partial.as_ref().and_then(|(index, rest)| {
            (!wanted.is_empty() && rest.trim_start().starts_with(&wanted)).then_some(*index)
        });
        let found = rest
            .or_else(|| {
                (!span.is_detached())
                    .then(|| self.paged.find(span, self.cursor, kinds))
                    .flatten()
            })
            .or_else(|| self.paged.find_text(text, self.cursor, kinds));
        self.partial = found.and_then(|index| {
            let own: String = self.paged.occurrences[index]
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let rest = match &self.partial {
                Some((at, rest)) if *at == index => rest.trim_start().to_string(),
                _ => own,
            };
            rest.strip_prefix(&wanted)
                .filter(|rest| !rest.trim().is_empty())
                .map(|rest| (index, rest.to_string()))
        });
        let found = found?;
        self.cursor = self.cursor.max(self.paged.order(found));
        let mut all = vec![found];
        if !span.is_detached() {
            all.extend(self.paged.continuation(found, span));
        }
        self.found.extend(&all);
        if let Some(cell) = &mut self.cell {
            cell.extend(&all);
        }
        Some(found)
    }

    fn text(&mut self, text: &str, span: Span, format: &Format) {
        if self.runs.is_empty() && text.trim().is_empty() && !self.roles.contains(&Role::Code) {
            return;
        }
        let found = if text.trim().is_empty() {
            None
        } else {
            self.find(span, text)
        };
        let paged = self.paged;
        if let Some(found) = found {
            self.look = Some(paged.occurrences[found].look.clone());
        }
        let look = self.look.clone();
        // Typst's own spacing (as between a heading's number and its text) is not text in HTML.
        if let (Some(found), Some(before)) = (found, self.previous_run) {
            let (now, then) = (&paged.occurrences[found], &paged.occurrences[before]);
            let gap = now.x - then.right;
            // A space already written counts for a quarter em.
            let written = if self.spaced || text.starts_with(char::is_whitespace) {
                0.25 * now.look.size
            } else {
                0.0
            };
            if now.page == then.page
                && (now.baseline - then.baseline).abs() < 0.5
                && gap - written > 0.2 * now.look.size
            {
                let extra = gap - written - 0.25 * now.look.size;
                let properties = self.run_properties(Some(&now.look), format, extra);
                let _ = write!(
                    self.runs,
                    r#"<w:r>{properties}<w:t xml:space="preserve"> </w:t></w:r>"#
                );
            }
        }
        let properties = self.run_properties(look.as_ref(), format, 0.0);
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                let _ = write!(self.runs, "<w:r>{properties}<w:br/></w:r>");
            }
            if !line.is_empty() {
                let _ = write!(
                    self.runs,
                    r#"<w:r>{properties}<w:t xml:space="preserve">{}</w:t></w:r>"#,
                    escape(line)
                );
            }
        }
        if found.is_some() {
            self.previous_run = found;
        }
        self.spaced = text.ends_with(char::is_whitespace);
    }

    fn frame(&mut self, frame: &HtmlFrame) {
        let picture = match single_image(&frame.inner) {
            Some((image, size)) => media(image, size).map(|media| {
                (
                    media,
                    size.x.to_pt(),
                    size.y.to_pt(),
                    image.alt().map(str::to_string),
                )
            }),
            None => png(&frame.inner, 2.0).map(|bytes| {
                let size = frame.inner.size();
                let media = Media {
                    bytes,
                    extension: "png",
                };
                (media, size.x.to_pt(), size.y.to_pt(), None)
            }),
        };
        // A picture that cannot be drawn is left out rather than failing the whole file.
        let Ok((media, width, height, alt)) = picture else {
            return;
        };
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        // Where Typst put the image: its page, size and place.
        let paged = self.paged;
        let placement = drawn(&frame.inner).and_then(|span| {
            let seen = self.placed.entry(span).or_default();
            *seen += 1;
            paged.image(span, *seen - 1)
        });
        if placement.is_some_and(|placement| paged.sheets[placement.page].designed) {
            return;
        }
        self.picture(media, (width, height), alt.as_deref(), placement);
    }

    /// The figure with `key` as Typst drew it, cut from its page.
    fn drawing_of_figure(&mut self, key: u128) {
        let paged = self.paged;
        let seen = self.figures_seen.entry(key).or_default();
        *seen += 1;
        let Some(placement) = paged.figure(key, *seen - 1) else {
            return;
        };
        if paged.sheets[placement.page].designed {
            return;
        }
        // A little room around it, for strokes that reach past the shapes they outline.
        let room = Point::splat(Abs::pt(2.0));
        let rect = Rect::new(placement.rect.min - room, placement.rect.max + room);
        let placement = paged::Placement::at(placement.page, rect);
        let placement = &placement;
        let size = typst::layout::Size::new(rect.max.x - rect.min.x, rect.max.y - rect.min.y);
        let mut frame = Frame::soft(size);
        frame.push(
            Point::zero() - rect.min,
            FrameItem::Group(typst::layout::GroupItem::new(
                self.document.pages()[placement.page].frame.clone(),
            )),
        );
        let Ok(bytes) = png(&frame, 2.0) else {
            return;
        };
        let media = Media {
            bytes,
            extension: "png",
        };
        self.picture(
            media,
            (size.x.to_pt(), size.y.to_pt()),
            None,
            Some(placement),
        );
    }

    /// A picture in a paragraph of its own, sized and spaced as Typst placed it if known.
    fn picture(
        &mut self,
        media: Media,
        (width, height): (f64, f64),
        alt: Option<&str>,
        placement: Option<&paged::Placement>,
    ) {
        let (width, height) = placement.map_or((width, height), |placement| {
            let rect = placement.rect;
            (
                (rect.max.x - rect.min.x).to_pt(),
                (rect.max.y - rect.min.y).to_pt(),
            )
        });
        let scale = (self.room / width).min(1.0);
        self.flush();
        self.roles.push(Role::Figure);
        let link = self.parts.picture(&mut self.links, media);
        let id = self.parts.id();
        let picture = drawing::inline_picture(id, &link, width * scale, height * scale, alt);
        self.drawing = true;
        let geometry = placement.map(|placement| Geometry {
            lines: vec![Line {
                page: placement.page,
                baseline: placement.rect.max.y.to_pt(),
                left: placement.rect.min.x.to_pt(),
                right: placement.rect.max.x.to_pt(),
                size: height,
            }],
            // Word sets a picture on the baseline: its line reaches from its top to the baseline.
            pitch: Some(height / BASELINE),
        });
        let properties = self.properties(geometry.as_ref());
        if let Some((line, _)) = self.previous.filter(|_| placement.is_some()) {
            self.previous = Some((line, 0.0));
        }
        self.drawing = false;
        let page = placement.map(|placement| placement.page);
        self.push(format!("<w:p>{properties}{picture}</w:p>"), page);
        self.roles.pop();
    }

    /// A Word footnote for Typst's footnote `id`, written where its marker is.
    fn footnote(&mut self, id: &str) -> bool {
        let Some(note) = self.notes.get(id).cloned() else {
            return false;
        };
        let number = self.footnotes.len() + 1;
        // The note's text is written on its own, then everything is put back.
        let runs = std::mem::take(&mut self.runs);
        let found = std::mem::take(&mut self.found);
        let out = std::mem::take(&mut self.out);
        let previous = self.previous.take();
        let lists = std::mem::take(&mut self.lists);
        let roles = std::mem::take(&mut self.roles);
        let headings = std::mem::take(&mut self.headings);
        let (previous_run, look, spaced, partial, item) = (
            self.previous_run.take(),
            self.look.clone(),
            self.spaced,
            self.partial.take(),
            self.item,
        );
        std::mem::swap(&mut self.links, &mut self.note_links);
        self.in_note = true;
        self.footnotes.push(String::new());
        let content: Vec<HtmlNode> = note
            .children
            .iter()
            .filter(|node| {
                !matches!(node, HtmlNode::Element(child) if attribute(child, "role") == Some("doc-backlink"))
            })
            .cloned()
            .collect();
        self.nodes(&content, &Format::default());
        self.flush();
        let mut xml: String = std::mem::take(&mut self.out)
            .into_iter()
            .map(|out| out.xml)
            .collect();
        let mark =
            r#"<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r>"#;
        match xml.find("</w:pPr>") {
            Some(at) if xml.starts_with("<w:p><w:pPr>") => {
                xml.insert_str(at + "</w:pPr>".len(), mark)
            }
            _ => {
                xml = format!(
                    r#"<w:p><w:pPr><w:pStyle w:val="FootnoteText"/></w:pPr>{mark}</w:p>{xml}"#
                )
            }
        }
        self.footnotes[number - 1] = format!(r#"<w:footnote w:id="{number}">{xml}</w:footnote>"#);
        self.in_note = false;
        std::mem::swap(&mut self.links, &mut self.note_links);
        self.runs = runs;
        self.found = found;
        self.out = out;
        self.previous = previous;
        self.lists = lists;
        self.roles = roles;
        self.headings = headings;
        self.previous_run = previous_run;
        self.look = look;
        self.spaced = spaced;
        self.partial = partial;
        self.item = item;
        let _ = write!(
            self.runs,
            r#"<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="{number}"/></w:r>"#
        );
        true
    }

    fn list(&mut self, element: &HtmlElement, format: &Format) {
        self.flush();
        let plain = css(element).contains("list-style-type:none");
        let number = if plain {
            None
        } else if &*name(element) == "ol" {
            let start = attribute(element, "start")
                .and_then(|start| start.parse().ok())
                .unwrap_or(1);
            self.numbers.push(start);
            Some(self.numbers.len() + 1)
        } else {
            Some(1)
        };
        self.lists.push(List { number, text: None });
        self.nodes(&element.children, format);
        self.flush();
        self.lists.pop();
    }

    fn table(&mut self, element: &HtmlElement, format: &Format) {
        self.flush();
        let mut rows: Vec<(&HtmlElement, bool)> = Vec::new();
        for node in &element.children {
            let HtmlNode::Element(child) = node else {
                continue;
            };
            match &*name(child) {
                "tr" => rows.push((child, false)),
                group @ ("thead" | "tbody" | "tfoot") => rows.extend(
                    child
                        .children
                        .iter()
                        .filter_map(|node| match node {
                            HtmlNode::Element(row) if &*name(row) == "tr" => Some(row),
                            _ => None,
                        })
                        .map(|row| (row, group == "thead")),
                ),
                _ => {}
            }
        }
        let span = |cell: &HtmlElement, key: &str| {
            attribute(cell, key)
                .and_then(|span| span.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1)
        };
        let columns = rows
            .iter()
            .map(|(row, _)| cells(row).iter().map(|cell| span(cell, "colspan")).sum())
            .max()
            .unwrap_or(1)
            .max(1);

        // Write every cell first: where their text sits on the page gives the columns.
        let outer = std::mem::take(&mut self.out);
        let previous = self.previous.take();
        let mut written: Vec<Cell> = Vec::new();
        let mut covered = vec![0_usize; columns];
        for (row_index, (row, _)) in rows.iter().enumerate() {
            let mut at = 0;
            for cell in cells(row) {
                while at < columns && covered[at] > 0 {
                    at += 1;
                }
                if at >= columns {
                    break;
                }
                let wide = span(cell, "colspan").min(columns - at);
                let tall = span(cell, "rowspan");
                covered[at..at + wide].fill(tall);
                let mut inside = format.clone();
                inside.bold |= &*name(cell) == "th";
                self.cell = Some(Vec::new());
                self.previous = None;
                self.nodes(&cell.children, &inside);
                self.flush();
                let found = self.cell.take().unwrap_or_default();
                let content: String = std::mem::take(&mut self.out)
                    .into_iter()
                    .map(|out| out.xml)
                    .collect();
                written.push(Cell {
                    row: row_index,
                    column: at,
                    wide,
                    tall,
                    content,
                    found,
                });
                at += wide;
            }
            for count in &mut covered {
                *count = count.saturating_sub(1);
            }
        }
        self.out = outer;
        self.previous = previous;

        let paged = self.paged;
        let first = written
            .iter()
            .find_map(|cell| cell.found.first().copied())
            .map(|found| &paged.occurrences[found]);
        let page = first.map(|first| first.page);
        let region = page.map_or_else(
            || Rect::new(Point::zero(), Point::with_x(Abs::pt(self.room))),
            |page| self.region(page),
        );
        let fill_of = |cell: &Cell| {
            let found = &paged.occurrences[*cell.found.first()?];
            self.fill_at(
                found.page,
                Point::new(Abs::pt(found.x + 0.5), Abs::pt(found.baseline - 1.0)),
            )
        };
        // Columns: from the cells' backgrounds, or else from where their text starts.
        let mut lefts: Vec<Option<f64>> = vec![None; columns + 1];
        let mut padding: Option<(f64, f64, f64)> = None;
        for cell in written.iter().filter(|cell| cell.wide == 1) {
            if let Some(fill) = fill_of(cell) {
                let (left, right) = (fill.rect.min.x.to_pt(), fill.rect.max.x.to_pt());
                lefts[cell.column] =
                    Some(lefts[cell.column].map_or(left, |known: f64| known.min(left)));
                lefts[cell.column + 1] =
                    Some(lefts[cell.column + 1].map_or(right, |known: f64| known.max(right)));
                if padding.is_none()
                    && let Some(found) = cell.found.first().map(|&found| &paged.occurrences[found])
                {
                    // Word puts the cell's one line of `line` height between these margins, with its
                    // baseline where Typst has it.
                    let line = found.look.size * self.leading;
                    padding = Some((
                        found.x - left,
                        found.baseline - fill.rect.min.y.to_pt() - BASELINE * line,
                        fill.rect.max.y.to_pt() - found.baseline - (1.0 - BASELINE) * line,
                    ));
                }
            }
        }
        let inset = padding.map_or(5.0, |(side, _, _)| side.clamp(0.0, 30.0));
        for cell in written.iter().filter(|cell| cell.wide == 1) {
            if lefts[cell.column].is_none()
                && let Some(left) = cell
                    .found
                    .iter()
                    .map(|&found| paged.occurrences[found].x)
                    .reduce(f64::min)
            {
                lefts[cell.column] = Some(left - inset);
            }
        }
        if lefts[columns].is_none() {
            let right = written
                .iter()
                .flat_map(|cell| cell.found.iter())
                .map(|&found| paged.occurrences[found].right + inset)
                .fold(f64::MIN, f64::max)
                .min(region.max.x.to_pt());
            lefts[columns] = Some(right);
        }
        let start = lefts[0].unwrap_or(region.min.x.to_pt());
        lefts[0] = Some(start);
        let total = lefts[columns].unwrap_or(region.max.x.to_pt()) - start;
        let mut widths = Vec::with_capacity(columns);
        let mut known = start;
        for column in 0..columns {
            let next = (column + 1..=columns).find_map(|index| lefts[index]);
            let missing = (column + 1..=columns)
                .take_while(|index| lefts[*index].is_none())
                .count();
            let width = next.map_or(total / columns as f64, |next| {
                (next - known) / (missing + 1) as f64
            });
            widths.push(width.max(12.0));
            known += width;
        }

        // Lines: Typst draws them as separate strokes; Word wants borders.
        let area = Rect::new(
            Point::new(Abs::pt(start - 1.0), Abs::pt(f64::MIN)),
            Point::new(Abs::pt(start + total + 1.0), Abs::pt(f64::MAX)),
        );
        let strokes: Vec<&paged::Stroke> = page
            .map(|page| {
                let rows: Vec<f64> = written
                    .iter()
                    .flat_map(|cell| cell.found.iter())
                    .map(|&found| paged.occurrences[found].baseline)
                    .collect();
                let (top, bottom) = rows
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(top, bottom), row| {
                        (top.min(*row), bottom.max(*row))
                    });
                paged.sheets[page]
                    .strokes
                    .iter()
                    .filter(|stroke| {
                        stroke.rect.max.x > area.min.x
                            && stroke.rect.min.x < area.max.x
                            && stroke.rect.max.y.to_pt() >= top - 40.0
                            && stroke.rect.min.y.to_pt() <= bottom + 40.0
                    })
                    .collect()
            })
            .unwrap_or_default();
        let border = |vertical: bool| {
            strokes
                .iter()
                .find(|stroke| {
                    let (width, height) = (
                        (stroke.rect.max.x - stroke.rect.min.x).to_pt(),
                        (stroke.rect.max.y - stroke.rect.min.y).to_pt(),
                    );
                    if vertical {
                        height > width
                    } else {
                        width >= height
                    }
                })
                .map_or_else(
                    || r#"w:val="nil""#.to_string(),
                    |stroke| {
                        format!(
                            r#"w:val="single" w:sz="{}" w:space="0" w:color="{}""#,
                            (stroke.thickness * 8.0).round().clamp(2.0, 96.0) as i64,
                            colour(stroke.colour)
                        )
                    },
                )
        };
        let (across, down) = (border(false), border(true));
        let (_, top, bottom) = padding.unwrap_or((inset, 2.0, 2.0));
        // Typst fits a line to its last hundredth of a point; Word measures a little wider. Room
        // on the right keeps a line that just fits in Typst from wrapping in Word.
        let trailing = inset / 3.0;
        let mut xml = format!(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="{}" w:type="dxa"/><w:tblInd w:w="{}" w:type="dxa"/><w:tblBorders><w:top {across}/><w:left {down}/><w:bottom {across}/><w:right {down}/><w:insideH {across}/><w:insideV {down}/></w:tblBorders><w:tblLayout w:type="fixed"/><w:tblCellMar><w:top w:w="{}" w:type="dxa"/><w:left w:w="{}" w:type="dxa"/><w:bottom w:w="{}" w:type="dxa"/><w:right w:w="{}" w:type="dxa"/></w:tblCellMar><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>"#,
            twips(total),
            twips(start - region.min.x.to_pt()),
            twips(top.clamp(0.0, 30.0)),
            twips(inset),
            twips(bottom.clamp(0.0, 30.0)),
            twips(trailing),
        );
        for width in &widths {
            let _ = write!(xml, r#"<w:gridCol w:w="{}"/>"#, twips(*width));
        }
        xml.push_str("</w:tblGrid>");
        // A row's baseline, for finding the background of a cell without text.
        let baselines: HashMap<usize, f64> = written
            .iter()
            .filter_map(|cell| Some((cell.row, paged.occurrences[*cell.found.first()?].baseline)))
            .collect();
        for (row, (_, header)) in rows.iter().enumerate() {
            xml.push_str("<w:tr>");
            if *header {
                xml.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            let mut column = 0;
            for cell in written.iter().filter(|cell| cell.row == row) {
                // Cells covered from a row above.
                while column < cell.column {
                    let _ = write!(
                        xml,
                        r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/><w:vMerge/></w:tcPr><w:p/></w:tc>"#,
                        twips(widths[column])
                    );
                    column += 1;
                }
                let width: f64 = widths[cell.column..cell.column + cell.wide].iter().sum();
                // Typst aligns a cell's text within the cell; Word wants that on the paragraph.
                let cell_left = start + widths[..cell.column].iter().sum::<f64>();
                let reach = cell
                    .found
                    .iter()
                    .map(|&found| &paged.occurrences[found])
                    .fold((f64::MAX, f64::MIN), |(left, right), found| {
                        (left.min(found.x), right.max(found.right))
                    });
                let align = if reach.0 > reach.1 {
                    None
                } else if ((reach.0 + reach.1) / 2.0 - (cell_left + width / 2.0)).abs() < 1.5
                    && reach.0 > cell_left + inset + 1.0
                {
                    Some("center")
                } else if (cell_left + width - inset - reach.1).abs() < 1.5
                    && reach.0 > cell_left + inset + 1.0
                {
                    Some("right")
                } else {
                    None
                };
                let content = match align {
                    Some(align) => cell
                        .content
                        .replace(r#"<w:jc w:val="center"/>"#, "")
                        .replace(r#"<w:jc w:val="right"/>"#, "")
                        .replace("</w:pPr>", &format!(r#"<w:jc w:val="{align}"/></w:pPr>"#)),
                    None => cell.content.clone(),
                };
                let fill = fill_of(cell).or_else(|| {
                    let page = page?;
                    let middle = lefts[cell.column]? + widths[cell.column] / 2.0;
                    self.fill_at(
                        page,
                        Point::new(Abs::pt(middle), Abs::pt(*baselines.get(&cell.row)? - 1.0)),
                    )
                });
                let shading = fill.map_or_else(String::new, |fill| {
                    format!(
                        r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                        colour(fill.colour)
                    )
                });
                let _ = write!(
                    xml,
                    r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>{}{}{shading}</w:tcPr>{}</w:tc>"#,
                    twips(width),
                    if cell.wide > 1 {
                        format!(r#"<w:gridSpan w:val="{}"/>"#, cell.wide)
                    } else {
                        String::new()
                    },
                    if cell.tall > 1 {
                        r#"<w:vMerge w:val="restart"/>"#
                    } else {
                        ""
                    },
                    if content.is_empty() {
                        "<w:p/>"
                    } else {
                        &content
                    },
                );
                column = cell.column + cell.wide;
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl>");
        // Word has no space above a table: the paragraph before it takes Typst's gap below itself.
        let table_top = page.and_then(|page| {
            written
                .iter()
                .filter(|cell| cell.row == 0)
                .filter_map(|cell| fill_of(cell).map(|fill| fill.rect.min.y.to_pt()))
                .reduce(f64::min)
                .or_else(|| {
                    let found = &paged.occurrences[*written.first()?.found.first()?];
                    (found.page == page).then_some(found.baseline - found.ascent - top)
                })
        });
        if let (Some(table_top), Some((line, height))) = (table_top, previous)
            && Some(line.page) == page
            && let Some(before) = self.out.last_mut()
            && before.xml.starts_with("<w:p><w:pPr>")
        {
            let gap = table_top - line.baseline - (1.0 - BASELINE) * height;
            if gap > 0.5 {
                before.xml = before.xml.replacen(
                    r#"w:after="0""#,
                    &format!(r#"w:after="{}""#, twips(gap.min(200.0))),
                    1,
                );
            }
        }
        self.push(xml, page);
        // The text after the table is spaced from the table's bottom.
        let last = written
            .iter()
            .flat_map(|cell| cell.found.iter())
            .map(|&found| &paged.occurrences[found])
            .filter(|found| Some(found.page) == page)
            .max_by(|a, b| a.baseline.total_cmp(&b.baseline));
        if let Some(last) = last {
            let height = last.look.size * self.leading;
            let line = Line {
                page: last.page,
                baseline: last.baseline + (1.0 - BASELINE) * height + bottom.clamp(0.0, 30.0),
                left: last.x,
                right: last.right,
                size: last.look.size,
            };
            self.previous = Some((line, 0.0));
        }
    }

    fn element(&mut self, element: &HtmlElement, format: &Format) {
        let mut inner = format.clone();
        match &*name(element) {
            "head" | "style" | "script" | "meta" | "title" | "img" => {}
            "section"
                if attribute(element, "role") == Some("doc-endnotes") && !self.notes.is_empty() => {
            }
            "p" | "div" | "section" | "article" | "main" | "header" | "footer" | "aside"
            | "nav" | "address" | "body" | "html" => {
                let level = attribute(element, "aria-level")
                    .filter(|_| attribute(element, "role") == Some("heading"))
                    .and_then(|level| level.parse::<usize>().ok());
                if let Some(id) = attribute(element, "id") {
                    self.flush();
                    self.bookmarks.push(bookmark(id));
                }
                let outline = attribute(element, "role") == Some("doc-toc");
                self.outline |= outline;
                self.block(
                    level.map(|level| Role::Heading(level.saturating_sub(1).clamp(1, 6))),
                    element,
                    format,
                );
                if outline {
                    self.outline = false;
                }
            }
            heading @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                let level = heading[1..].parse::<usize>().unwrap_or(2) - 1;
                if let Some(id) = attribute(element, "id") {
                    self.flush();
                    self.bookmarks.push(bookmark(id));
                }
                self.block(Some(Role::Heading(level)), element, format);
            }
            "figure" => {
                // Content HTML has no form for (a diagram drawn with `layout`) comes out empty:
                // the figure is then taken from the page as a picture.
                let key = self.figure.take();
                let shown = element.children.iter().any(|node| match node {
                    HtmlNode::Element(child) if &*name(child) == "figcaption" => false,
                    other => holds(std::slice::from_ref(other)),
                });
                if !shown && let Some(key) = key {
                    self.flush();
                    self.drawing_of_figure(key);
                }
                self.block(Some(Role::Figure), element, format)
            }
            "figcaption" => self.block(Some(Role::Caption), element, format),
            "blockquote" => self.block(Some(Role::Quote), element, format),
            "pre" => self.block(Some(Role::Code), element, format),
            // Typst sets a term and its description on one line.
            "dt" => {
                self.flush();
                inner.bold = true;
                self.nodes(&element.children, &inner);
            }
            "dd" => {
                self.nodes(&element.children, format);
                self.flush();
            }
            "dl" => self.block(None, element, format),
            "ul" | "ol" | "menu" => self.list(element, format),
            "li" => {
                self.flush();
                if let Some(id) = attribute(element, "id") {
                    self.bookmarks.push(bookmark(id));
                }
                self.item = true;
                self.nodes(&element.children, format);
                self.flush();
                self.item = false;
            }
            "table" => self.table(element, format),
            "hr" => {
                self.flush();
                if let Some(id) = attribute(element, "id") {
                    self.bookmarks.push(bookmark(id));
                }
                if attribute(element, "class") == Some(PAGE_BREAK) {
                    self.out.push(Out {
                        xml: r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#.into(),
                        page: None,
                        page_break: true,
                    });
                    self.previous = None;
                } else {
                    self.push(
                        r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="auto"/></w:pBdr></w:pPr></w:p>"#.into(),
                        None,
                    );
                }
            }
            "br" => {
                let properties = self.run_properties(self.look.clone().as_ref(), format, 0.0);
                let _ = write!(self.runs, "<w:r>{properties}<w:br/></w:r>");
            }
            "math" => {
                let start = self.found.len();
                let mut spans = HashSet::new();
                self::spans(&element.children, &mut spans);
                if let Some(span) = first_span(&element.children) {
                    self.find(span, "");
                }
                let look = self
                    .found
                    .get(start)
                    .map(|&found| self.paged.occurrences[found].look.clone());
                let run = match look.as_ref().or(self.look.as_ref()) {
                    Some(look) => format!(
                        r#"<w:rPr><w:rFonts w:ascii="Cambria Math" w:hAnsi="Cambria Math"/><w:color w:val="{}"/><w:sz w:val="{1}"/><w:szCs w:val="{1}"/></w:rPr>"#,
                        colour(look.colour),
                        (look.size * 2.0).round() as i64
                    ),
                    None => r#"<w:rPr><w:rFonts w:ascii="Cambria Math" w:hAnsi="Cambria Math"/></w:rPr>"#.into(),
                };
                let omml = math::omml(element, &run);
                if attribute(element, "display") == Some("block") {
                    let found = self.found.split_off(start);
                    self.flush();
                    self.found = found;
                    self.tall = true;
                    self.centre = true;
                    let geometry = self.geometry();
                    let properties = self.properties(geometry.as_ref());
                    self.centre = false;
                    let marks = self.marks();
                    let page = geometry.map(|geometry| geometry.first().page);
                    self.push(
                        format!("<w:p>{properties}{marks}<m:oMathPara>{omml}</m:oMathPara></w:p>"),
                        page,
                    );
                    self.found.clear();
                    self.tall = false;
                } else {
                    self.tall = true;
                    // An empty run first, so a paragraph that opens with a formula is not skipped.
                    self.runs.push_str("<w:r/>");
                    self.runs.push_str(&omml);
                    self.previous_run = None;
                    self.spaced = false;
                }
            }
            "a" => {
                inner.link = true;
                let start = self.runs.len();
                let target = attribute(element, "href").unwrap_or_default();
                if self.outline
                    && let Some(anchor) = target.strip_prefix('#')
                {
                    self.target = Some(anchor.to_string());
                }
                self.nodes(&element.children, &inner);
                let content = self.runs.split_off(start.min(self.runs.len()));
                let opening = match target.strip_prefix('#') {
                    Some(anchor) => format!(r#"<w:hyperlink w:anchor="{}">"#, bookmark(anchor)),
                    None if !target.is_empty() => {
                        format!(
                            r#"<w:hyperlink r:id="{}">"#,
                            link(&mut self.links, "hyperlink", target.to_string())
                        )
                    }
                    None => String::new(),
                };
                if opening.is_empty() {
                    self.runs.push_str(&content);
                } else {
                    let _ = write!(self.runs, "{opening}{content}</w:hyperlink>");
                }
            }
            "sup" if attribute(element, "role") == Some("doc-noteref") => {
                let mut links = Vec::new();
                collect(&element.children, "a", &mut links);
                let id = links
                    .first()
                    .and_then(|link| attribute(link, "href"))
                    .and_then(|href| href.strip_prefix('#'))
                    .map(str::to_string);
                if !id.is_some_and(|id| self.footnote(&id)) {
                    inner.script = Some("superscript");
                    self.nodes(&element.children, &inner);
                }
            }
            other => {
                let css = css(element);
                match other {
                    "strong" | "b" => inner.bold = true,
                    "em" | "i" | "cite" | "dfn" | "var" => inner.italic = true,
                    "u" | "ins" => inner.underline = true,
                    "s" | "del" | "strike" => inner.strike = true,
                    "sub" => inner.script = Some("subscript"),
                    "sup" => inner.script = Some("superscript"),
                    "code" | "kbd" | "samp" => inner.code = true,
                    "mark" => inner.highlight = true,
                    _ => {}
                }
                inner.underline |= css.contains("text-decoration:underline");
                inner.strike |= css.contains("text-decoration:line-through");
                inner.small_caps |= css.contains("small-caps");
                if let Some(colour) = css
                    .split(';')
                    .find_map(|rule| rule.strip_prefix("color:#"))
                    .filter(|colour| colour.len() >= 6)
                {
                    inner.colour = Some(colour[..6].to_ascii_uppercase());
                }
                if css.contains("display:block") {
                    self.block(None, element, &inner);
                } else {
                    self.nodes(&element.children, &inner);
                }
            }
        }
    }
}

/// The look most of the body text has.
fn body_look(paged: &Paged) -> Option<Look> {
    // Per family, size, weight, slant and colour: how wide its text runs, and one example.
    type Kind = (String, i64, bool, bool, [u8; 3]);
    let mut counts: HashMap<Kind, (f64, usize)> = HashMap::new();
    for (index, occurrence) in paged.occurrences.iter().enumerate() {
        if occurrence.chrome || paged.sheets[occurrence.page].designed {
            continue;
        }
        let look = &occurrence.look;
        let key = (
            look.family.clone(),
            (look.size * 10.0).round() as i64,
            look.bold,
            look.italic,
            look.colour,
        );
        let entry = counts.entry(key).or_insert((0.0, index));
        entry.0 += occurrence.right - occurrence.x;
    }
    counts
        .into_values()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| paged.occurrences[index].look.clone())
}

/// Baseline distance over font size, for paragraphs of one line.
fn leading(paged: &Paged) -> f64 {
    let mut ratios: Vec<f64> = Vec::new();
    for block in &paged.blocks {
        let mut baselines: Vec<(usize, f64, f64)> = block
            .occurrences
            .iter()
            .map(|&index| &paged.occurrences[index])
            .filter(|occurrence| !occurrence.chrome)
            .map(|occurrence| (occurrence.page, occurrence.baseline, occurrence.look.size))
            .collect();
        baselines.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        baselines.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 0.5);
        ratios.extend(
            baselines
                .windows(2)
                .filter(|pair| pair[0].0 == pair[1].0 && pair[1].1 > pair[0].1)
                .map(|pair| (pair[1].1 - pair[0].1) / pair[0].2),
        );
    }
    ratios.sort_by(f64::total_cmp);
    ratios
        .get(ratios.len() / 2)
        .copied()
        .unwrap_or(1.3)
        .clamp(1.0, 2.5)
}

fn styles(body: Option<&Look>, headings: &[Option<Look>; 7], language: &str) -> String {
    let properties = |look: &Look| {
        drawing::run_properties(&look.family, look.size, look.bold, look.italic, look.colour)
    };
    // Kerning on, as Typst does; Word leaves it off unless asked.
    let defaults = body
        .map(properties)
        .unwrap_or_default()
        .replace("<w:sz ", r#"<w:kern w:val="2"/><w:sz "#);
    let mut xml = format!(
        r#"{XML}<w:styles xmlns:w="{MAIN}"><w:docDefaults><w:rPrDefault><w:rPr>{defaults}<w:lang w:val="{}"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="264" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>"#,
        escape(language)
    );
    for (level, look) in headings.iter().enumerate() {
        let (id, name) = if level == 0 {
            ("Title".to_string(), "Title".to_string())
        } else {
            (format!("Heading{level}"), format!("heading {level}"))
        };
        let outline = if level == 0 {
            String::new()
        } else {
            format!(r#"<w:outlineLvl w:val="{}"/>"#, level - 1)
        };
        let font = match (look, body) {
            (Some(look), _) => properties(look),
            (None, Some(body)) => drawing::run_properties(
                &body.family,
                body.size * [2.0, 1.6, 1.35, 1.2, 1.1, 1.0, 1.0][level],
                true,
                false,
                body.colour,
            ),
            _ => String::new(),
        };
        let _ = write!(
            xml,
            r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="{}"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/>{outline}</w:pPr><w:rPr>{font}</w:rPr></w:style>"#,
            if level == 0 { 10 } else { 9 },
        );
    }
    xml.push_str(r#"<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="SourceCode"><w:name w:val="Source Code"/><w:basedOn w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="34"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/></w:style><w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style><w:style w:type="character" w:styleId="VerbatimChar"><w:name w:val="Verbatim Char"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/></w:rPr></w:style><w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:pPr><w:spacing w:after="0"/></w:pPr></w:style></w:styles>"#);
    xml
}

fn numbering(starts: &[usize]) -> String {
    let level = |kind: &str, level: usize, text: String| {
        format!(
            r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{kind}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
            720 * (level + 1)
        )
    };
    // Typst's own markers.
    let bullets: String = (0..9)
        .map(|depth| level("bullet", depth, ["•", "‣", "–"][depth % 3].to_string()))
        .collect();
    let numbers: String = (0..9)
        .map(|depth| level("decimal", depth, format!("%{}.", depth + 1)))
        .collect();
    let mut xml = format!(
        r#"{XML}<w:numbering xmlns:w="{MAIN}"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>{bullets}</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>{numbers}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#
    );
    // Each ordered list counts from its own start, at whatever depth it sits.
    for (index, start) in starts.iter().enumerate() {
        let overrides: String = (0..9)
            .map(|depth| {
                format!(
                    r#"<w:lvlOverride w:ilvl="{depth}"><w:startOverride w:val="{start}"/></w:lvlOverride>"#
                )
            })
            .collect();
        let _ = write!(
            xml,
            r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/>{overrides}</w:num>"#,
            index + 2
        );
    }
    xml.push_str("</w:numbering>");
    xml
}

/// A run of pages Word shows with one header (or one designed page).
struct Section {
    pages: Vec<usize>,
    designed: bool,
}

/// The header, footer and background of a page, without its body.
struct Chrome {
    background: typst_layout::Page,
    layers: Layers,
}

fn chrome(document: &PagedDocument, paged: &Paged, index: usize) -> Result<Chrome, String> {
    let page = paged.chrome(&document.pages()[index], index);
    let (background, layers) = layers(&page, &Equations::new())?;
    Ok(Chrome { background, layers })
}

/// What tells one page's header apart from another's: the picture and the text, numbers aside.
fn signature(chrome: &Chrome) -> u128 {
    let texts: Vec<String> = chrome
        .layers
        .runs
        .iter()
        .map(|run| {
            run.text
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect()
        })
        .collect();
    let mut page = chrome.background.clone();
    if page.fill == typst::foundations::Smart::Auto {
        page.fill = typst::foundations::Smart::Custom(None);
    }
    let pixels = typst_render::render(&page, &typst_render::RenderOptions::default());
    hash128(&(pixels.data(), texts))
}

/// Which numbers in the header of `section` are page numbers: those that differ from the next
/// page's in step with the page number.
fn fields(
    first: &Chrome,
    second: Option<&Chrome>,
    numbers: (u64, Option<u64>),
    pages: usize,
    total: usize,
) -> Vec<Vec<Option<Field>>> {
    let (Some(second), (number, Some(next))) = (second, numbers) else {
        return Vec::new();
    };
    let ours = paragraphs(lines(first.layers.runs.clone()));
    let theirs = paragraphs(lines(second.layers.runs.clone()));
    if ours.len() != theirs.len() {
        return Vec::new();
    }
    ours.iter()
        .zip(&theirs)
        .map(|(ours, theirs)| {
            let (a, b) = (drawing::numbers(ours), drawing::numbers(theirs));
            if a.len() != b.len() {
                return Vec::new();
            }
            let mut fields: Vec<Option<Field>> = a
                .iter()
                .zip(&b)
                .map(|(a, b)| {
                    (*a == number.to_string() && *b == next.to_string()).then_some(Field::Page)
                })
                .collect();
            if fields.contains(&Some(Field::Page)) {
                for (field, (a, b)) in fields.iter_mut().zip(a.iter().zip(&b)) {
                    if field.is_none() && a == b {
                        if *a == pages.to_string() {
                            *field = Some(Field::SectionPages);
                        } else if *a == total.to_string() {
                            *field = Some(Field::Pages);
                        }
                    }
                }
            }
            fields
        })
        .collect()
}

fn header(content: &str) -> String {
    format!(
        r#"{XML}<w:hdr {}><w:p>{TINY}{content}</w:p></w:hdr>"#,
        namespaces()
    )
}

fn section_properties(sheet: &paged::Sheet, designed: bool, header: &str) -> String {
    let orientation = if sheet.width > sheet.height {
        r#" w:orient="landscape""#
    } else {
        ""
    };
    let (top, right, bottom, left) = match sheet.body.filter(|_| !designed) {
        Some(body) => (
            body.min.y.to_pt(),
            sheet.width - body.max.x.to_pt(),
            sheet.height - body.max.y.to_pt(),
            body.min.x.to_pt(),
        ),
        None => (0.0, 0.0, 0.0, 0.0),
    };
    format!(
        r#"<w:sectPr><w:headerReference w:type="default" r:id="{header}"/><w:type w:val="nextPage"/><w:pgSz w:w="{}" w:h="{}"{orientation}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="0" w:footer="0" w:gutter="0"/><w:pgNumType w:start="{}"/></w:sectPr>"#,
        twips(sheet.width),
        twips(sheet.height),
        twips(top),
        twips(right),
        twips(bottom),
        twips(left),
        sheet.number,
    )
}

/// Ends a section in its last paragraph, or in one of its own after a table.
fn close(blocks: &mut Vec<String>, properties: &str) {
    match blocks.last_mut() {
        Some(last) if last.starts_with("<w:p><w:pPr>") => {
            let end = last.find("</w:pPr>").unwrap_or(last.len());
            last.insert_str(end, properties);
        }
        _ => blocks.push(format!(
            r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0" w:line="20" w:lineRule="exact"/>{properties}</w:pPr></w:p>"#
        )),
    }
}

fn content_types(headers: usize, footnotes: bool) -> String {
    let mut xml = format!(
        r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/><Override PartName="/word/document.xml" ContentType="{WORD}.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="{WORD}.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="{WORD}.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="{WORD}.settings+xml"/><Override PartName="/word/fontTable.xml" ContentType="{WORD}.fontTable+xml"/>"#
    );
    for number in 1..=headers {
        let _ = write!(
            xml,
            r#"<Override PartName="/word/header{number}.xml" ContentType="{WORD}.header+xml"/>"#
        );
    }
    if footnotes {
        let _ = write!(
            xml,
            r#"<Override PartName="/word/footnotes.xml" ContentType="{WORD}.footnotes+xml"/>"#
        );
    }
    xml.push_str("</Types>");
    xml
}

/// Hyphenation on when Typst justified text, as it then hyphenates too.
fn settings(hyphenate: bool, footnotes: bool) -> String {
    let hyphenation = if hyphenate {
        "<w:autoHyphenation/>"
    } else {
        ""
    };
    let notes = if footnotes {
        r#"<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr>"#
    } else {
        ""
    };
    format!(
        r#"{XML}<w:settings xmlns:w="{MAIN}"><w:embedTrueTypeFonts/>{hyphenation}{notes}<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#
    )
}

pub fn docx(
    document: &PagedDocument,
    html: &HtmlDocument,
    equations: &Equations,
) -> Result<Vec<u8>, String> {
    let failed = |error: zip::result::ZipError| format!("The Word file could not be made: {error}");
    let mut found = HashSet::new();
    spans(&html.root().children, &mut found);
    let paged = Paged::new(document, &found);
    if paged.sheets.is_empty() {
        return Err("This page has nothing to export.".into());
    }
    let mut targets_found = HashMap::new();
    targets(
        &html.root().children,
        &paged,
        &mut Vec::new(),
        &mut targets_found,
    );
    let room = text_width(document).to_pt();
    let mut parts = Parts {
        media: Vec::new(),
        ids: 0,
        faces: fonts::Faces::new(),
    };
    // Dot leaders: Typst repeats a dot (or a few) between an outline entry and its page.
    let leader = paged
        .blocks
        .iter()
        .filter(|block| block.kind == "entry")
        .any(|block| {
            block
                .occurrences
                .iter()
                .filter(|&&index| paged.occurrences[index].dots)
                .count()
                >= 3
        });
    let mut writer = Writer {
        paged: &paged,
        parts: &mut parts,
        links: vec![
            ("styles", "styles.xml".into()),
            ("numbering", "numbering.xml".into()),
            ("settings", "settings.xml".into()),
            ("fontTable", "fontTable.xml".into()),
        ],
        out: Vec::new(),
        runs: String::new(),
        found: Vec::new(),
        cell: None,
        cursor: 0,
        previous_run: None,
        look: None,
        spaced: false,
        tall: false,
        previous: None,
        leading: leading(&paged),
        roles: Vec::new(),
        headings: Vec::new(),
        heading_looks: Default::default(),
        lists: Vec::new(),
        item: false,
        bookmarks: Vec::new(),
        marks: 0,
        numbers: Vec::new(),
        outline: false,
        target: None,
        targets: targets_found,
        leader,
        room,
        placed: HashMap::new(),
        justified: false,
        partial: None,
        notes: notes(&html.root().children),
        footnotes: Vec::new(),
        note_links: Vec::new(),
        in_note: false,
        centre: false,
        spacing: HashMap::new(),
        drawing: false,
        figure: None,
        figures_seen: HashMap::new(),
        document,
    };
    writer.nodes(&html.root().children, &Format::default());
    writer.flush();
    let Writer {
        mut links,
        out,
        numbers,
        heading_looks,
        justified,
        footnotes,
        note_links,
        ..
    } = writer;
    if !footnotes.is_empty() {
        link(&mut links, "footnotes", "footnotes.xml".into());
    }
    let language = attribute(html.root(), "lang").unwrap_or("en").to_string();

    // Sections: each designed page alone; runs of other pages with the same header together.
    let mut sections: Vec<Section> = Vec::new();
    let mut chromes: HashMap<usize, Chrome> = HashMap::new();
    let mut last_signature = None;
    for (index, sheet) in paged.sheets.iter().enumerate() {
        if sheet.designed {
            sections.push(Section {
                pages: vec![index],
                designed: true,
            });
            last_signature = None;
            continue;
        }
        let page_chrome = chrome(document, &paged, index)?;
        let signature = signature(&page_chrome);
        chromes.insert(index, page_chrome);
        match sections.last_mut() {
            Some(section) if !section.designed && last_signature == Some(signature) => {
                section.pages.push(index)
            }
            _ => sections.push(Section {
                pages: vec![index],
                designed: false,
            }),
        }
        last_signature = Some(signature);
    }
    let mut section_of = vec![0; paged.sheets.len()];
    for (index, section) in sections.iter().enumerate() {
        for &page in &section.pages {
            section_of[page] = index;
        }
    }

    // The body's paragraphs go to the section of their page, in order.
    let mut contents: Vec<Vec<Out>> = (0..sections.len()).map(|_| Vec::new()).collect();
    let mut current = sections
        .iter()
        .position(|section| !section.designed)
        .unwrap_or(0);
    for block in out {
        if let Some(page) = block.page {
            if paged.sheets[page].designed {
                continue;
            }
            current = current.max(section_of[page]);
        }
        contents[current].push(block);
    }

    let mut headers: Vec<(String, Vec<(&'static str, String)>)> = vec![(header(""), Vec::new())];
    let empty = link(&mut links, "header", "header1.xml".into());
    let mut body = String::new();
    let total = paged.sheets.len();
    let shown: Vec<usize> = (0..sections.len())
        .filter(|&index| {
            sections[index].designed || contents[index].iter().any(|block| !block.page_break)
        })
        .collect();
    for (position, &index) in shown.iter().enumerate() {
        let section = &sections[index];
        let first = section.pages[0];
        let sheet = &paged.sheets[first];
        let mut content: Vec<String> = Vec::new();
        let header_link = if section.designed {
            let mut anchors_links = std::mem::take(&mut links);
            let anchors = drawing::copy(
                &document.pages()[first],
                equations,
                &[],
                &mut parts,
                &mut anchors_links,
            )?;
            links = anchors_links;
            content.push(format!("<w:p>{TINY}{anchors}</w:p>"));
            empty.clone()
        } else {
            let blocks = &contents[index];
            // Page breaks only between paragraphs: a section starts on a new page anyway.
            for (at, block) in blocks.iter().enumerate() {
                let inside = blocks[..at].iter().any(|block| !block.page_break)
                    && blocks[at + 1..].iter().any(|block| !block.page_break);
                if !block.page_break || inside {
                    content.push(block.xml.clone());
                }
            }
            let ours = &chromes[&first];
            let next = section.pages.get(1).map(|page| &chromes[page]);
            let numbers = (
                sheet.number,
                section.pages.get(1).map(|page| paged.sheets[*page].number),
            );
            let fields = fields(ours, next, numbers, section.pages.len(), total);
            let mut header_links = Vec::new();
            let chrome = chromes
                .remove(&first)
                .ok_or("The header could not be read.")?;
            let mut background = chrome.background;
            let anchors = drawing::copy_layers(
                &mut background,
                chrome.layers,
                &fields,
                &mut parts,
                &mut header_links,
            )?;
            if anchors.is_empty() {
                empty.clone()
            } else {
                headers.push((header(&anchors), header_links));
                link(&mut links, "header", format!("header{}.xml", headers.len()))
            }
        };
        let properties = section_properties(sheet, section.designed, &header_link);
        if position + 1 == shown.len() {
            body.extend(content);
            body.push_str(&properties);
        } else {
            close(&mut content, &properties);
            body.extend(content);
        }
    }
    if shown.is_empty() {
        let _ = write!(
            body,
            "<w:p/>{}",
            section_properties(&paged.sheets[0], false, &empty)
        );
    }

    let embedded = fonts::embed(&parts.faces);
    let document_xml = format!(
        r#"{XML}<w:document {}><w:body>{body}</w:body></w:document>"#,
        namespaces()
    );
    let mut parts_xml = vec![
        (
            "[Content_Types].xml".to_string(),
            content_types(headers.len(), !footnotes.is_empty()),
        ),
        (
            "_rels/.rels".into(),
            relationships(&[("officeDocument", "word/document.xml".into())]),
        ),
        ("word/document.xml".into(), document_xml),
        (
            "word/styles.xml".into(),
            styles(body_look(&paged).as_ref(), &heading_looks, &language),
        ),
        ("word/numbering.xml".into(), numbering(&numbers)),
        (
            "word/settings.xml".into(),
            settings(justified, !footnotes.is_empty()),
        ),
        ("word/fontTable.xml".into(), embedded.table.clone()),
        (
            "word/_rels/fontTable.xml.rels".into(),
            relationships(&embedded.links),
        ),
        ("word/_rels/document.xml.rels".into(), relationships(&links)),
    ];
    if !footnotes.is_empty() {
        parts_xml.push((
            "word/footnotes.xml".into(),
            format!(
                r#"{XML}<w:footnotes {}><w:footnote w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{}</w:footnotes>"#,
                namespaces(),
                footnotes.concat()
            ),
        ));
        parts_xml.push((
            "word/_rels/footnotes.xml.rels".into(),
            relationships(&note_links),
        ));
    }
    for (index, (xml, header_links)) in headers.into_iter().enumerate() {
        let number = index + 1;
        parts_xml.push((format!("word/header{number}.xml"), xml));
        parts_xml.push((
            format!("word/_rels/header{number}.xml.rels"),
            relationships(&header_links),
        ));
    }

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, xml) in parts_xml {
        zip.start_file(name, SimpleFileOptions::default())
            .map_err(failed)?;
        zip.write_all(xml.as_bytes())
            .map_err(|error| failed(error.into()))?;
    }
    for (folder, files) in [("media", &parts.media), ("fonts", &embedded.files)] {
        for (file, bytes) in files {
            zip.start_file(
                format!("word/{folder}/{file}"),
                SimpleFileOptions::default(),
            )
            .map_err(failed)?;
            zip.write_all(bytes).map_err(|error| failed(error.into()))?;
        }
    }
    Ok(zip.finish().map_err(failed)?.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use typst::layout::Size;
    use zip::ZipArchive;

    use super::*;
    use crate::export::tests::{document, html, part};

    /// The Word file for `source`, with every XML part checked to be well formed.
    fn word(source: &str) -> ZipArchive<Cursor<Vec<u8>>> {
        let (html, _) = html(source);
        let bytes = docx(&document(source), &html, &math::equations(&html)).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().unwrap().to_string();
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let mut text = String::new();
                file.read_to_string(&mut text).unwrap();
                roxmltree::Document::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
            }
        }
        archive
    }

    fn body(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> String {
        part(archive, "word/document.xml")
    }

    #[test]
    fn structure_becomes_word_structure() {
        let source = "#set page(width: 300pt, height: 400pt, margin: 30pt)\n#set heading(numbering: \"1.\")\n= Known heading\nSome *bold* text with #\"A & B < C\" and #link(\"https://example.com\")[a link].\n\n== Smaller\n- one\n- two\n  + nested\n  + list\n\n#table(columns: 2, table.header[*Name*][*Value*], [a], [b], table.cell(colspan: 2)[wide])";
        let mut archive = word(source);
        let body = body(&mut archive);
        assert!(body.contains(r#"<w:pStyle w:val="Heading1"/>"#), "{body}");
        assert!(body.contains(r#"<w:pStyle w:val="Heading2"/>"#), "{body}");
        assert!(body.contains("A &amp; B &lt; C"), "{body}");
        assert!(body.contains("<w:hyperlink r:id="), "{body}");
        assert!(body.contains(r#"<w:numId w:val="1"/>"#), "{body}");
        assert!(
            body.contains(r#"<w:ilvl w:val="1"/><w:numId w:val="2"/>"#),
            "{body}"
        );
        assert!(
            body.contains("<w:tbl>") && body.contains("<w:tblHeader/>"),
            "{body}"
        );
        assert!(body.contains(r#"<w:gridSpan w:val="2"/>"#), "{body}");
        assert!(
            body.contains(r#"<w:pgSz w:w="6000" w:h="8000"/>"#),
            "{body}"
        );
        assert!(body.contains(r#"w:left="600""#), "{body}");
        let links = part(&mut archive, "word/_rels/document.xml.rels");
        assert!(
            links.contains(r#"Target="https://example.com" TargetMode="External""#),
            "{links}"
        );
    }

    #[test]
    fn text_keeps_the_look_and_place_it_has_on_the_page() {
        let source = "#set text(font: \"DejaVu Sans Mono\", size: 12pt, fill: rgb(\"#123456\"))\n#set par(justify: true)\n= Heading\n#text(fill: rgb(\"#AA0000\"))[Red] text. #lorem(40)\n\n#align(center)[Centred]";
        let mut archive = word(source);
        let body = body(&mut archive);
        assert!(
            body.contains(r#"<w:rFonts w:ascii="DejaVu Sans Mono""#),
            "{body}"
        );
        assert!(body.contains(r#"<w:color w:val="123456"/>"#), "{body}");
        assert!(body.contains(r#"<w:color w:val="AA0000"/>"#), "{body}");
        assert!(body.contains(r#"<w:sz w:val="24"/>"#), "{body}");
        assert!(body.contains(r#"<w:jc w:val="both"/>"#), "{body}");
        assert!(body.contains(r#"<w:jc w:val="center"/>"#), "{body}");
        assert!(body.contains(r#"w:lineRule="exact""#), "{body}");
        // The font travels inside the file, obfuscated as Word wants.
        let table = part(&mut archive, "word/fontTable.xml");
        assert!(
            table.contains(r#"<w:font w:name="DejaVu Sans Mono"><w:embedRegular"#),
            "{table}"
        );
        let mut font = Vec::new();
        archive
            .by_name("word/fonts/font1.odttf")
            .unwrap()
            .read_to_end(&mut font)
            .unwrap();
        assert!(!font.is_empty());
        assert!(part(&mut archive, "word/settings.xml").contains("<w:embedTrueTypeFonts/>"));
    }

    #[test]
    fn header_and_footer_become_the_word_header_with_page_numbers() {
        let source = "#set page(width: 300pt, height: 300pt, header: [Running title], numbering: \"1 of 1\")\n#lorem(150)";
        let mut archive = word(source);
        let header = part(&mut archive, "word/header2.xml");
        assert!(header.contains("Running title"), "{header}");
        assert!(
            header.contains(r#"<w:fldSimple w:instr=" PAGE ">"#),
            "{header}"
        );
        assert!(
            header.contains(r#"<w:fldSimple w:instr=" SECTIONPAGES ">"#),
            "{header}"
        );
        let body = body(&mut archive);
        assert!(!body.contains("Running title"), "{body}");
        assert_eq!(body.matches("<w:sectPr>").count(), 1, "{body}");
        assert!(
            body.contains(r#"<w:headerReference w:type="default" r:id="#),
            "{body}"
        );
    }

    #[test]
    fn a_designed_page_is_copied_and_the_rest_flows() {
        let source = "#page(margin: 0pt)[#block(width: 100%, height: 100%, fill: navy)[#text(fill: white, size: 30pt)[Cover title]]]\n= First chapter\nBody text.";
        let mut archive = word(source);
        let body = body(&mut archive);
        assert_eq!(body.matches("<w:sectPr>").count(), 2, "{body}");
        let end = body.find("</w:sectPr>").unwrap();
        let cover = &body[..end + body[end..].find("</w:p>").unwrap()];
        assert!(
            cover.contains(r#"behindDoc="1""#) && cover.contains("<wps:txbx>"),
            "{cover}"
        );
        assert!(
            cover.contains("Cover title") && !cover.contains("First chapter"),
            "{cover}"
        );
        assert_eq!(body.matches("Cover title").count(), 1, "{body}");
        assert!(body.contains(r#"<w:pStyle w:val="Heading1"/>"#), "{body}");
    }

    #[test]
    fn footnotes_formulas_pictures_and_outlines_are_word_ones() {
        let dot = png(&Frame::soft(Size::splat(Abs::pt(4.0))), 1.0).unwrap();
        std::fs::write(std::env::temp_dir().join("znote-docx-dot.png"), dot).unwrap();
        let source = "#outline()\n= Section\nText.#footnote[The note.] Inline $x^2$ here.\n$ a/b $\n#figure(image(\"znote-docx-dot.png\", width: 50%, alt: \"Dot\"), caption: [A dot])";
        let mut archive = word(source);
        let body = body(&mut archive);
        assert!(
            body.contains(r#"<w:footnoteReference w:id="1"/>"#),
            "{body}"
        );
        assert!(!body.contains("The note."), "{body}");
        assert!(part(&mut archive, "word/footnotes.xml").contains("The note."));
        assert!(body.contains("<w:r/><m:oMath><m:sSup>"), "{body}");
        assert!(body.contains("<m:oMathPara><m:oMath><m:f>"), "{body}");
        assert!(body.contains(r#"descr="Dot""#), "{body}");
        assert!(body.contains(r#"<w:pStyle w:val="Caption"/>"#), "{body}");
        assert!(
            body.contains(r#" PAGEREF _loc_1 \h "#) || body.contains(" PAGEREF _"),
            "{body}"
        );
        let mut picture = Vec::new();
        archive
            .by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut picture)
            .unwrap();
        assert!(picture.starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_figure_html_cannot_show_is_taken_from_the_page_as_a_picture() {
        let source = "Before.\n#figure(layout(size => rect(width: 2cm, height: 1cm, fill: red)), caption: [Measured])\nAfter.";
        let mut archive = word(source);
        let body = body(&mut archive);
        let picture = body.find("<wp:inline").expect(&body);
        assert!(picture < body.find("Measured").unwrap(), "{body}");
        let mut png = Vec::new();
        archive
            .by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut png)
            .unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // About the size Typst drew it (2 cm wide, 2 pt of room on each side).
        let width = body[picture..]
            .split(r#"cx=""#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .and_then(|cx| cx.parse::<f64>().ok())
            .unwrap();
        assert!((width / 12700.0 - (56.69 + 4.0)).abs() < 1.0, "{width}");
    }

    #[test]
    fn a_heading_typst_moved_to_the_next_page_keeps_its_space_above() {
        // The second heading does not fit with the lines it keeps on its page, so Typst starts a
        // page with it, leaving room on the first; Word may keep it there.
        let source = "#set page(width: 300pt, height: 250pt)\n= First\n#lorem(30)\n\n#lorem(25)\n= Second\n#lorem(60)";
        let document = document(source);
        assert_eq!(
            document.pages().len(),
            2,
            "the case needs Typst to move the heading"
        );
        let mut archive = word(source);
        let body = body(&mut archive);
        let before = |heading: &str| {
            let at = body.find(&format!(">{heading}<")).unwrap();
            let paragraph = &body[body[..at].rfind("<w:p>").unwrap()..at];
            paragraph
                .split(r#"w:before=""#)
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap()
        };
        let paragraph = |text: &str| {
            let at = body.find(text).unwrap();
            body[body[..at].rfind("<w:p>").unwrap()..at].to_string()
        };
        let first_body = paragraph("Lorem ipsum");
        let space = first_body
            .split(r#"w:before=""#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap();
        assert!(before("Second") > space, "{body}");
        assert!(before("Second") > 100, "{body}");
    }

    #[test]
    fn layout_keeps_its_content_and_page_breaks_carry_over() {
        let source = "#align(center)[Centred]\n#grid(columns: 2, [Left cell], [Right cell])\n#pad(x: 1cm)[Padded]\n#pagebreak()\nNext page\n#polygon((0pt, 0pt), (1cm, 0pt), (0pt, 1cm))";
        let (html, _) = html(source);
        let bytes = docx(&document(source), &html, &Equations::new()).unwrap();
        let body = part(
            &mut ZipArchive::new(Cursor::new(bytes)).unwrap(),
            "word/document.xml",
        );
        for text in ["Centred", "Left cell", "Right cell", "Padded", "Next page"] {
            assert!(body.contains(text), "{text}: {body}");
        }
        assert_eq!(
            body.matches(r#"<w:br w:type="page"/>"#).count(),
            1,
            "{body}"
        );
        assert!(
            body.contains("<wp:inline"),
            "the polygon is a picture: {body}"
        );
    }
}
