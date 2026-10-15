//! Word drawings: pictures in the text or at a fixed place on the page, and text boxes; and the
//! pages Word gets as a copy (designed pages, and the header and footer of the others), built from
//! the same fixed layout as a PowerPoint slide.

use std::fmt::Write as _;

use typst::foundations::Smart;
use typst::layout::Rect;
use typst_layout::Page;

use super::Parts;
use typst::layout::{Abs, Frame, FrameItem, Point};
use typst_render::RenderOptions;

use crate::export::layout::{
    Align, BACKGROUND_PIXELS_PER_POINT, Layers, Paragraph, Piece, Run, layers, lines, paragraphs,
    place,
};
use crate::export::math::{self, Equations};
use crate::export::{Media, escape};

pub const PICTURE: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
pub const SHAPE: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const EMU_PER_POINT: f64 = 12700.0;

pub fn emu(points: f64) -> i64 {
    (points * EMU_PER_POINT).round() as i64
}

pub fn twips(points: f64) -> i64 {
    (points * 20.0).round() as i64
}

fn graphic(id: usize, link: &str, cx: i64, cy: i64) -> String {
    format!(
        r#"<a:graphic><a:graphicData uri="{PICTURE}"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="Picture {id}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{link}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>"#
    )
}

fn described(alt: Option<&str>) -> String {
    alt.map(|alt| format!(r#" descr="{}""#, escape(alt)))
        .unwrap_or_default()
}

pub fn inline_picture(id: usize, link: &str, width: f64, height: f64, alt: Option<&str>) -> String {
    let (cx, cy) = (emu(width), emu(height));
    format!(
        r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="Picture {id}"{}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>{}</wp:inline></w:drawing></w:r>"#,
        described(alt),
        graphic(id, link, cx, cy)
    )
}

/// The opening of a drawing pinned to the page at `area`, in front of the text or behind it.
fn anchor(id: usize, area: Rect, behind: bool, name: &str, alt: Option<&str>) -> String {
    format!(
        r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="{}" behindDoc="{}" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx="{}" cy="{}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="{id}" name="{name} {id}"{}/><wp:cNvGraphicFramePr/>"#,
        id * 2 + 2,
        u8::from(behind),
        emu(area.min.x.to_pt()),
        emu(area.min.y.to_pt()),
        emu((area.max.x - area.min.x).to_pt()),
        emu((area.max.y - area.min.y).to_pt()),
        described(alt),
    )
}

pub fn anchored_picture(
    id: usize,
    link: &str,
    area: Rect,
    behind: bool,
    alt: Option<&str>,
) -> String {
    format!(
        r#"<w:r><w:drawing>{}{}</wp:anchor></w:drawing></w:r>"#,
        anchor(id, area, behind, "Picture", alt),
        graphic(
            id,
            link,
            emu((area.max.x - area.min.x).to_pt()),
            emu((area.max.y - area.min.y).to_pt())
        )
    )
}

/// A text box pinned to the page, holding `paragraphs` (`w:p` elements).
pub fn text_box(id: usize, area: Rect, wraps: bool, paragraphs: &str) -> String {
    format!(
        r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing>{}<a:graphic><a:graphicData uri="{SHAPE}"><wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></wps:spPr><wps:txbx><w:txbxContent>{paragraphs}</w:txbxContent></wps:txbx><wps:bodyPr rot="0" vert="horz" wrap="{}" lIns="0" tIns="0" rIns="0" bIns="0" anchor="t" anchorCtr="0" upright="1"><a:noAutofit/></wps:bodyPr></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback/></mc:AlternateContent></w:r>"#,
        anchor(id, area, false, "Text", None),
        emu((area.max.x - area.min.x).to_pt()),
        emu((area.max.y - area.min.y).to_pt()),
        if wraps { "square" } else { "none" },
    )
}

pub fn run_properties(
    family: &str,
    size: f64,
    bold: bool,
    italic: bool,
    colour: [u8; 3],
) -> String {
    let family = escape(family);
    let [red, green, blue] = colour;
    let size = (size * 2.0).round() as i64;
    format!(
        r#"<w:rFonts w:ascii="{family}" w:hAnsi="{family}" w:eastAsia="{family}" w:cs="{family}"/>{}{}<w:color w:val="{red:02X}{green:02X}{blue:02X}"/><w:sz w:val="{size}"/><w:szCs w:val="{size}"/>"#,
        if bold { "<w:b/>" } else { "" },
        if italic { "<w:i/>" } else { "" },
    )
}

fn properties(run: &Run) -> String {
    let [red, green, blue, _] = run.colour;
    format!(
        "<w:rPr>{}</w:rPr>",
        run_properties(
            &run.family,
            run.size,
            run.bold,
            run.italic,
            [red, green, blue]
        )
    )
}

/// What a number in a header or footer stands for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Page,
    SectionPages,
    Pages,
}

/// Text split where digits start and stop, so a page number can be told from what is around it.
pub fn segments(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut digits = None;
    for (at, character) in text.char_indices() {
        let digit = character.is_ascii_digit();
        if digits.is_some_and(|before| before != digit) {
            parts.push(&text[start..at]);
            start = at;
        }
        digits = Some(digit);
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

pub fn is_number(segment: &str) -> bool {
    !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit())
}

/// A text box paragraph's text, one entry per number in it.
pub fn numbers(paragraph: &Paragraph) -> Vec<String> {
    place(paragraph)
        .pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Text(_, text) => Some(text.as_str()),
            Piece::Break => None,
        })
        .flat_map(segments)
        .filter(|segment| is_number(segment))
        .map(str::to_string)
        .collect()
}

/// The paragraph of a text box: `fields` says, per number in its text, which field it becomes.
fn paragraph(paragraph: &Paragraph, fields: &[Option<Field>]) -> String {
    let placed = place(paragraph);
    let align = match paragraph.align {
        Align::Left => "left",
        Align::Centre => "center",
        Align::Right => "right",
    };
    let line = if placed.wrapped {
        format!(r#" w:line="{}" w:lineRule="exact""#, twips(paragraph.pitch))
    } else {
        String::new()
    };
    let mut xml = format!(
        r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0"{line}/><w:jc w:val="{align}"/></w:pPr>"#
    );
    let mut number = 0;
    for piece in &placed.pieces {
        let (run, text) = match piece {
            Piece::Break => {
                xml.push_str("<w:r><w:br/></w:r>");
                continue;
            }
            Piece::Text(run, text) => (run, text),
        };
        let properties = properties(run);
        for segment in segments(text) {
            let field = if is_number(segment) {
                number += 1;
                fields.get(number - 1).copied().flatten()
            } else {
                None
            };
            let shown = format!(
                r#"<w:r>{properties}<w:t xml:space="preserve">{}</w:t></w:r>"#,
                escape(segment)
            );
            match field {
                Some(field) => {
                    let instruction = match field {
                        Field::Page => "PAGE",
                        Field::SectionPages => "SECTIONPAGES",
                        Field::Pages => "NUMPAGES",
                    };
                    let _ = write!(
                        xml,
                        r#"<w:fldSimple w:instr=" {instruction} ">{shown}</w:fldSimple>"#
                    );
                }
                None => xml.push_str(&shown),
            }
        }
    }
    xml.push_str("</w:p>");
    xml
}

/// A page as Word copies it: anchors for its picture, images, text boxes and equations, to go
/// into one paragraph. `fields` gives, per text box, which of its numbers are fields.
pub fn copy(
    page: &Page,
    equations: &Equations,
    fields: &[Vec<Option<Field>>],
    parts: &mut Parts,
    links: &mut Vec<(&'static str, String)>,
) -> Result<String, String> {
    let (mut background, layers) = layers(page, equations)?;
    copy_layers(&mut background, layers, fields, parts, links)
}

pub fn copy_layers(
    background: &mut Page,
    layers: Layers,
    fields: &[Vec<Option<Field>>],
    parts: &mut Parts,
    links: &mut Vec<(&'static str, String)>,
) -> Result<String, String> {
    let mut xml = String::new();
    // A page that sets no fill is white in Word anyway; leave it transparent so nothing hides.
    if background.fill == Smart::Auto {
        background.fill = Smart::Custom(None);
    }
    let size = background.frame.size();
    let full = Rect::new(Point::zero(), size.to_point());
    if !blank(background) {
        let options = RenderOptions {
            pixel_per_pt: BACKGROUND_PIXELS_PER_POINT.into(),
            ..Default::default()
        };
        let bytes = typst_render::render(background, &options)
            .encode_png()
            .map_err(|error| format!("A picture could not be drawn: {error}"))?;
        let link = parts.picture(
            links,
            Media {
                bytes,
                extension: "png",
            },
        );
        let id = parts.id();
        xml.push_str(&anchored_picture(id, &link, full, true, None));
    }
    for picture in layers.pictures {
        let link = parts.picture(links, picture.media);
        let id = parts.id();
        xml.push_str(&anchored_picture(
            id,
            &link,
            picture.rect,
            false,
            picture.alt.as_deref(),
        ));
    }
    let empty = Vec::new();
    for (index, paragraph) in paragraphs(lines(layers.runs)).iter().enumerate() {
        for run in paragraph.lines.iter().flatten() {
            parts.font(&run.family, run.bold, run.italic, &run.font);
        }
        let placed = place(paragraph);
        // A line that just fits in Typst may not in Word (another font, other measurements): a
        // single line gets room to grow away from where it is aligned.
        let room = if placed.wrapped {
            0.0
        } else {
            placed.width * 0.5
        };
        let x = match paragraph.align {
            Align::Left => placed.x,
            Align::Centre => placed.x - room / 2.0,
            Align::Right => placed.x - room,
        };
        let area = Rect::new(
            Point::new(Abs::pt(x), Abs::pt(placed.top)),
            Point::new(
                Abs::pt(x + placed.width + room),
                Abs::pt(placed.top + placed.height),
            ),
        );
        let id = parts.id();
        xml.push_str(&text_box(
            id,
            area,
            placed.wrapped,
            &self::paragraph(paragraph, fields.get(index).unwrap_or(&empty)),
        ));
    }
    for formula in layers.formulas {
        let [red, green, blue, _] = formula.colour;
        let run = format!(
            r#"<w:rPr><w:rFonts w:ascii="Cambria Math" w:hAnsi="Cambria Math"/><w:color w:val="{red:02X}{green:02X}{blue:02X}"/><w:sz w:val="{0}"/><w:szCs w:val="{0}"/></w:rPr>"#,
            (formula.size * 2.0).round() as i64
        );
        let id = parts.id();
        xml.push_str(&text_box(
            id,
            formula.rect,
            false,
            &format!(
                r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0"/><w:jc w:val="center"/></w:pPr><m:oMathPara>{}</m:oMathPara></w:p>"#,
                math::omml(&formula.math, &run)
            ),
        ));
    }
    Ok(xml)
}

fn blank(page: &Page) -> bool {
    page.fill.clone().unwrap_or(None).is_none() && !paints(&page.frame)
}

fn paints(frame: &Frame) -> bool {
    frame.items().any(|(_, item)| match item {
        FrameItem::Group(group) => paints(&group.frame),
        FrameItem::Tag(_) | FrameItem::Link(..) => false,
        _ => true,
    })
}
