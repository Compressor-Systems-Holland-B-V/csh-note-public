//! Export of the compiled page: PDF, PowerPoint (`pptx`) and Word (`docx`). All three come from
//! the page's own compile; Word and the equations in PowerPoint also read the same page compiled
//! for Typst's HTML target, which keeps its structure.

mod docx;
mod layout;
mod math;
mod pptx;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use typst::foundations::{Content, Smart};
use typst::layout::{Abs, Frame, FrameItem, Sides, Size};
use typst::text::{Font, FontFlags, FontInstance, FontStyle, FontWeight};
use typst::visualize::{ExchangeFormat, Image, ImageKind, RasterFormat};
use typst_layout::{Page, PagedDocument};
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;

use crate::compile;

const BROKE: &str = "This page could not be exported. Simplify it, for example make the page smaller, and try again.";

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const NAME_FAMILY: u16 = 1;
const NAME_STYLE: u16 = 2;
const NAME_FULL: u16 = 4;
const NAME_VERSION: u16 = 5;
const SELECTION_ITALIC: u16 = 0x01;
const SELECTION_BOLD: u16 = 0x20;
const EMBEDDING_RESTRICTED: u16 = 0x02;

fn table<'a>(font: &'a Font, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let data: &[u8] = font.data();
    let long =
        |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize);
    let start = if data.starts_with(b"ttcf") {
        long(12 + 4 * font.index() as usize)?
    } else {
        0
    };
    let count = usize::from(short(data, start + 4)?);
    let record = (0..count)
        .map(|index| start + 12 + 16 * index)
        .find(|&at| data.get(at..at + 4) == Some(tag))?;
    data.get(long(record + 8)?..)?.get(..long(record + 12)?)
}

fn short(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub fn restricted(font: &Font) -> bool {
    table(font, b"OS/2")
        .and_then(|os2| short(os2, 8))
        .is_some_and(|permissions| permissions & EMBEDDING_RESTRICTED != 0)
}

/// A TrueType font (not a collection, not variable) whose licence lets it travel inside a
/// document, as PowerPoint and Word embed fonts.
fn embeddable_true_type(font: &Font) -> bool {
    let data: &[u8] = font.data();
    (data.starts_with(&[0, 1, 0, 0]) || data.starts_with(b"true"))
        && !font.info().flags.contains(FontFlags::VARIABLE)
        && table(font, b"OS/2")
            .filter(|os2| os2.len() >= 78)
            .and_then(|os2| short(os2, 8))
            .is_some_and(|permissions| permissions & EMBEDDING_RESTRICTED == 0)
}

fn name(font: &FontInstance, id: u16) -> Option<String> {
    font.ttf()
        .names()
        .into_iter()
        .filter(|name| name.name_id == id)
        .find_map(|name| name.to_string())
}

struct Face {
    family: String,
    bold: bool,
    italic: bool,
}

fn face(instance: &FontInstance) -> Face {
    let font = instance.font();
    let info = font.info();
    let selection = table(font, b"OS/2").and_then(|os2| short(os2, 62));
    Face {
        family: name(instance, NAME_FAMILY).unwrap_or_else(|| info.family.clone()),
        bold: selection.map_or(info.variant.weight >= FontWeight::SEMIBOLD, |bits| {
            bits & SELECTION_BOLD != 0
        }),
        italic: selection.map_or(info.variant.style != FontStyle::Normal, |bits| {
            bits & SELECTION_ITALIC != 0
        }),
    }
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars().filter(|character| !character.is_control()) {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn relationships(targets: &[(&str, String)]) -> String {
    let mut xml = format!(r#"{XML}<Relationships xmlns="{RELATIONSHIPS}">"#);
    for (index, (kind, target)) in targets.iter().enumerate() {
        let external = if *kind == "hyperlink" {
            r#" TargetMode="External""#
        } else {
            ""
        };
        let _ = write!(
            xml,
            r#"<Relationship Id="rId{}" Type="{OFFICE}/{kind}" Target="{}"{external}/>"#,
            index + 1,
            escape(target)
        );
    }
    xml.push_str("</Relationships>");
    xml
}

pub fn pdf(document: &PagedDocument) -> Result<Vec<u8>, String> {
    typst_pdf::pdf(document, &PdfOptions::default()).map_err(|errors| {
        let reason = errors
            .first()
            .map(|error| error.message.to_string())
            .unwrap_or_default();
        format!("The PDF could not be made: {reason}")
    })
}

/// A picture file for a document part.
struct Media {
    bytes: Vec<u8>,
    extension: &'static str,
}

/// The frame drawn on a transparent background.
fn png(frame: &Frame, pixel_per_pt: f64) -> Result<Vec<u8>, String> {
    let page = Page {
        frame: frame.clone(),
        bleed: Sides::splat(Abs::zero()),
        fill: Smart::Custom(None),
        numbering: None,
        supplement: Content::empty(),
        number: 1,
    };
    let options = RenderOptions {
        pixel_per_pt: pixel_per_pt.into(),
        ..Default::default()
    };
    typst_render::render(&page, &options)
        .encode_png()
        .map_err(|error| format!("A picture could not be drawn: {error}"))
}

/// An image as Office reads it: PNG, JPEG and GIF as they are, anything else drawn at 2x.
fn media(image: &Image, size: Size) -> Result<Media, String> {
    if let ImageKind::Raster(raster) = image.kind()
        && raster.exif_rotation().is_none_or(|rotation| rotation <= 1)
        && let RasterFormat::Exchange(format) = raster.format()
    {
        let extension = match format {
            ExchangeFormat::Png => Some("png"),
            ExchangeFormat::Jpg => Some("jpg"),
            ExchangeFormat::Gif => Some("gif"),
            _ => None,
        };
        if let Some(extension) = extension {
            return Ok(Media {
                bytes: raster.data().to_vec(),
                extension,
            });
        }
    }
    let mut frame = Frame::soft(size);
    frame.push(
        Default::default(),
        FrameItem::Image(image.clone(), size, typst::syntax::Span::detached()),
    );
    Ok(Media {
        bytes: png(&frame, 2.0)?,
        extension: "png",
    })
}

/// Show rules for the HTML compile. Typst leaves layout out of HTML along with what it holds;
/// these keep the content. Images and drawings become frames so their pixels and size come along,
/// images measured against the text width so relative widths work.
fn prelude(document: &PagedDocument) -> String {
    let width = docx::text_width(document).to_pt();
    let picture = "html.frame(it)";
    let body = "it.body";
    let block = "block(it.body)";
    let drawn = "if it.body == none { html.frame(it) } else { it.body }";
    let rules = [
        ("image", format!("html.frame(box(width: {width}pt, it))")),
        ("align", block.into()),
        ("pad", block.into()),
        ("place", block.into()),
        ("columns", block.into()),
        ("move", body.into()),
        ("rotate", body.into()),
        ("scale", body.into()),
        ("skew", body.into()),
        ("rect", drawn.into()),
        ("square", drawn.into()),
        ("ellipse", drawn.into()),
        ("circle", drawn.into()),
        ("line", picture.into()),
        ("polygon", picture.into()),
        ("curve", picture.into()),
        (
            "stack",
            "it.children.filter(child => type(child) == content).join(parbreak())".into(),
        ),
        (
            "grid",
            "it.children.map(child => if child.func() == grid.cell { child.body }).join(parbreak())"
                .into(),
        ),
        (
            "pagebreak",
            format!(r#"html.elem("hr", attrs: (class: "{}"))"#, docx::PAGE_BREAK),
        ),
    ];
    // Only for the HTML itself: inside a picture Typst lays things out as on the page.
    rules
        .iter()
        .map(|(element, html)| {
            format!(
                "#show {element}: it => context if target() == \"html\" {{ {html} }} else {{ it }}\n"
            )
        })
        .collect()
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Pptx,
    Docx,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    path: String,
}

#[derive(Default)]
pub struct Exports {
    last: Mutex<Option<PathBuf>>,
}

impl Exports {
    fn last(&self) -> Result<PathBuf, String> {
        let last = self.last.lock().map_err(|error| error.to_string())?;
        last.clone()
            .ok_or_else(|| "Nothing has been exported yet.".to_string())
    }
}

#[tauri::command]
pub async fn export_page(
    app: AppHandle,
    page: String,
    text: String,
    format: Format,
) -> Result<Option<Exported>, String> {
    crate::blocking(move || export_now(&app, &page, text, format)).await
}

fn export_now(
    app: &AppHandle,
    page: &str,
    text: String,
    format: Format,
) -> Result<Option<Exported>, String> {
    let exports = app.state::<Exports>();
    let page = page.to_string();
    let (document, html) = compile::with_page(app, &page, |world| {
        let document = world.document(text.clone())?;
        let html =
            match format {
                Format::Pdf => None,
                // Without the HTML compile the formulas stay in the slide picture.
                Format::Pptx => world.html(text, &prelude(&document)).ok(),
                Format::Docx => Some(world.html(text, &prelude(&document)).map_err(|error| {
                    format!("This page cannot become a Word document: {error}")
                })?),
            };
        Ok((document, html))
    })?;
    let (label, extension) = match format {
        Format::Pdf => ("PDF", "pdf"),
        Format::Pptx => ("PowerPoint", "pptx"),
        Format::Docx => ("Word", "docx"),
    };
    let name = page.rsplit('/').next().unwrap_or(&page);
    let dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{name}.{extension}"))
        .add_filter(label, &[extension]);
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut path = target.into_path().map_err(|error| error.to_string())?;
    if path
        .extension()
        .is_none_or(|given| !given.eq_ignore_ascii_case(extension))
    {
        path.as_mut_os_string().push(format!(".{extension}"));
    }

    let bytes = match (format, html) {
        (Format::Pdf, _) => crate::guarded(BROKE, || pdf(&document))?,
        (Format::Pptx, html) => {
            let equations = html
                .map(|(html, _)| math::equations(&html))
                .unwrap_or_default();
            crate::guarded(BROKE, || pptx::pptx(&document, &equations))?
        }
        (Format::Docx, Some((html, _))) => {
            let equations = math::equations(&html);
            crate::guarded(BROKE, || docx::docx(&document, &html, &equations))?
        }
        (Format::Docx, None) => return Err(BROKE.into()),
    };
    std::fs::write(&path, bytes)
        .map_err(|error| format!("The file could not be saved: {error}"))?;
    *exports.last.lock().map_err(|error| error.to_string())? = Some(path.clone());
    Ok(Some(Exported {
        path: path.to_string_lossy().into_owned(),
    }))
}

#[tauri::command]
pub fn open_export(app: AppHandle, exports: State<Exports>) -> Result<(), String> {
    let path = exports.last()?.to_string_lossy().into_owned();
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|error| format!("The file could not be opened: {error}"))
}

#[tauri::command]
pub fn show_export(app: AppHandle, exports: State<Exports>) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(exports.last()?)
        .map_err(|error| format!("The folder could not be opened: {error}"))
}

#[cfg(test)]
pub(super) mod tests {
    use std::io::{Cursor, Read};
    use std::sync::{Arc, LazyLock};

    use typst_html::HtmlDocument;
    use typst_kit::fonts::FontStore;
    use zip::ZipArchive;

    use super::*;
    use crate::compile::{PageWorld, font_store};

    static FONTS: LazyLock<Arc<FontStore>> =
        LazyLock::new(|| Arc::new(font_store(&std::env::temp_dir().join("no-themes"))));

    pub fn world() -> PageWorld {
        let folder = std::env::temp_dir();
        PageWorld::new(
            &folder,
            "main.typ",
            &folder.join("no-themes"),
            FONTS.clone(),
            |_| {},
        )
        .unwrap()
    }

    pub fn document(text: &str) -> PagedDocument {
        world().document(text.into()).unwrap()
    }

    pub fn html(text: &str) -> (HtmlDocument, Vec<String>) {
        let document = document(text);
        world().html(text.into(), &prelude(&document)).unwrap()
    }

    pub fn part(archive: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    #[test]
    fn a_pdf_is_a_pdf() {
        assert!(pdf(&document("Hello")).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn a_page_with_problems_is_not_exported() {
        let mut world = world();
        assert!(
            world
                .document("#nope".into())
                .unwrap_err()
                .contains("Fix the problems")
        );
    }
}
