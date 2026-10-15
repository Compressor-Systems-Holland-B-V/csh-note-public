use std::collections::HashMap;
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
    entry
        .take(MAX_PART_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text)
}

fn numbered(archive: &Archive, prefix: &str) -> Vec<String> {
    let number = |name: &str| {
        name.strip_prefix(prefix)?
            .strip_suffix(".xml")?
            .parse::<u32>()
            .ok()
    };
    let mut names: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|name| name.ok())
        .filter_map(|name| Some((number(&name)?, name.to_string())))
        .collect();
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

fn named(node: &Node, name: &str) -> bool {
    node.tag_name().name() == name
}

fn one_of(node: &Node, names: &[&str]) -> bool {
    names.contains(&node.tag_name().name())
}

fn attr<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|found| found.name() == name)
        .map(|found| found.value())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|inside| named(inside, name))
}

fn said(node: Node) -> String {
    node.descendants()
        .filter(Node::is_text)
        .filter_map(|text| text.text())
        .collect()
}

fn parsed(xml: &str) -> Result<Document<'_>, String> {
    Document::parse(xml).map_err(|error| error.to_string())
}

const BLOCKS: [&str; 4] = ["p", "h", "tbl", "table"];
const TABLES: [&str; 2] = ["tbl", "table"];
const ROWS: [&str; 2] = ["tr", "table-row"];
const CELLS: [&str; 3] = ["tc", "table-cell", "covered-table-cell"];
const MAX_ROWS: usize = 2000;
const MAX_COLUMNS: usize = 50;

type Headings = HashMap<String, usize>;

fn heading(style: &str) -> Option<usize> {
    let style = style.to_lowercase().replace(' ', "");
    if style == "title" {
        return Some(1);
    }
    style
        .strip_prefix("heading")?
        .parse()
        .ok()
        .filter(|level| (1..=6).contains(level))
}

fn headings(xml: &str) -> Headings {
    let Ok(document) = parsed(xml) else {
        return Headings::new();
    };
    document
        .descendants()
        .filter(|node| named(node, "style"))
        .filter_map(|style| {
            let level = heading(attr(&child(style, "name")?, "val")?)?;
            Some((attr(&style, "styleId")?.to_string(), level))
        })
        .collect()
}

fn spoken(node: Node) -> String {
    node.descendants()
        .filter(|inside| one_of(inside, &["p", "h"]))
        .map(|inside| said(inside).trim().to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn paragraph(node: Node, headings: &Headings) -> Option<(String, bool)> {
    let text = said(node).trim().to_string();
    if text.is_empty() {
        return None;
    }
    let props = child(node, "pPr");
    let level = if named(&node, "h") {
        Some(
            attr(&node, "outline-level")
                .and_then(|level| level.parse().ok())
                .unwrap_or(1usize)
                .clamp(1, 6),
        )
    } else {
        props
            .and_then(|props| child(props, "pStyle"))
            .and_then(|style| attr(&style, "val"))
            .and_then(|style| headings.get(style).copied().or_else(|| heading(style)))
    };
    if let Some(level) = level {
        return Some((format!("{} {text}", "#".repeat(level)), false));
    }
    let lists = node
        .ancestors()
        .filter(|above| named(above, "list"))
        .count();
    let depth = props
        .and_then(|props| child(props, "numPr"))
        .map(|numbered| {
            child(numbered, "ilvl")
                .and_then(|level| attr(&level, "val"))
                .and_then(|level| level.parse().ok())
                .unwrap_or(0usize)
        })
        .or((lists > 0).then(|| lists - 1));
    Some(match depth {
        Some(depth) => (format!("{}- {text}", "  ".repeat(depth.min(5))), true),
        None => (text, false),
    })
}

fn filled(cells: &[String]) -> bool {
    cells.iter().any(|cell| !cell.is_empty())
}

fn pipes(rows: &[Vec<String>]) -> String {
    let width = rows
        .iter()
        .filter_map(|cells| cells.iter().rposition(|cell| !cell.is_empty()))
        .max()
        .map_or(0, |last| last + 1);
    let line = |cells: &[String]| {
        let cells: Vec<String> = (0..width)
            .map(|index| {
                cells
                    .get(index)
                    .map(|cell| {
                        cell.split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .replace('|', "\\|")
                    })
                    .unwrap_or_default()
            })
            .collect();
        format!("| {} |", cells.join(" | "))
    };
    let mut rows = rows.iter().filter(|cells| filled(cells));
    let Some(head) = rows.next() else {
        return String::new();
    };
    let mut lines = vec![line(head), format!("|{}", " --- |".repeat(width))];
    lines.extend(rows.map(|cells| line(cells)));
    lines.join("\n")
}

fn in_table(row: &Node, table: Node) -> bool {
    row.ancestors().skip(1).find(|above| one_of(above, &TABLES)) == Some(table)
}

fn grid(table: Node) -> String {
    let rows: Vec<Vec<String>> = table
        .descendants()
        .filter(|row| one_of(row, &ROWS) && in_table(row, table))
        .map(|row| {
            row.children()
                .filter(|cell| one_of(cell, &CELLS))
                .map(spoken)
                .collect()
        })
        .collect();
    pipes(&rows)
}

fn blocks(root: Node, headings: &Headings, skipped: Option<Node>) -> String {
    let found: Vec<(String, bool)> = root
        .descendants()
        .filter(|node| {
            one_of(node, &BLOCKS)
                && !node
                    .ancestors()
                    .skip(1)
                    .any(|above| one_of(&above, &BLOCKS))
                && !skipped.is_some_and(|skipped| node.ancestors().any(|above| above == skipped))
        })
        .filter_map(|node| {
            if one_of(&node, &TABLES) {
                Some((grid(node), false)).filter(|(table, _)| !table.is_empty())
            } else {
                paragraph(node, headings)
            }
        })
        .collect();
    let mut text = String::new();
    for (index, (block, item)) in found.iter().enumerate() {
        if index > 0 {
            text.push_str(if *item && found[index - 1].1 {
                "\n"
            } else {
                "\n\n"
            });
        }
        text.push_str(block);
    }
    text
}

fn section(heading: &str, body: &str) -> String {
    if body.is_empty() {
        format!("## {heading}")
    } else {
        format!("## {heading}\n\n{body}")
    }
}

fn slide(number: usize, root: Node, title: Option<Node>) -> String {
    let named = title
        .map(spoken)
        .filter(|title| !title.is_empty())
        .map_or(format!("Slide {number}"), |title| {
            format!("Slide {number}: {title}")
        });
    section(&named, &blocks(root, &Headings::new(), title))
}

fn sheet_table(mut rows: Vec<Vec<String>>) -> String {
    let more = rows.len() > MAX_ROWS;
    rows.truncate(MAX_ROWS);
    let table = pipes(&rows);
    if more {
        format!("{table}\n\n(More rows not shown.)")
    } else {
        table
    }
}

fn column(reference: &str) -> Option<usize> {
    reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .take(3)
        .fold(0usize, |number, letter| {
            number * 26 + (letter.to_ascii_uppercase() as usize - 'A' as usize + 1)
        })
        .checked_sub(1)
}

fn sheet(xml: &str, shared: &[String]) -> Result<String, String> {
    let value = |cell: Node| {
        let value = child(cell, "v").map(said);
        match (attr(&cell, "t"), value) {
            (Some("s"), Some(index)) => index
                .parse()
                .ok()
                .and_then(|index: usize| shared.get(index).cloned())
                .unwrap_or_default(),
            (_, Some(value)) => value,
            _ => cell
                .descendants()
                .filter(|inside| named(inside, "t"))
                .map(said)
                .collect(),
        }
    };
    let document = parsed(xml)?;
    let mut rows = Vec::new();
    for row in document.descendants().filter(|node| named(node, "row")) {
        let mut cells: Vec<String> = Vec::new();
        for cell in row.children().filter(|inside| named(inside, "c")) {
            let at = attr(&cell, "r")
                .and_then(column)
                .unwrap_or(cells.len())
                .max(cells.len());
            if at >= MAX_COLUMNS {
                break;
            }
            cells.resize(at, String::new());
            cells.push(value(cell));
        }
        if filled(&cells) {
            rows.push(cells);
            if rows.len() > MAX_ROWS {
                break;
            }
        }
    }
    Ok(sheet_table(rows))
}

fn repeated(node: &Node, name: &str) -> usize {
    attr(node, name)
        .and_then(|count| count.parse().ok())
        .unwrap_or(1)
}

fn open_sheet(table: Node) -> String {
    let mut rows = Vec::new();
    for row in table
        .descendants()
        .filter(|row| named(row, "table-row") && in_table(row, table))
    {
        let mut cells: Vec<String> = Vec::new();
        for cell in row.children().filter(|cell| one_of(cell, &CELLS)) {
            let text = spoken(cell);
            let count = repeated(&cell, "number-columns-repeated").min(MAX_COLUMNS - cells.len());
            cells.extend(std::iter::repeat_n(text, count));
            if cells.len() >= MAX_COLUMNS {
                break;
            }
        }
        if !filled(&cells) {
            continue;
        }
        let count = repeated(&row, "number-rows-repeated").min(MAX_ROWS + 1 - rows.len());
        rows.extend(std::iter::repeat_n(cells, count));
        if rows.len() > MAX_ROWS {
            break;
        }
    }
    sheet_table(rows)
}

fn worksheets(archive: &mut Archive) -> Vec<(String, String)> {
    let mut listed = || -> Result<Vec<(String, String)>, String> {
        let links = part(archive, "xl/_rels/workbook.xml.rels")?;
        let links = parsed(&links)?;
        let targets: HashMap<&str, &str> = links
            .descendants()
            .filter(|node| named(node, "Relationship"))
            .filter_map(|link| Some((attr(&link, "Id")?, attr(&link, "Target")?)))
            .collect();
        let workbook = part(archive, "xl/workbook.xml")?;
        let found = parsed(&workbook)?
            .descendants()
            .filter(|node| named(node, "sheet"))
            .filter_map(|sheet| {
                let target = targets.get(attr(&sheet, "id")?)?;
                let path = target
                    .strip_prefix('/')
                    .map_or(format!("xl/{target}"), str::to_string);
                Some((attr(&sheet, "name")?.to_string(), path))
            })
            .filter(|(_, path)| archive.index_for_name(path).is_some())
            .collect();
        Ok(found)
    };
    match listed() {
        Ok(found) if !found.is_empty() => found,
        _ => numbered(archive, "xl/worksheets/sheet")
            .into_iter()
            .enumerate()
            .map(|(index, name)| (format!("Sheet {}", index + 1), name))
            .collect(),
    }
}

fn office(file: &Path, kind: &str) -> Result<String, String> {
    let mut archive = File::open(file)
        .map_err(|error| error.to_string())
        .and_then(|file| ZipArchive::new(file).map_err(|error| error.to_string()))?;
    let mut sections = Vec::new();
    match kind {
        "docx" => {
            let styles = part(&mut archive, "word/styles.xml")
                .map(|xml| headings(&xml))
                .unwrap_or_default();
            let xml = part(&mut archive, "word/document.xml")?;
            sections.push(blocks(parsed(&xml)?.root(), &styles, None));
        }
        "pptx" => {
            for (index, name) in numbered(&archive, "ppt/slides/slide").iter().enumerate() {
                let xml = part(&mut archive, name)?;
                let document = parsed(&xml)?;
                let title = document
                    .descendants()
                    .find(|node| {
                        named(node, "ph")
                            && matches!(attr(node, "type"), Some("title" | "ctrTitle"))
                    })
                    .and_then(|place| place.ancestors().find(|above| named(above, "sp")));
                sections.push(slide(index + 1, document.root(), title));
            }
        }
        "xlsx" => {
            let shared: Vec<String> = match part(&mut archive, "xl/sharedStrings.xml") {
                Ok(xml) => parsed(&xml)?
                    .descendants()
                    .filter(|node| named(node, "si"))
                    .map(said)
                    .collect(),
                Err(_) => Vec::new(),
            };
            for (name, path) in worksheets(&mut archive) {
                sections.push(section(
                    &name,
                    &sheet(&part(&mut archive, &path)?, &shared)?,
                ));
            }
        }
        _ => {
            let xml = part(&mut archive, "content.xml")?;
            let document = parsed(&xml)?;
            match kind {
                "odp" => {
                    for (index, page) in document
                        .descendants()
                        .filter(|node| named(node, "page"))
                        .enumerate()
                    {
                        let title = page.descendants().find(|node| {
                            named(node, "frame") && attr(node, "class") == Some("title")
                        });
                        sections.push(slide(index + 1, page, title));
                    }
                }
                "ods" => {
                    for (index, table) in document
                        .descendants()
                        .filter(|node| {
                            named(node, "table")
                                && !node.ancestors().skip(1).any(|above| named(&above, "table"))
                        })
                        .enumerate()
                    {
                        let name = attr(&table, "name")
                            .map_or(format!("Sheet {}", index + 1), str::to_string);
                        sections.push(section(&name, &open_sheet(table)));
                    }
                }
                _ => sections.push(blocks(document.root(), &Headings::new(), None)),
            }
        }
    }
    Ok(sections.join("\n\n"))
}

fn pdf(file: &Path) -> Result<String, String> {
    let document = lopdf::Document::load(file).map_err(|error| error.to_string())?;
    let pages: Vec<String> = document
        .get_pages()
        .into_keys()
        .filter_map(|page| {
            Some(document.extract_text(&[page]).ok()?.trim().to_string())
                .filter(|text| !text.is_empty())
                .map(|text| section(&format!("Page {page}"), &text))
        })
        .collect();
    if pages.is_empty() {
        return Err("it has no text in it. It may be a scan.".into());
    }
    Ok(pages.join("\n\n"))
}

pub fn text(file: &Path) -> Result<String, String> {
    crate::guarded("This file could not be read: it may be damaged.", || {
        read(file)
    })
}

fn read(file: &Path) -> Result<String, String> {
    let kind = file
        .extension()
        .and_then(|kind| kind.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let read = match kind.as_str() {
        "pdf" => pdf(file),
        kind if OFFICE.contains(&kind) => office(file, kind),
        _ => {
            return fs::read_to_string(file)
                .map_err(|_| "This file cannot be read as text.".to_string());
        }
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
        let folder =
            std::env::temp_dir().join(format!("znote-extract-{test}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn packed(file: &Path, parts: &[(&str, &str)]) {
        let mut archive = ZipWriter::new(File::create(file).unwrap());
        for (name, xml) in parts {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(xml.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn a_pdf_gives_its_text_and_a_broken_one_gives_a_reason() {
        let folder = scratch("pdf");
        let fonts = Arc::new(font_store(&folder.join("no-themes")));
        let mut world = PageWorld::new(
            &folder,
            "main.typ",
            &folder.join("no-themes"),
            fonts,
            |_| {},
        )
        .unwrap();
        let file = folder.join("report.PDF");
        fs::write(
            &file,
            crate::export::pdf(
                &world
                    .document("Hello znote\n#pagebreak()\nSecond".into())
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let read = text(&file).unwrap();
        assert!(
            read.contains("## Page 1\n\nHello znote") && read.contains("## Page 2\n\nSecond"),
            "{read}"
        );

        fs::write(&file, b"%PDF-1.7 nothing here").unwrap();
        assert!(
            text(&file)
                .unwrap_err()
                .starts_with("This pdf file cannot be read")
        );
    }

    #[test]
    fn office_files_give_their_text() {
        let folder = scratch("office");
        let file = |name: &str, parts: &[(&str, &str)]| {
            packed(&folder.join(name), parts);
            text(&folder.join(name))
        };
        let word = r#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Tom &amp; </w:t></w:r><w:r><w:t>Ann</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Next</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            file("a.docx", &[("word/document.xml", word)]).unwrap(),
            "Tom & Ann\n\nNext"
        );

        let slide = |said: &str| {
            format!(
                r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>{said}</a:t></a:r></a:p></p:sld>"#
            )
        };
        let slides = [
            ("ppt/slides/slide10.xml", slide("Last")),
            ("ppt/slides/slide2.xml", slide("First")),
        ];
        let slides: Vec<(&str, &str)> = slides
            .iter()
            .map(|(name, xml)| (*name, xml.as_str()))
            .collect();
        assert_eq!(
            file("a.pptx", &slides).unwrap(),
            "## Slide 1\n\nFirst\n\n## Slide 2\n\nLast"
        );

        let shared = "<sst><si><t>Name</t></si><si><r><t>Mi</t></r><r><t>lk</t></r></si></sst>";
        let cells = r#"<worksheet><sheetData><row><c t="s"><v>0</v></c><c t="inlineStr"><is><t>Price</t></is></c></row><row><c t="s"><v>1</v></c><c><f>1+1</f><v>2</v></c></row></sheetData></worksheet>"#;
        assert_eq!(
            file(
                "a.xlsx",
                &[
                    ("xl/sharedStrings.xml", shared),
                    ("xl/worksheets/sheet1.xml", cells)
                ]
            )
            .unwrap(),
            "## Sheet 1\n\n| Name | Price |\n| --- | --- |\n| Milk | 2 |"
        );

        let open = r#"<office:document-content xmlns:office="o" xmlns:text="t"><text:p>Hello <text:span>there</text:span></text:p></office:document-content>"#;
        assert_eq!(
            file("a.odt", &[("content.xml", open)]).unwrap(),
            "Hello there"
        );

        assert!(file("b.docx", &[("other.xml", "<a/>")]).is_err());
        fs::write(folder.join("c.docx"), "not a zip").unwrap();
        assert!(text(&folder.join("c.docx")).is_err());
        fs::write(folder.join("notes.md"), "plain").unwrap();
        assert_eq!(text(&folder.join("notes.md")).unwrap(), "plain");
        fs::write(folder.join("picture.png"), [0xff, 0xd8, 0x00]).unwrap();
        assert!(text(&folder.join("picture.png")).is_err());
    }

    #[test]
    fn office_files_keep_their_structure() {
        let folder = scratch("structure");
        let file = |name: &str, parts: &[(&str, &str)]| {
            packed(&folder.join(name), parts);
            text(&folder.join(name)).unwrap()
        };
        let styles = r#"<w:styles xmlns:w="w"><w:style w:styleId="Kop2"><w:name w:val="heading 2"/></w:style></w:styles>"#;
        let word = r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Plan</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Kop2"/></w:pPr><w:r><w:t>Goals</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr><w:r><w:t>Two</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Role</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>Tom | Ann</w:t></w:r></w:p><w:p><w:r><w:t>Smith</w:t></w:r></w:p></w:tc><w:tc/></w:tr></w:tbl>
            <w:p><w:r><w:t>End</w:t></w:r></w:p>
        </w:body></w:document>"#;
        assert_eq!(
            file(
                "plan.docx",
                &[("word/document.xml", word), ("word/styles.xml", styles)]
            ),
            "# Plan\n\n## Goals\n\n- One\n  - Two\n\n| Name | Role |\n| --- | --- |\n| Tom \\| Ann Smith |  |\n\nEnd"
        );

        let open = r#"<office:document-content xmlns:office="o" xmlns:text="t"><office:body><office:text>
            <text:h text:outline-level="3">Scope</text:h>
            <text:list><text:list-item><text:p>Alpha</text:p></text:list-item></text:list>
            </office:text></office:body></office:document-content>"#;
        assert_eq!(
            file("a.odt", &[("content.xml", open)]),
            "### Scope\n\n- Alpha"
        );

        let slide = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><a:p><a:r><a:t>Intro</a:t></a:r></a:p></p:sp><p:sp><a:p><a:r><a:t>Hello</a:t></a:r></a:p></p:sp></p:sld>"#;
        assert_eq!(
            file("a.pptx", &[("ppt/slides/slide1.xml", slide)]),
            "## Slide 1: Intro\n\nHello"
        );

        let book = r#"<workbook xmlns:r="r"><sheets><sheet name="Budget" sheetId="1" r:id="rId7"/></sheets></workbook>"#;
        let links = r#"<Relationships><Relationship Id="rId7" Target="worksheets/sheet3.xml"/></Relationships>"#;
        let cells = r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="C1" t="inlineStr"><is><t>Cost</t></is></c></row><row/><row><c r="C3"><v>4</v></c></row></sheetData></worksheet>"#;
        assert_eq!(
            file(
                "a.xlsx",
                &[
                    ("xl/workbook.xml", book),
                    ("xl/_rels/workbook.xml.rels", links),
                    ("xl/worksheets/sheet3.xml", cells)
                ]
            ),
            "## Budget\n\n| Item |  | Cost |\n| --- | --- | --- |\n|  |  | 4 |"
        );

        let calc = r#"<office:document-content xmlns:office="o" xmlns:table="t" xmlns:text="x"><office:body><office:spreadsheet>
            <table:table table:name="Stock"><table:table-row><table:table-cell><text:p>Box</text:p></table:table-cell><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
            <table:table-row table:number-rows-repeated="3"><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row>
            <table:table-row table:number-rows-repeated="1048576"><table:table-cell table:number-columns-repeated="16384"/></table:table-row></table:table>
            </office:spreadsheet></office:body></office:document-content>"#;
        assert_eq!(
            file("a.ods", &[("content.xml", calc)]),
            "## Stock\n\n| Box |\n| --- |\n| x |\n| x |\n| x |"
        );

        let many: String = (0..MAX_ROWS + 5)
            .map(|row| format!("<row><c><v>{row}</v></c></row>"))
            .collect();
        let many = format!("<worksheet><sheetData>{many}</sheetData></worksheet>");
        let read = file("big.xlsx", &[("xl/worksheets/sheet1.xml", &many)]);
        assert!(
            read.ends_with(&format!("| {} |\n\n(More rows not shown.)", MAX_ROWS - 1)),
            "{read}"
        );

        let show = r#"<office:document-content xmlns:office="o" xmlns:draw="d" xmlns:presentation="p" xmlns:text="t"><office:body><office:presentation>
            <draw:page><draw:frame presentation:class="title"><draw:text-box><text:p>Welcome</text:p></draw:text-box></draw:frame><draw:frame><draw:text-box><text:p>Body</text:p></draw:text-box></draw:frame></draw:page>
            <draw:page/></office:presentation></office:body></office:document-content>"#;
        assert_eq!(
            file("a.odp", &[("content.xml", show)]),
            "## Slide 1: Welcome\n\nBody\n\n## Slide 2"
        );
    }
}
