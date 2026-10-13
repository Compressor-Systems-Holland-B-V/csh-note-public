use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use roxmltree::{Document, Node};
use zip::ZipArchive;

pub const OFFICE: [&str; 6] = ["docx", "pptx", "xlsx", "odt", "odp", "ods"];

const MAX_PART_BYTES: u64 = 32 * 1024 * 1024;

type Archive = ZipArchive<File>;

fn part(archive: &mut Archive, name: &str) -> Result<String, String> {
    let mut text = String::new();
    let entry = archive.by_name(name).map_err(|error| error.to_string())?;
    entry.take(MAX_PART_BYTES).read_to_string(&mut text).map_err(|error| error.to_string())?;
    Ok(text)
}

fn numbered(archive: &Archive, prefix: &str) -> Vec<String> {
    let number = |name: &str| name.strip_prefix(prefix)?.strip_suffix(".xml")?.parse::<u32>().ok();
    let mut names: Vec<(u32, String)> = archive.file_names().filter_map(|name| name.ok()).filter_map(|name| Some((number(&name)?, name.to_string()))).collect();
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

fn named(node: &Node, name: &str) -> bool {
    node.tag_name().name() == name
}

fn said(node: Node) -> String {
    node.descendants().filter(Node::is_text).filter_map(|text| text.text()).collect()
}

fn parsed(xml: &str) -> Result<Document<'_>, String> {
    Document::parse(xml).map_err(|error| error.to_string())
}

fn paragraphs(xml: &str) -> Result<String, String> {
    let lines: Vec<String> = parsed(xml)?
        .descendants()
        .filter(|node| named(node, "p") && !node.ancestors().skip(1).any(|above| named(&above, "p")))
        .map(|node| said(node).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Ok(lines.join("\n"))
}

fn sheet(xml: &str, shared: &[String]) -> Result<String, String> {
    let cell = |cell: Node| {
        let value = cell.children().find(|child| named(child, "v")).map(said);
        match (cell.attribute("t"), value) {
            (Some("s"), Some(index)) => index.parse().ok().and_then(|index: usize| shared.get(index).cloned()).unwrap_or_default(),
            (_, Some(value)) => value,
            _ => cell.descendants().filter(|inside| named(inside, "t")).map(said).collect(),
        }
    };
    let rows: Vec<String> = parsed(xml)?
        .descendants()
        .filter(|node| named(node, "row"))
        .map(|row| row.children().filter(|child| named(child, "c")).map(cell).collect::<Vec<_>>().join("\t"))
        .collect();
    Ok(rows.join("\n"))
}

fn office(file: &Path, kind: &str) -> Result<String, String> {
    let mut archive = File::open(file).map_err(|error| error.to_string()).and_then(|file| ZipArchive::new(file).map_err(|error| error.to_string()))?;
    let titled = |archive: &mut Archive, prefix: &str, title: &str, read: &dyn Fn(&str) -> Result<String, String>| {
        let parts: Result<Vec<String>, String> = numbered(archive, prefix)
            .iter()
            .enumerate()
            .map(|(index, name)| Ok(format!("{title} {}\n{}", index + 1, read(&part(archive, name)?)?)))
            .collect();
        parts.map(|parts| parts.join("\n\n"))
    };
    match kind {
        "docx" => paragraphs(&part(&mut archive, "word/document.xml")?),
        "pptx" => titled(&mut archive, "ppt/slides/slide", "Slide", &paragraphs),
        "xlsx" => {
            let shared: Vec<String> = match part(&mut archive, "xl/sharedStrings.xml") {
                Ok(xml) => parsed(&xml)?.descendants().filter(|node| named(node, "si")).map(said).collect(),
                Err(_) => Vec::new(),
            };
            titled(&mut archive, "xl/worksheets/sheet", "Sheet", &|xml| sheet(xml, &shared))
        }
        _ => paragraphs(&part(&mut archive, "content.xml")?),
    }
}

fn pdf(file: &Path) -> Result<String, String> {
    let document = lopdf::Document::load(file).map_err(|error| error.to_string())?;
    let pages: Vec<String> = document
        .get_pages()
        .into_keys()
        .filter_map(|page| Some(document.extract_text(&[page]).ok()?.trim().to_string()).filter(|text| !text.is_empty()).map(|text| format!("Page {page}\n{text}")))
        .collect();
    if pages.is_empty() {
        return Err("it has no text in it. It may be a scan.".into());
    }
    Ok(pages.join("\n\n"))
}

pub fn text(file: &Path) -> Result<String, String> {
    let kind = file.extension().and_then(|kind| kind.to_str()).unwrap_or_default().to_lowercase();
    let read = match kind.as_str() {
        "pdf" => pdf(file),
        kind if OFFICE.contains(&kind) => office(file, kind),
        _ => return fs::read_to_string(file).map_err(|_| "This file cannot be read as text.".to_string()),
    };
    read.map_err(|error| format!("This {kind} file cannot be read: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::compile::{PageWorld, font_store};

    fn scratch(test: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("znote-extract-{test}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn packed(file: &Path, parts: &[(&str, &str)]) {
        let mut archive = ZipWriter::new(File::create(file).unwrap());
        for (name, xml) in parts {
            archive.start_file(*name, SimpleFileOptions::default()).unwrap();
            archive.write_all(xml.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn a_pdf_gives_its_text_and_a_broken_one_gives_a_reason() {
        let folder = scratch("pdf");
        let fonts = Arc::new(font_store(&folder.join("no-themes")));
        let mut world = PageWorld::new(&folder, "main.typ", &folder.join("no-themes"), fonts, |_| {}).unwrap();
        let file = folder.join("report.PDF");
        fs::write(&file, crate::export::pdf(&world.document("Hello znote\n#pagebreak()\nSecond".into()).unwrap()).unwrap()).unwrap();
        let read = text(&file).unwrap();
        assert!(read.contains("Page 1\nHello znote") && read.contains("Page 2\nSecond"), "{read}");

        fs::write(&file, b"%PDF-1.7 nothing here").unwrap();
        assert!(text(&file).unwrap_err().starts_with("This pdf file cannot be read"));
    }

    #[test]
    fn office_files_give_their_text() {
        let folder = scratch("office");
        let file = |name: &str, parts: &[(&str, &str)]| {
            packed(&folder.join(name), parts);
            text(&folder.join(name))
        };
        let word = r#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Tom &amp; </w:t></w:r><w:r><w:t>Ann</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Next</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(file("a.docx", &[("word/document.xml", word)]).unwrap(), "Tom & Ann\nNext");

        let slide = |said: &str| format!(r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>{said}</a:t></a:r></a:p></p:sld>"#);
        let slides = [("ppt/slides/slide10.xml", slide("Last")), ("ppt/slides/slide2.xml", slide("First"))];
        let slides: Vec<(&str, &str)> = slides.iter().map(|(name, xml)| (*name, xml.as_str())).collect();
        assert_eq!(file("a.pptx", &slides).unwrap(), "Slide 1\nFirst\n\nSlide 2\nLast");

        let shared = "<sst><si><t>Name</t></si><si><r><t>Mi</t></r><r><t>lk</t></r></si></sst>";
        let cells = r#"<worksheet><sheetData><row><c t="s"><v>0</v></c><c t="inlineStr"><is><t>Price</t></is></c></row><row><c t="s"><v>1</v></c><c><f>1+1</f><v>2</v></c></row></sheetData></worksheet>"#;
        assert_eq!(file("a.xlsx", &[("xl/sharedStrings.xml", shared), ("xl/worksheets/sheet1.xml", cells)]).unwrap(), "Sheet 1\nName\tPrice\nMilk\t2");

        let open = r#"<office:document-content xmlns:office="o" xmlns:text="t"><text:p>Hello <text:span>there</text:span></text:p></office:document-content>"#;
        assert_eq!(file("a.odt", &[("content.xml", open)]).unwrap(), "Hello there");

        assert!(file("b.docx", &[("other.xml", "<a/>")]).is_err());
        fs::write(folder.join("c.docx"), "not a zip").unwrap();
        assert!(text(&folder.join("c.docx")).is_err());
        fs::write(folder.join("notes.md"), "plain").unwrap();
        assert_eq!(text(&folder.join("notes.md")).unwrap(), "plain");
        fs::write(folder.join("picture.png"), [0xff, 0xd8, 0x00]).unwrap();
        assert!(text(&folder.join("picture.png")).is_err());
    }
}
