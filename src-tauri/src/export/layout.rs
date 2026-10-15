//! Fixed layout: a laid-out page as a picture of everything that is not text, images or formulas,
//! with the text as boxes at its own place. PowerPoint uses it for every slide; Word for the pages
//! that are designed rather than written, and for the header and footer.

use typst::introspection::{Location, Tag};
use typst::layout::{Abs, Frame, FrameItem, GroupItem, Point, Rect, Size};
use typst::text::{FontFlags, FontInstance, TextItem};
use typst::visualize::Paint;
use typst_html::HtmlElement;
use typst_layout::Page;

use super::math::{self, Equations};
use super::{Media, face, media, png};

pub(super) const BACKGROUND_PIXELS_PER_POINT: f64 = 2.0;
const WRAP_SLACK: f64 = 1.03;

#[derive(Clone)]
pub(super) struct Run {
    pub(super) x: f64,
    pub(super) baseline: f64,
    pub(super) width: f64,
    pub(super) ascent: f64,
    pub(super) descent: f64,
    pub(super) size: f64,
    pub(super) family: String,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) colour: [u8; 4],
    pub(super) text: String,
    pub(super) first_word: f64,
    pub(super) trailing: f64,
    pub(super) element: Option<Location>,
    pub(super) font: FontInstance,
}

pub(super) fn run(text: &TextItem, at: Point, element: Option<Location>) -> Option<Run> {
    let font = text.font.font();
    let info = font.info();
    let face = face(&text.font);
    let Paint::Solid(colour) = &text.fill else {
        return None;
    };
    if text.stroke.is_some() || info.flags.contains(FontFlags::MATH) {
        return None;
    }
    let metrics = text.font.metrics();
    let advance = |bytes: std::ops::Range<usize>| -> f64 {
        let inside = text
            .glyphs
            .iter()
            .filter(|glyph| bytes.contains(&glyph.range().start));
        inside
            .map(|glyph| glyph.x_advance.at(text.size).to_pt())
            .sum()
    };
    let content = text.text.as_str();
    let lead = content.len() - content.trim_start().len();
    let word_end = content[lead..]
        .find(char::is_whitespace)
        .map_or(content.len(), |index| lead + index);
    Some(Run {
        x: at.x.to_pt(),
        baseline: at.y.to_pt(),
        width: text.width().to_pt(),
        ascent: metrics.ascender.at(text.size).to_pt(),
        descent: -metrics.descender.at(text.size).to_pt(),
        size: text.size.to_pt(),
        family: face.family,
        bold: face.bold,
        italic: face.italic,
        colour: colour.to_vec4_u8(),
        text: text.text.to_string(),
        first_word: advance(0..word_end),
        trailing: advance(content.trim_end().len()..content.len()),
        element,
        font: text.font.clone(),
    })
}

pub(super) fn inside(run: &Run, clip: Option<Rect>) -> bool {
    let slack = Abs::pt(1.0);
    clip.is_none_or(|clip| {
        Abs::pt(run.x) >= clip.min.x - slack
            && Abs::pt(run.x + run.width) <= clip.max.x + slack
            && Abs::pt(run.baseline - run.ascent) >= clip.min.y - slack
            && Abs::pt(run.baseline + run.descent) <= clip.max.y + slack
    })
}

pub(super) fn narrowed(clip: Option<Rect>, group: &GroupItem, origin: Point) -> Option<Rect> {
    let Some(curve) = &group.clip else {
        return clip;
    };
    let own = curve.bbox(None);
    let (min, max) = (own.min + origin, own.max + origin);
    Some(match clip {
        Some(outer) => Rect::new(min.max(outer.min), max.min(outer.max)),
        None => Rect::new(min, max),
    })
}

/// What a page splits into besides its background picture.
#[derive(Default)]
pub(super) struct Layers {
    pub(super) runs: Vec<Run>,
    pub(super) pictures: Vec<Picture>,
    pub(super) formulas: Vec<Formula>,
    pub(super) images: usize,
}

pub(super) struct Picture {
    pub(super) rect: Rect,
    pub(super) media: Media,
    pub(super) alt: Option<String>,
}

pub(super) struct Formula {
    pub(super) rect: Rect,
    pub(super) size: f64,
    pub(super) colour: [u8; 4],
    pub(super) math: HtmlElement,
    pub(super) picture: Vec<u8>,
}

/// Frame items at their place on the page.
pub(super) type Items = Vec<(Point, FrameItem)>;

pub(super) struct Lift<'a> {
    pub(super) equations: &'a Equations,
    /// Per image in paint order: whether nothing painted later overlaps it, so it can leave the
    /// background without ending up on top of something drawn over it.
    pub(super) images: Vec<bool>,
}

pub(super) fn rect(at: Point, size: Size) -> Rect {
    Rect::new(at, at + size.to_point())
}

pub(super) fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(a.min.min(b.min), a.max.max(b.max))
}

pub(super) fn overlaps(a: &Rect, b: &Rect) -> bool {
    let slack = Abs::pt(0.5);
    a.min.x < b.max.x - slack
        && b.min.x < a.max.x - slack
        && a.min.y < b.max.y - slack
        && b.min.y < a.max.y - slack
}

pub(super) fn within(rect: Rect, clip: Option<Rect>) -> bool {
    let slack = Abs::pt(1.0);
    clip.is_none_or(|clip| {
        rect.min.x >= clip.min.x - slack
            && rect.max.x <= clip.max.x + slack
            && rect.min.y >= clip.min.y - slack
            && rect.max.y <= clip.max.y + slack
    })
}

/// Where an item paints on the page, if it paints at all.
pub(super) fn bounds(item: &FrameItem, at: Point) -> Option<Rect> {
    match item {
        FrameItem::Group(group) => {
            let size = group.frame.size();
            [
                Point::zero(),
                Point::with_x(size.x),
                Point::with_y(size.y),
                size.to_point(),
            ]
            .map(|corner| rect(corner.transform(group.transform) + at, Size::zero()))
            .into_iter()
            .reduce(union)
        }
        FrameItem::Text(text) => {
            let metrics = text.font.metrics();
            Some(Rect::new(
                at - Point::with_y(metrics.ascender.at(text.size)),
                at + Point::new(text.width(), -metrics.descender.at(text.size)),
            ))
        }
        FrameItem::Shape(shape, _) => {
            let own = shape.bbox(true);
            Some(Rect::new(own.min + at, own.max + at))
        }
        FrameItem::Image(_, size, _) => Some(rect(at, *size)),
        _ => None,
    }
}

/// Everything that stays painted in the background, in paint order, with `true` for images.
/// Walks the frame the way `split` does, so the images come in the same order.
pub(super) fn painted(frame: &Frame, origin: Point, marks: &mut Vec<(Rect, bool)>) {
    for (position, item) in frame.items() {
        let at = origin + *position;
        match item {
            FrameItem::Group(group) if group.transform.is_only_translate() => painted(
                &group.frame,
                at + Point::new(group.transform.tx, group.transform.ty),
                marks,
            ),
            FrameItem::Text(text) if run(text, at, None).is_some() => {}
            _ => marks.extend(
                bounds(item, at).map(|bounds| (bounds, matches!(item, FrameItem::Image(..)))),
            ),
        }
    }
}

pub(super) fn liftable(marks: &[(Rect, bool)]) -> Vec<bool> {
    marks
        .iter()
        .enumerate()
        .filter(|(_, (_, image))| *image)
        .map(|(index, (own, _))| {
            !marks[index + 1..]
                .iter()
                .any(|(later, _)| overlaps(own, later))
        })
        .collect()
}

pub(super) fn first_text(items: &[(Point, FrameItem)]) -> Option<TextItem> {
    items.iter().find_map(|(_, item)| match item {
        FrameItem::Text(text) => Some(text.clone()),
        FrameItem::Group(group) => first_text(
            &group
                .frame
                .items()
                .map(|(at, item)| (*at, item.clone()))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    })
}

/// A formula from the items between its equation's start and end tags.
pub(super) fn formula(
    items: &[(Point, FrameItem)],
    math: &HtmlElement,
) -> Result<Option<Formula>, String> {
    let Some(area) = items
        .iter()
        .filter_map(|(at, item)| bounds(item, *at))
        .reduce(union)
    else {
        return Ok(None);
    };
    let Some(text) = first_text(items) else {
        return Ok(None);
    };
    let Paint::Solid(colour) = &text.fill else {
        return Ok(None);
    };
    let mut frame = Frame::soft(Size::new(area.max.x - area.min.x, area.max.y - area.min.y));
    for (at, item) in items {
        frame.push(*at - area.min, item.clone());
    }
    Ok(Some(Formula {
        rect: area,
        size: text.size.to_pt(),
        colour: colour.to_vec4_u8(),
        math: math.clone(),
        picture: png(&frame, BACKGROUND_PIXELS_PER_POINT)?,
    }))
}

pub(super) fn split(
    frame: &Frame,
    origin: Point,
    clip: Option<Rect>,
    open: &mut Vec<Location>,
    layers: &mut Layers,
    lift: &Lift,
) -> Result<Frame, String> {
    let mut rest = Frame::new(frame.size(), frame.kind());
    // The equation being collected: its end tag, its MathML and its items so far.
    let mut capture: Option<(Location, &HtmlElement, Items)> = None;
    for (position, item) in frame.items() {
        let at = origin + *position;
        if let Some((location, math, items)) = &mut capture {
            if !matches!(item, FrameItem::Tag(Tag::End(end, ..)) if *end == *location) {
                items.push((at, item.clone()));
                continue;
            }
            match formula(items, math)? {
                Some(formula) => layers.formulas.push(formula),
                None => items
                    .drain(..)
                    .for_each(|(at, item)| rest.push(at - origin, item)),
            }
            capture = None;
        }
        match item {
            FrameItem::Group(group) if group.transform.is_only_translate() => {
                let inner = at + Point::new(group.transform.tx, group.transform.ty);
                let mut group = group.clone();
                group.frame = split(
                    &group.frame,
                    inner,
                    narrowed(clip, &group, inner),
                    open,
                    layers,
                    lift,
                )?;
                rest.push(*position, FrameItem::Group(group));
            }
            FrameItem::Text(text) => {
                match run(text, at, open.last().copied()).filter(|run| inside(run, clip)) {
                    Some(run) => layers.runs.push(run),
                    None => rest.push(*position, item.clone()),
                }
            }
            FrameItem::Image(image, size, _) => {
                let index = layers.images;
                layers.images += 1;
                let area = rect(at, *size);
                if lift.images.get(index) == Some(&true) && within(area, clip) {
                    layers.pictures.push(Picture {
                        rect: area,
                        media: media(image, *size)?,
                        alt: image.alt().map(str::to_string),
                    });
                } else {
                    rest.push(*position, item.clone());
                }
            }
            FrameItem::Tag(tag) => {
                match tag {
                    Tag::Start(content, _) => {
                        if let Some(math) =
                            math::key(content).and_then(|key| lift.equations.get(&key))
                            && let Some(location) = content.location()
                        {
                            capture = Some((location, math, Vec::new()));
                        }
                        open.extend(content.location())
                    }
                    Tag::End(location, ..) => {
                        if let Some(index) = open.iter().rposition(|opened| opened == location) {
                            open.remove(index);
                        }
                    }
                }
                rest.push(*position, item.clone());
            }
            _ => rest.push(*position, item.clone()),
        }
    }
    // An equation that does not end in this frame stays in the picture.
    for (at, item) in capture.map(|(_, _, items)| items).unwrap_or_default() {
        rest.push(at - origin, item);
    }
    Ok(rest)
}

pub(super) fn row(run: &Run) -> i64 {
    (run.baseline * 2.0).round() as i64
}

pub(super) fn continues(line: &[Run], run: &Run) -> bool {
    line.last().is_some_and(|last| {
        let gap = run.x - (last.x + last.width);
        row(last) == row(run) && gap > -1.0 && gap < 0.3 * run.size
    })
}

pub(super) fn lines(mut runs: Vec<Run>) -> Vec<Vec<Run>> {
    runs.sort_by(|a, b| row(a).cmp(&row(b)).then(a.x.total_cmp(&b.x)));
    let mut lines: Vec<Vec<Run>> = Vec::new();
    for run in runs {
        match lines.last_mut() {
            Some(line) if continues(line, &run) => line.push(run),
            _ => lines.push(vec![run]),
        }
    }
    lines.retain(|line| line.iter().any(|run| !run.text.trim().is_empty()));
    lines
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Align {
    Left,
    Centre,
    Right,
}

pub(super) struct Paragraph {
    pub(super) lines: Vec<Vec<Run>>,
    pub(super) align: Align,
    pub(super) pitch: f64,
}

pub(super) fn span(line: &[Run]) -> (f64, f64) {
    let right = line
        .iter()
        .map(|run| run.x + run.width - run.trailing)
        .fold(line[0].x, f64::max);
    (line[0].x, right)
}

pub(super) fn size(line: &[Run]) -> f64 {
    line.iter().map(|run| run.size).fold(0.0, f64::max)
}

pub(super) fn alignment(above: &[Run], below: &[Run]) -> Option<Align> {
    let ((left, right), (next_left, next_right)) = (span(above), span(below));
    if (left - next_left).abs() <= 1.0 {
        Some(Align::Left)
    } else if (left + right - next_left - next_right).abs() <= 2.0 {
        Some(Align::Centre)
    } else if (right - next_right).abs() <= 1.0 {
        Some(Align::Right)
    } else {
        None
    }
}

pub(super) fn joins(paragraph: &Paragraph, line: &[Run]) -> Option<(Align, f64)> {
    let last = paragraph.lines.last()?;
    let pitch = line[0].baseline - last[0].baseline;
    let align = alignment(last, line)?;
    let same_text = last[0].element == line[0].element
        && last[0].family == line[0].family
        && (size(last) - size(line)).abs() < 0.01;
    let follows = if paragraph.lines.len() == 1 {
        pitch > 0.0 && pitch <= 2.5 * size(line)
    } else {
        align == paragraph.align && (pitch - paragraph.pitch).abs() <= 0.5
    };
    (same_text && follows).then_some((align, pitch))
}

pub(super) fn paragraphs(lines: Vec<Vec<Run>>) -> Vec<Paragraph> {
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    for line in lines {
        let joined = paragraphs
            .iter_mut()
            .rev()
            .find_map(|paragraph| Some((joins(paragraph, &line)?, paragraph)));
        match joined {
            Some(((align, pitch), paragraph)) => {
                paragraph.align = align;
                paragraph.pitch = pitch;
                paragraph.lines.push(line);
            }
            None => paragraphs.push(Paragraph {
                lines: vec![line],
                align: Align::Left,
                pitch: 0.0,
            }),
        }
    }
    paragraphs
}

pub(super) fn layers(page: &Page, equations: &Equations) -> Result<(Page, Layers), String> {
    let mut marks = Vec::new();
    painted(&page.frame, Point::zero(), &mut marks);
    let lift = Lift {
        equations,
        images: liftable(&marks),
    };
    let mut layers = Layers::default();
    let mut background = page.clone();
    background.frame = split(
        &page.frame,
        Point::zero(),
        None,
        &mut Vec::new(),
        &mut layers,
        &lift,
    )?;
    Ok((background, layers))
}

/// One piece of a text box's paragraph: a run's text, or a line break Typst forced.
pub(super) enum Piece<'a> {
    Break,
    Text(&'a Run, String),
}

/// Where a paragraph's text box goes and what it holds.
pub(super) struct Placed<'a> {
    pub(super) x: f64,
    pub(super) top: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    /// Several lines in a box that wraps, rather than one line in a box that does not.
    pub(super) wrapped: bool,
    pub(super) pieces: Vec<Piece<'a>>,
}

pub(super) fn place(paragraph: &Paragraph) -> Placed<'_> {
    let lines = &paragraph.lines;
    let first = &lines[0];
    let wrapped = lines.len() > 1;
    let ascent = first.iter().map(|run| run.ascent).fold(0.0, f64::max);
    let descent = first.iter().map(|run| run.descent).fold(0.0, f64::max);
    let left = lines
        .iter()
        .map(|line| span(line).0)
        .fold(f64::INFINITY, f64::min);
    let right = lines.iter().map(|line| span(line).1).fold(left, f64::max);
    let width = if wrapped {
        (right - left) * WRAP_SLACK
    } else {
        right - left
    };
    let x = match paragraph.align {
        Align::Left => left,
        Align::Centre => (left + right - width) / 2.0,
        Align::Right => right - width,
    };
    let (top, height) = if wrapped {
        (
            first[0].baseline + descent - paragraph.pitch,
            paragraph.pitch * lines.len() as f64,
        )
    } else {
        (first[0].baseline - ascent, ascent + descent)
    };
    let mut pieces = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut glue = "";
        if let Some(above) = index.checked_sub(1).map(|above| &lines[above]) {
            let (above_left, above_right) = span(above);
            let word_space = 0.25 * line[0].size;
            let open_ended = above.last().is_some_and(|run| {
                !run.text
                    .ends_with(|last: char| last.is_whitespace() || last == '-')
            });
            if above_right - above_left + word_space + line[0].first_word <= width {
                pieces.push(Piece::Break);
            } else if open_ended {
                glue = " ";
            }
        }
        for (position, run) in line.iter().enumerate() {
            let text = if position == 0 {
                format!("{glue}{}", run.text)
            } else {
                run.text.clone()
            };
            pieces.push(Piece::Text(run, text));
        }
    }
    Placed {
        x,
        top,
        width,
        height,
        wrapped,
        pieces,
    }
}
