//! Fonts inside the Word file, so it looks the same on a computer that lacks them. Word reads
//! TrueType fonts stored "obfuscated": the first 32 bytes mixed with a key named in the font table
//! (ECMA-376 part 1, 17.8.1).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use typst::text::FontInstance;
use typst::utils::hash128;

use crate::export::{XML, embeddable_true_type, escape};

/// Per family: the regular, bold, italic and bold italic face.
pub type Faces = BTreeMap<String, [Option<FontInstance>; 4]>;

pub fn slot(bold: bool, italic: bool) -> usize {
    usize::from(bold) + 2 * usize::from(italic)
}

fn key(guid: &str) -> [u8; 16] {
    let digits: Vec<u8> = guid
        .chars()
        .filter_map(|c| c.to_digit(16))
        .map(|digit| digit as u8)
        .collect();
    let mut key = [0; 16];
    for (index, pair) in digits.chunks(2).take(16).enumerate() {
        key[15 - index] = pair[0] << 4 | pair[1];
    }
    key
}

pub fn obfuscate(data: &[u8], guid: &str) -> Vec<u8> {
    let key = key(guid);
    let mut font = data.to_vec();
    for (index, byte) in font.iter_mut().take(32).enumerate() {
        *byte ^= key[index % 16];
    }
    font
}

fn guid(data: &[u8]) -> String {
    let hex = format!("{:032X}", hash128(data));
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The font table, its relationships and the font files. A font that cannot go in is named only.
pub struct Embedded {
    pub table: String,
    pub links: Vec<(&'static str, String)>,
    pub files: Vec<(String, Vec<u8>)>,
}

pub fn embed(faces: &Faces) -> Embedded {
    let mut table = format!(
        r#"{XML}<w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#
    );
    let mut links = Vec::new();
    let mut files = Vec::new();
    for (family, styles) in faces {
        let _ = write!(table, r#"<w:font w:name="{}">"#, escape(family));
        for (style, face) in ["Regular", "Bold", "Italic", "BoldItalic"]
            .iter()
            .zip(styles)
        {
            let Some(face) = face else {
                continue;
            };
            if !embeddable_true_type(face.font()) {
                continue;
            }
            let data: &[u8] = face.font().data();
            let guid = guid(data);
            let file = format!("font{}.odttf", files.len() + 1);
            files.push((file.clone(), obfuscate(data, &guid)));
            links.push(("font", format!("fonts/{file}")));
            let _ = write!(
                table,
                r#"<w:embed{style} r:id="rId{}" w:fontKey="{guid}"/>"#,
                links.len()
            );
        }
        table.push_str("</w:font>");
    }
    table.push_str("</w:fonts>");
    Embedded {
        table,
        links,
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscation_mixes_the_first_32_bytes_with_the_reversed_key_and_undoes_itself() {
        let guid = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
        let data: Vec<u8> = (0..40).collect();
        let mixed = obfuscate(&data, guid);
        assert_eq!(mixed[0], 0xFF);
        assert_eq!(mixed[1], 1 ^ 0xEE);
        assert_eq!(mixed[16], 16 ^ 0xFF);
        assert_eq!(&mixed[32..], &data[32..]);
        assert_eq!(obfuscate(&mixed, guid), data);
    }
}
