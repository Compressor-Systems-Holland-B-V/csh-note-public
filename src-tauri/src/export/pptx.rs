//! PowerPoint: one slide per page. The page minus its text, images and formulas is a background
//! picture; text becomes text boxes, images become pictures and formulas become equations.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use typst::layout::{Point, Rect};
use typst::text::FontInstance;
use typst_layout::{Page, PagedDocument};
use typst_render::RenderOptions;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::layout::{
    Align, BACKGROUND_PIXELS_PER_POINT, Formula, Paragraph, Piece, Run, layers, lines, paragraphs,
    place, rect,
};
use super::math::{self, Equations};
use super::{
    Media, NAME_FAMILY, NAME_FULL, NAME_STYLE, NAME_VERSION, SELECTION_ITALIC, XML,
    embeddable_true_type, escape, name, relationships, short, table,
};

const EMU_PER_POINT: f64 = 12700.0;
const SLIDE_EMU: std::ops::RangeInclusive<i64> = 914_400..=51_206_400;

const NAMESPACES: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const PRESENTATION: &str = "application/vnd.openxmlformats-officedocument.presentationml";
const EMPTY_TREE: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
const COLOUR_MAP: &str = r#"<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"#;

fn embeddable(instance: &FontInstance) -> Option<Vec<u8>> {
    let font = instance.font();
    let data: &[u8] = font.data();
    if !embeddable_true_type(font) {
        return None;
    }
    let os2 = table(font, b"OS/2").filter(|os2| os2.len() >= 78)?;
    let adjustment = table(font, b"head")?.get(8..12)?;
    let permissions = short(os2, 8)?;

    let swapped = |bytes: &[u8]| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|long| long.iter().rev().copied())
            .collect::<Vec<u8>>()
    };
    let mut eot = Vec::with_capacity(data.len() + 256);
    eot.extend([0; 4]);
    eot.extend((data.len() as u32).to_le_bytes());
    eot.extend(0x0002_0001_u32.to_le_bytes());
    eot.extend(0_u32.to_le_bytes());
    eot.extend(&os2[32..42]);
    eot.push(1);
    eot.push(u8::from(short(os2, 62)? & SELECTION_ITALIC != 0));
    eot.extend(u32::from(short(os2, 4)?).to_le_bytes());
    eot.extend(permissions.to_le_bytes());
    eot.extend(0x504C_u16.to_le_bytes());
    eot.extend(swapped(&os2[42..58]));
    eot.extend(swapped(os2.get(78..86).unwrap_or(&[0; 8])));
    eot.extend(swapped(adjustment));
    eot.extend([0; 16]);
    for id in [NAME_FAMILY, NAME_STYLE, NAME_VERSION, NAME_FULL] {
        let text: Vec<u8> = name(instance, id)
            .unwrap_or_default()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        eot.extend([0; 2]);
        eot.extend((text.len() as u16).to_le_bytes());
        eot.extend(text);
    }
    eot.extend([0; 4]);
    eot.extend(data);
    let size = (eot.len() as u32).to_le_bytes();
    eot[..4].copy_from_slice(&size);
    Some(eot)
}

fn emu(points: f64) -> i64 {
    (points * EMU_PER_POINT).round() as i64
}

fn text_run(run: &Run, text: &str) -> String {
    let [red, green, blue, alpha] = run.colour;
    let family = escape(&run.family);
    format!(
        r#"<a:r><a:rPr sz="{}" b="{}" i="{}" dirty="0"><a:solidFill><a:srgbClr val="{red:02X}{green:02X}{blue:02X}"><a:alpha val="{}"/></a:srgbClr></a:solidFill><a:latin typeface="{family}"/><a:ea typeface="{family}"/><a:cs typeface="{family}"/></a:rPr><a:t>{}</a:t></a:r>"#,
        (run.size * 100.0).round() as i64,
        u8::from(run.bold),
        u8::from(run.italic),
        (f64::from(alpha) / 255.0 * 100_000.0).round() as i64,
        escape(text),
    )
}

fn text_box(id: usize, paragraph: &Paragraph) -> String {
    let placed = place(paragraph);
    let (wrap, spacing) = if placed.wrapped {
        (
            "square",
            format!(
                r#"<a:spcPts val="{}"/>"#,
                (paragraph.pitch * 100.0).round() as i64
            ),
        )
    } else {
        ("none", r#"<a:spcPct val="100000"/>"#.to_string())
    };
    let align = match paragraph.align {
        Align::Left => "l",
        Align::Centre => "ctr",
        Align::Right => "r",
    };
    let mut xml = format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="{wrap}" lIns="0" tIns="0" rIns="0" bIns="0" rtlCol="0" anchor="t"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p><a:pPr marL="0" indent="0" algn="{align}"><a:lnSpc>{spacing}</a:lnSpc><a:spcBef><a:spcPts val="0"/></a:spcBef><a:buNone/></a:pPr>"#,
        emu(placed.x),
        emu(placed.top),
        emu(placed.width),
        emu(placed.height),
    );
    for piece in &placed.pieces {
        match piece {
            Piece::Break => xml.push_str("<a:br/>"),
            Piece::Text(run, text) => xml.push_str(&text_run(run, text)),
        }
    }
    xml.push_str("</a:p></p:txBody></p:sp>");
    xml
}

fn placed(area: Rect) -> String {
    format!(
        r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        emu(area.min.x.to_pt()),
        emu(area.min.y.to_pt()),
        emu((area.max.x - area.min.x).to_pt()),
        emu((area.max.y - area.min.y).to_pt()),
    )
}

fn picture(id: usize, name: &str, link: usize, area: Rect, alt: Option<&str>) -> String {
    let description = alt
        .map(|alt| format!(r#" descr="{}""#, escape(alt)))
        .unwrap_or_default();
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name} {id}"{description}/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId{link}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
        placed(area)
    )
}

/// A native equation, with the formula's picture for programs that cannot show one.
fn equation(id: usize, link: usize, formula: &Formula) -> String {
    let [red, green, blue, _] = formula.colour;
    let run = format!(
        r#"<a:rPr lang="en-US" sz="{}" dirty="0"><a:solidFill><a:srgbClr val="{red:02X}{green:02X}{blue:02X}"/></a:solidFill><a:latin typeface="Cambria Math"/></a:rPr>"#,
        (formula.size * 100.0).round() as i64
    );
    let shape = |content: &str, fill: &str| {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Equation {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}</p:spPr><p:txBody><a:bodyPr wrap="none" lIns="0" tIns="0" rIns="0" bIns="0" anchor="ctr"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="ctr"/>{content}</a:p></p:txBody></p:sp>"#,
            placed(formula.rect)
        )
    };
    format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:a14="http://schemas.microsoft.com/office/drawing/2010/main" Requires="a14">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
        shape(
            &format!(
                r#"<a14:m><m:oMathPara xmlns:m="{}"><m:oMathParaPr><m:jc m:val="centerGroup"/></m:oMathParaPr>{}</m:oMathPara></a14:m>"#,
                math::NAMESPACE,
                math::omml(&formula.math, &run)
            ),
            "<a:noFill/>"
        ),
        shape(
            "",
            &format!(
                r#"<a:blipFill><a:blip r:embed="rId{link}"/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"#
            )
        ),
    )
}

struct Slide {
    xml: String,
    background: Vec<u8>,
    /// Linked from the slide as `rId3` onwards.
    media: Vec<Media>,
    fonts: BTreeMap<(String, bool, bool), FontInstance>,
}

fn slide(page: &Page, equations: &Equations) -> Result<Slide, String> {
    let (background, layers) = layers(page, equations)?;
    let options = RenderOptions {
        pixel_per_pt: BACKGROUND_PIXELS_PER_POINT.into(),
        ..Default::default()
    };
    let picture_of_page = typst_render::render(&background, &options)
        .encode_png()
        .map_err(|error| format!("A slide could not be drawn: {error}"))?;
    let paragraphs = paragraphs(lines(layers.runs));
    let fonts = paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.lines.iter().flatten())
        .map(|run| ((run.family.clone(), run.bold, run.italic), run.font.clone()))
        .collect();

    let mut xml = format!(r#"{XML}<p:sld {NAMESPACES}><p:cSld><p:spTree>{EMPTY_TREE}"#);
    xml.push_str(&picture(
        2,
        "Background",
        2,
        rect(Point::zero(), page.frame.size()),
        None,
    ));
    let mut media = Vec::new();
    let mut id = 3;
    for lifted in layers.pictures {
        xml.push_str(&picture(
            id,
            "Picture",
            3 + media.len(),
            lifted.rect,
            lifted.alt.as_deref(),
        ));
        media.push(lifted.media);
        id += 1;
    }
    for paragraph in &paragraphs {
        xml.push_str(&text_box(id, paragraph));
        id += 1;
    }
    for formula in layers.formulas {
        xml.push_str(&equation(id, 3 + media.len(), &formula));
        media.push(Media {
            bytes: formula.picture,
            extension: "png",
        });
        id += 1;
    }
    let _ = write!(xml, "</p:spTree></p:cSld>{COLOUR_MAP}</p:sld>");
    Ok(Slide {
        xml,
        background: picture_of_page,
        media,
        fonts,
    })
}

fn content_types(slides: usize) -> String {
    let mut xml = format!(
        r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Default Extension="fntdata" ContentType="application/x-fontdata"/><Override PartName="/ppt/presentation.xml" ContentType="{PRESENTATION}.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="{PRESENTATION}.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="{PRESENTATION}.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#
    );
    for number in 1..=slides {
        let _ = write!(
            xml,
            r#"<Override PartName="/ppt/slides/slide{number}.xml" ContentType="{PRESENTATION}.slide+xml"/>"#
        );
    }
    xml.push_str("</Types>");
    xml
}

type Embedded = BTreeMap<String, [Option<usize>; 4]>;

fn presentation(slides: usize, width: i64, height: i64, embedded: &Embedded) -> String {
    let mut xml = format!(
        r#"{XML}<p:presentation {NAMESPACES} embedTrueTypeFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>"#
    );
    for index in 0..slides {
        let _ = write!(
            xml,
            r#"<p:sldId id="{}" r:id="rId{}"/>"#,
            256 + index,
            3 + index
        );
    }
    let _ = write!(
        xml,
        r#"</p:sldIdLst><p:sldSz cx="{width}" cy="{height}"/><p:notesSz cx="6858000" cy="9144000"/>"#
    );
    if !embedded.is_empty() {
        xml.push_str("<p:embeddedFontLst>");
        for (family, styles) in embedded {
            let _ = write!(
                xml,
                r#"<p:embeddedFont><p:font typeface="{}"/>"#,
                escape(family)
            );
            for (style, link) in ["regular", "bold", "italic", "boldItalic"]
                .iter()
                .zip(styles)
            {
                if let Some(link) = link {
                    let _ = write!(xml, r#"<p:{style} r:id="rId{link}"/>"#);
                }
            }
            xml.push_str("</p:embeddedFont>");
        }
        xml.push_str("</p:embeddedFontLst>");
    }
    xml.push_str("</p:presentation>");
    xml
}

fn master() -> String {
    format!(
        r#"{XML}<p:sldMaster {NAMESPACES}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
    )
}

fn layout() -> String {
    format!(
        r#"{XML}<p:sldLayout {NAMESPACES} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{EMPTY_TREE}</p:spTree></p:cSld>{COLOUR_MAP}</p:sldLayout>"#
    )
}

fn office_theme() -> String {
    let colours: String = [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "404040"),
        ("lt2", "E6E6E6"),
        ("accent1", "595959"),
        ("accent2", "737373"),
        ("accent3", "8C8C8C"),
        ("accent4", "A6A6A6"),
        ("accent5", "BFBFBF"),
        ("accent6", "D9D9D9"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ]
    .iter()
    .map(|(name, value)| format!(r#"<a:{name}><a:srgbClr val="{value}"/></a:{name}>"#))
    .collect();
    let font = r#"<a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/>"#;
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#.repeat(3);
    let line =
        r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#.repeat(3);
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3);
    format!(
        r#"{XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="znote"><a:themeElements><a:clrScheme name="znote">{colours}</a:clrScheme><a:fontScheme name="znote"><a:majorFont>{font}</a:majorFont><a:minorFont>{font}</a:minorFont></a:fontScheme><a:fmtScheme name="znote"><a:fillStyleLst>{fill}</a:fillStyleLst><a:lnStyleLst>{line}</a:lnStyleLst><a:effectStyleLst>{effect}</a:effectStyleLst><a:bgFillStyleLst>{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

pub fn pptx(document: &PagedDocument, equations: &Equations) -> Result<Vec<u8>, String> {
    let failed =
        |error: zip::result::ZipError| format!("The presentation could not be made: {error}");
    let pages = document.pages();
    let first = pages.first().ok_or("This page has nothing to export.")?;
    let side = |points: f64| emu(points).clamp(*SLIDE_EMU.start(), *SLIDE_EMU.end());

    let mut parts = vec![
        (
            "[Content_Types].xml".to_string(),
            content_types(pages.len()),
        ),
        (
            "_rels/.rels".into(),
            relationships(&[("officeDocument", "ppt/presentation.xml".into())]),
        ),
        ("ppt/slideMasters/slideMaster1.xml".into(), master()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
            relationships(&[
                ("slideLayout", "../slideLayouts/slideLayout1.xml".into()),
                ("theme", "../theme/theme1.xml".into()),
            ]),
        ),
        ("ppt/slideLayouts/slideLayout1.xml".into(), layout()),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),
            relationships(&[("slideMaster", "../slideMasters/slideMaster1.xml".into())]),
        ),
        ("ppt/theme/theme1.xml".into(), office_theme()),
    ];
    let mut links = vec![
        ("slideMaster", "slideMasters/slideMaster1.xml".to_string()),
        ("theme", "theme/theme1.xml".into()),
    ];
    let mut fonts = BTreeMap::new();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (index, page) in pages.iter().enumerate() {
        let number = index + 1;
        let slide = slide(page, equations)?;
        fonts.extend(slide.fonts);
        links.push(("slide", format!("slides/slide{number}.xml")));
        parts.push((format!("ppt/slides/slide{number}.xml"), slide.xml));
        let mut files = vec![(format!("image{number}.png"), slide.background)];
        files.extend(slide.media.into_iter().enumerate().map(|(index, media)| {
            (
                format!("image{number}-{}.{}", index + 1, media.extension),
                media.bytes,
            )
        }));
        let mut targets = vec![(
            "slideLayout",
            "../slideLayouts/slideLayout1.xml".to_string(),
        )];
        for (file, bytes) in files {
            targets.push(("image", format!("../media/{file}")));
            writer
                .start_file(format!("ppt/media/{file}"), stored)
                .map_err(failed)?;
            writer
                .write_all(&bytes)
                .map_err(|error| failed(error.into()))?;
        }
        parts.push((
            format!("ppt/slides/_rels/slide{number}.xml.rels"),
            relationships(&targets),
        ));
    }

    let mut embedded = Embedded::new();
    // A font that cannot be embedded is referenced by name only.
    for ((family, bold, italic), font) in fonts {
        let Some(data) = embeddable(&font) else {
            continue;
        };
        let number = links.len() + 1;
        links.push(("font", format!("fonts/font{number}.fntdata")));
        embedded.entry(family).or_default()[usize::from(bold) + 2 * usize::from(italic)] =
            Some(number);
        writer
            .start_file(
                format!("ppt/fonts/font{number}.fntdata"),
                SimpleFileOptions::default(),
            )
            .map_err(failed)?;
        writer
            .write_all(&data)
            .map_err(|error| failed(error.into()))?;
    }
    let (width, height) = (
        side(first.frame.width().to_pt()),
        side(first.frame.height().to_pt()),
    );
    parts.push((
        "ppt/presentation.xml".into(),
        presentation(pages.len(), width, height, &embedded),
    ));
    parts.push((
        "ppt/_rels/presentation.xml.rels".into(),
        relationships(&links),
    ));

    for (name, xml) in parts {
        writer
            .start_file(name, SimpleFileOptions::default())
            .map_err(failed)?;
        writer
            .write_all(xml.as_bytes())
            .map_err(|error| failed(error.into()))?;
    }
    Ok(writer.finish().map_err(failed)?.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use typst::layout::{Abs, Frame, Size};
    use zip::ZipArchive;

    use super::*;
    use crate::export::png;
    use crate::export::tests::{document, html, part};

    fn blank(page: &Page) -> bool {
        let picture = typst_render::render(page, &RenderOptions::default());
        picture
            .pixels()
            .iter()
            .all(|pixel| *pixel == picture.pixels()[0])
    }

    #[test]
    fn each_page_becomes_a_slide_with_editable_text_over_a_picture() {
        let source = "#set page(width: 400pt, height: 225pt)\n= Known heading\n#\"A & B < C\" *bold* _slanted_\n#pagebreak()\nTwo";
        let document = document(source);
        let bytes = pptx(&document, &Equations::new()).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().unwrap().to_string())
            .collect();

        let count = |folder: &str| {
            names
                .iter()
                .filter(|name| name.starts_with(folder) && !name.contains("_rels"))
                .count()
        };
        assert_eq!(count("ppt/slides/"), 2);
        assert_eq!(count("ppt/media/"), 2);
        assert!(
            part(&mut archive, "ppt/presentation.xml")
                .contains(r#"<p:sldSz cx="5080000" cy="2857500"/>"#)
        );
        assert!(
            part(&mut archive, "ppt/_rels/presentation.xml.rels").contains("slides/slide2.xml")
        );
        assert!(part(&mut archive, "[Content_Types].xml").contains("/ppt/slides/slide2.xml"));

        let slide = part(&mut archive, "ppt/slides/slide1.xml");
        assert!(slide.contains("<a:t>Known heading</a:t>"), "{slide}");
        assert!(slide.contains("A &amp; B &lt; C"), "{slide}");
        assert!(
            slide.contains(r#"b="1" i="0""#) && slide.contains(r#"b="0" i="1""#),
            "{slide}"
        );
        assert!(
            slide.contains(r#"<a:latin typeface="Libertinus Serif"/>"#),
            "{slide}"
        );
        assert_eq!(slide.matches("<p:sp>").count(), 2);
        assert!(part(&mut archive, "ppt/slides/slide2.xml").contains("<a:t>Two</a:t>"));

        assert!(!blank(&document.pages()[0]));
        assert!(blank(
            &layers(&document.pages()[0], &Equations::new()).unwrap().0
        ));
    }

    #[test]
    fn math_and_transformed_text_stay_in_the_picture() {
        let document = document("$ x^2 + y $\n#rotate(20deg)[Tilted]");
        let page = &document.pages()[0];
        let slide = slide(page, &Equations::new()).unwrap();
        assert!(!slide.xml.contains("<p:sp>"), "{}", slide.xml);
        assert!(slide.fonts.is_empty());
        assert!(!blank(&layers(page, &Equations::new()).unwrap().0));
    }

    #[test]
    fn text_in_a_clipped_box_is_editable_unless_the_clip_cuts_it() {
        let document = document(
            "#block(width: 200pt, inset: 10pt, radius: 8pt, fill: aqua, clip: true)[Inside the panel]\n#block(width: 30pt, height: 8pt, clip: true)[Cut off by the clip]",
        );
        let slide = slide(&document.pages()[0], &Equations::new()).unwrap();
        assert!(
            slide.xml.contains("<a:t>Inside the panel</a:t>"),
            "{}",
            slide.xml
        );
        assert!(!slide.xml.contains("Cut off"), "{}", slide.xml);
    }

    #[test]
    fn lines_of_one_paragraph_share_a_box_that_wraps() {
        let narrow = "#set page(width: 200pt, height: 400pt)\n";
        let boxes = |source: &str| {
            slide(
                &document(&format!("{narrow}{source}")).pages()[0],
                &Equations::new(),
            )
            .unwrap()
            .xml
        };

        let heading = boxes("= A heading that is long enough to wrap");
        assert_eq!(heading.matches("<p:sp>").count(), 1, "{heading}");
        assert!(
            heading.contains(r#"wrap="square""#) && heading.contains("<a:spcPts"),
            "{heading}"
        );
        assert!(
            heading.contains("long enough to wrap</a:t>") && !heading.contains("<a:br/>"),
            "{heading}"
        );

        let untagged = boxes("#block(width: 100pt, text(20pt)[Block title wrapping here])");
        assert_eq!(untagged.matches("<p:sp>").count(), 1, "{untagged}");

        let centred =
            boxes("#align(center)[A centred paragraph that is long enough to wrap over lines]");
        assert_eq!(centred.matches("<p:sp>").count(), 1, "{centred}");
        assert!(centred.contains(r#"algn="ctr""#), "{centred}");

        let forced = boxes("One \\ Two is longer");
        assert_eq!(forced.matches("<p:sp>").count(), 1, "{forced}");
        assert!(forced.contains("<a:br/>"), "{forced}");

        let glued = boxes("#block(width: 150pt)[Building Excellence, \\ Timeless Quality.]");
        assert!(
            glued.contains("<a:br/>") || glued.contains("<a:t> Timeless Quality.</a:t>"),
            "{glued}"
        );

        let single = boxes("Short");
        assert!(single.contains(r#"wrap="none""#), "{single}");

        let separate = boxes("First paragraph.\n\nSecond paragraph.\n- Item one\n- Item two");
        assert_eq!(separate.matches("<p:sp>").count(), 6, "{separate}");
    }

    #[test]
    fn true_type_fonts_travel_inside_the_presentation() {
        let document = document("Serif text and `mono text`");
        let bytes = pptx(&document, &Equations::new()).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();

        let links = part(&mut archive, "ppt/_rels/presentation.xml.rels");
        assert!(
            links.contains(r#"relationships/font" Target="fonts/font4.fntdata""#),
            "{links}"
        );
        let presentation = part(&mut archive, "ppt/presentation.xml");
        let listed = r#"<p:embeddedFontLst><p:embeddedFont><p:font typeface="DejaVu Sans Mono"/><p:regular r:id="rId4"/></p:embeddedFont></p:embeddedFontLst>"#;
        assert!(presentation.contains(listed), "{presentation}");
        assert!(part(&mut archive, "[Content_Types].xml").contains("application/x-fontdata"));

        let mut font = Vec::new();
        archive
            .by_name("ppt/fonts/font4.fntdata")
            .unwrap()
            .read_to_end(&mut font)
            .unwrap();
        let size = |at: usize| u32::from_le_bytes(font[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(size(0), font.len());
        assert_eq!(&font[34..36], &[0x4C, 0x50]);
        assert!(font[font.len() - size(4)..].starts_with(&[0, 1, 0, 0]));
    }

    #[test]
    fn images_become_pictures_unless_something_is_drawn_over_them() {
        let dot = png(&Frame::soft(Size::splat(Abs::pt(4.0))), 1.0).unwrap();
        std::fs::write(std::env::temp_dir().join("znote-export-dot.png"), dot).unwrap();
        let page = document(
            "#image(\"znote-export-dot.png\", width: 2cm, alt: \"A dot\")\n#place(top + left, rect(width: 3cm, height: 3cm, fill: blue.transparentize(50%)))\n#image(\"znote-export-dot.png\", width: 1cm)\n#rotate(10deg, image(\"znote-export-dot.png\", width: 1cm))",
        );
        let slide = slide(&page.pages()[0], &Equations::new()).unwrap();
        assert_eq!(
            slide.xml.matches(r#"name="Picture "#).count(),
            1,
            "{}",
            slide.xml
        );
        assert!(!slide.xml.contains("A dot"), "{}", slide.xml);
        assert_eq!(slide.media.len(), 1);
        assert_eq!(slide.media[0].extension, "png");
        assert!(
            slide.xml.contains(r#"<a:blip r:embed="rId3"/>"#),
            "{}",
            slide.xml
        );

        let alone = document("#image(\"znote-export-dot.png\", width: 2cm, alt: \"A dot\")");
        let bytes = pptx(&alone, &Equations::new()).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(part(&mut archive, "ppt/slides/slide1.xml").contains(r#"descr="A dot""#));
        assert!(
            part(&mut archive, "ppt/slides/_rels/slide1.xml.rels")
                .contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1-1.png""#)
        );
        let mut picture = Vec::new();
        archive
            .by_name("ppt/media/image1-1.png")
            .unwrap()
            .read_to_end(&mut picture)
            .unwrap();
        assert!(picture.starts_with(b"\x89PNG"));
    }

    #[test]
    fn formulas_become_equations_with_their_picture_as_fallback() {
        let source = "Text $x^2$ end.\n$ a/b $\n#rotate(20deg)[Tilted]";
        let equations = math::equations(&html(source).0);
        assert_eq!(equations.len(), 2);
        let document = document(source);
        let page = &document.pages()[0];
        let slide = slide(page, &equations).unwrap();
        assert_eq!(slide.xml.matches("<a14:m>").count(), 2, "{}", slide.xml);
        assert!(
            slide.xml.contains("<m:sSup>") && slide.xml.contains("<m:f>"),
            "{}",
            slide.xml
        );
        assert!(
            slide.xml.contains(r#"<a:latin typeface="Cambria Math"/>"#),
            "{}",
            slide.xml
        );
        assert_eq!(slide.xml.matches("<mc:Fallback>").count(), 2);
        assert_eq!(slide.media.len(), 2);

        let without = layers(page, &Equations::new()).unwrap().0;
        let with = layers(page, &equations).unwrap().0;
        let options = RenderOptions::default();
        assert_ne!(
            typst_render::render(&without, &options).data(),
            typst_render::render(&with, &options).data()
        );
    }
}
