//! Typst's MathML (from the HTML target) as Office Math, which Word and PowerPoint edit natively.
//! Elements without an Office Math counterpart keep their content, laid out in a row.

use std::collections::HashMap;

use typst::foundations::Content;
use typst::introspection::Tag;
use typst::math::EquationElem;
use typst::utils::{ResolvedPicoStr, hash128};
use typst_html::{HtmlDocument, HtmlElement, HtmlNode};

use super::escape;

/// The `math` element of each equation in an HTML compile, by [`key`].
pub type Equations = HashMap<u128, HtmlElement>;

pub const NAMESPACE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

const OPENING: &str = "([{|‖⟨⌊⌈⟦";
const CLOSING: &str = ")]}|‖⟩⌋⌉⟧";
const LARGE: &str = "∑∏∐⋀⋁⋂⋃⨀⨁⨂⨄⨆∫∬∭∮∯∰∱∲∳⨌";

/// Identifies an equation across the paged and the HTML compile of the same page: its body, spans
/// included, is the same in both.
pub fn key(content: &Content) -> Option<u128> {
    content
        .to_packed::<EquationElem>()
        .map(|equation| hash128(&equation.body))
}

pub fn equations(document: &HtmlDocument) -> Equations {
    fn walk(nodes: &[HtmlNode], pending: &mut Option<u128>, found: &mut Equations) {
        for node in nodes {
            match node {
                HtmlNode::Tag(Tag::Start(content, _)) => {
                    if let Some(key) = key(content) {
                        *pending = Some(key);
                    }
                }
                HtmlNode::Element(element) if &*name(element) == "math" => {
                    if let Some(key) = pending.take() {
                        found.insert(key, element.clone());
                    }
                }
                HtmlNode::Element(element) => walk(&element.children, pending, found),
                _ => {}
            }
        }
    }
    let mut found = Equations::new();
    walk(&document.root().children, &mut None, &mut found);
    found
}

pub fn name(element: &HtmlElement) -> ResolvedPicoStr {
    element.tag.resolve()
}

pub fn attribute<'a>(element: &'a HtmlElement, key: &str) -> Option<&'a str> {
    element
        .attrs
        .0
        .iter()
        .find(|(attr, _)| attr.resolve().as_str() == key)
        .map(|(_, value)| value.as_str())
}

/// One `m:oMath` for a `math` element. `run` goes into every `m:r` after its `m:rPr`: the run
/// properties of the host format (`w:rPr` in Word, `a:rPr` in PowerPoint).
pub fn omml(math: &HtmlElement, run: &str) -> String {
    let mut xml = String::from("<m:oMath>");
    row(&math.children, run, &mut xml);
    xml.push_str("</m:oMath>");
    xml
}

fn elements(nodes: &[HtmlNode]) -> Vec<&HtmlElement> {
    nodes
        .iter()
        .filter_map(|node| match node {
            HtmlNode::Element(element) => Some(element),
            _ => None,
        })
        .collect()
}

fn text(element: &HtmlElement) -> String {
    let mut text = String::new();
    for node in &element.children {
        match node {
            HtmlNode::Text(part, _) => text.push_str(part),
            HtmlNode::Element(inner) => text.push_str(&self::text(inner)),
            _ => {}
        }
    }
    text
}

fn token(content: &str, upright: bool, run: &str, xml: &mut String) {
    if content.is_empty() {
        return;
    }
    let style = if upright {
        r#"<m:rPr><m:sty m:val="p"/></m:rPr>"#
    } else {
        ""
    };
    xml.push_str(&format!(
        r#"<m:r>{style}{run}<m:t xml:space="preserve">{}</m:t></m:r>"#,
        escape(content)
    ));
}

/// The large operator an `m:nary` stands for, if `element` is one.
fn large(element: &HtmlElement) -> Option<char> {
    let mut chars = text(element).chars().collect::<Vec<_>>();
    (&*name(element) == "mo" && chars.len() == 1 && LARGE.contains(chars[0]))
        .then(|| chars.remove(0))
}

fn wrap(tag: &str, nodes: &[&HtmlElement], run: &str, xml: &mut String) {
    xml.push_str(&format!("<m:{tag}>"));
    for element in nodes {
        node(element, run, xml);
    }
    xml.push_str(&format!("</m:{tag}>"));
}

fn row(nodes: &[HtmlNode], run: &str, xml: &mut String) {
    let items: Vec<&HtmlNode> = nodes
        .iter()
        .filter(|node| matches!(node, HtmlNode::Element(_) | HtmlNode::Text(..)))
        .collect();
    let mut index = 0;
    while index < items.len() {
        let element = match items[index] {
            HtmlNode::Element(element) => element,
            HtmlNode::Text(content, _) => {
                token(content, false, run, xml);
                index += 1;
                continue;
            }
            _ => unreachable!(),
        };
        let arguments = elements(&element.children);
        let base = arguments.first().and_then(|base| large(base));
        let scripts = match &*name(element) {
            "munder" | "mover" | "munderover" => Some("undOvr"),
            "msub" | "msup" | "msubsup" => Some("subSup"),
            _ => None,
        };
        if let (Some(operator), Some(location)) = (base, scripts) {
            let (under, over) = match &*name(element) {
                "munder" | "msub" => (arguments.get(1), None),
                "mover" | "msup" => (None, arguments.get(1)),
                _ => (arguments.get(1), arguments.get(2)),
            };
            xml.push_str(&format!(
                r#"<m:nary><m:naryPr><m:chr m:val="{operator}"/><m:limLoc m:val="{location}"/>{}{}</m:naryPr>"#,
                if under.is_none() { r#"<m:subHide m:val="1"/>"# } else { "" },
                if over.is_none() { r#"<m:supHide m:val="1"/>"# } else { "" },
            ));
            wrap(
                "sub",
                &under.into_iter().copied().collect::<Vec<_>>(),
                run,
                xml,
            );
            wrap(
                "sup",
                &over.into_iter().copied().collect::<Vec<_>>(),
                run,
                xml,
            );
            // The operand is the next item: MathML puts it beside the operator, Office Math inside.
            let operand: Vec<&HtmlElement> = match items.get(index + 1) {
                Some(HtmlNode::Element(next)) => vec![next],
                _ => Vec::new(),
            };
            wrap("e", &operand, run, xml);
            xml.push_str("</m:nary>");
            index += 1 + operand.len();
            continue;
        }
        node(element, run, xml);
        index += 1;
    }
}

fn fenced(element: &HtmlElement, run: &str, xml: &mut String) -> bool {
    let items = elements(&element.children);
    let fence = |index: usize, set: &str| {
        items
            .get(index)
            .filter(|item| &*name(item) == "mo")
            .map(|item| text(item))
            .filter(|content| content.chars().count() == 1 && set.contains(content.as_str()))
    };
    let open = fence(0, OPENING);
    let close = items
        .len()
        .checked_sub(1)
        .and_then(|last| fence(last, CLOSING));
    if items.len() < 2 || (open.is_none() && close.is_none()) {
        return false;
    }
    let inner = &items[usize::from(open.is_some())..items.len() - usize::from(close.is_some())];
    xml.push_str(&format!(
        r#"<m:d><m:dPr><m:begChr m:val="{}"/><m:endChr m:val="{}"/></m:dPr>"#,
        escape(open.as_deref().unwrap_or_default()),
        escape(close.as_deref().unwrap_or_default()),
    ));
    xml.push_str("<m:e>");
    for item in inner {
        node(item, run, xml);
    }
    xml.push_str("</m:e></m:d>");
    true
}

fn table(element: &HtmlElement, run: &str, xml: &mut String) {
    let rows: Vec<Vec<&HtmlElement>> = elements(&element.children)
        .into_iter()
        .filter(|row| &*name(row) == "mtr")
        .map(|row| elements(&row.children))
        .collect();
    let class = attribute(element, "class").unwrap_or_default();
    if class.contains("cases") || class.contains("multiline-equation") {
        xml.push_str("<m:eqArr>");
        for cells in rows {
            xml.push_str("<m:e>");
            for cell in cells {
                row(&cell.children, run, xml);
            }
            xml.push_str("</m:e>");
        }
        xml.push_str("</m:eqArr>");
        return;
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
    xml.push_str(&format!(
        r#"<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="{columns}"/><m:mcJc m:val="center"/></m:mcPr></m:mc></m:mcs></m:mPr>"#
    ));
    for cells in rows {
        xml.push_str("<m:mr>");
        for cell in &cells {
            xml.push_str("<m:e>");
            row(&cell.children, run, xml);
            xml.push_str("</m:e>");
        }
        // Office Math wants every row as wide as the matrix.
        xml.push_str(&"<m:e/>".repeat(columns - cells.len()));
        xml.push_str("</m:mr>");
    }
    xml.push_str("</m:m>");
}

fn node(element: &HtmlElement, run: &str, xml: &mut String) {
    let arguments = elements(&element.children);
    let argument = |index: usize| {
        arguments
            .get(index)
            .copied()
            .into_iter()
            .collect::<Vec<_>>()
    };
    match &*name(element) {
        "mi" | "mn" | "mo" | "mtext" | "ms" => {
            let content = text(element);
            let upright = &*name(element) == "mtext"
                || attribute(element, "mathvariant") == Some("normal")
                || (&*name(element) == "mo" && content.chars().any(char::is_alphabetic));
            token(&content, upright, run, xml);
        }
        "mspace" => {
            let width = attribute(element, "width")
                .and_then(|width| width.strip_suffix("em"))
                .and_then(|width| width.parse::<f64>().ok())
                .unwrap_or(0.0);
            if width > 0.0 {
                token(
                    if width >= 0.5 { "\u{2003}" } else { "\u{2005}" },
                    true,
                    run,
                    xml,
                );
            }
        }
        "mfrac" => {
            let bar = attribute(element, "linethickness").is_none_or(|thickness| {
                thickness
                    .trim_end_matches(char::is_alphabetic)
                    .parse::<f64>()
                    .is_ok_and(|thickness| thickness != 0.0)
            });
            xml.push_str("<m:f>");
            if !bar {
                xml.push_str(r#"<m:fPr><m:type m:val="noBar"/></m:fPr>"#);
            }
            wrap("num", &argument(0), run, xml);
            wrap("den", &argument(1), run, xml);
            xml.push_str("</m:f>");
        }
        "msup" | "msub" => {
            let (kind, script) = if &*name(element) == "msup" {
                ("sSup", "sup")
            } else {
                ("sSub", "sub")
            };
            xml.push_str(&format!("<m:{kind}>"));
            wrap("e", &argument(0), run, xml);
            wrap(script, &argument(1), run, xml);
            xml.push_str(&format!("</m:{kind}>"));
        }
        "msubsup" => {
            xml.push_str("<m:sSubSup>");
            wrap("e", &argument(0), run, xml);
            wrap("sub", &argument(1), run, xml);
            wrap("sup", &argument(2), run, xml);
            xml.push_str("</m:sSubSup>");
        }
        "msqrt" => {
            xml.push_str(r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/>"#);
            wrap("e", &arguments, run, xml);
            xml.push_str("</m:rad>");
        }
        "mroot" => {
            xml.push_str("<m:rad>");
            wrap("deg", &argument(1), run, xml);
            wrap("e", &argument(0), run, xml);
            xml.push_str("</m:rad>");
        }
        "mover" if attribute(element, "accent") == Some("true") => {
            let mark = arguments.get(1).map(|mark| text(mark)).unwrap_or_default();
            xml.push_str(&format!(
                r#"<m:acc><m:accPr><m:chr m:val="{}"/></m:accPr>"#,
                escape(&mark)
            ));
            wrap("e", &argument(0), run, xml);
            xml.push_str("</m:acc>");
        }
        "munder" | "mover" => {
            let kind = if &*name(element) == "munder" {
                "limLow"
            } else {
                "limUpp"
            };
            xml.push_str(&format!("<m:{kind}>"));
            wrap("e", &argument(0), run, xml);
            wrap("lim", &argument(1), run, xml);
            xml.push_str(&format!("</m:{kind}>"));
        }
        "munderover" => {
            xml.push_str("<m:limUpp><m:e><m:limLow>");
            wrap("e", &argument(0), run, xml);
            wrap("lim", &argument(1), run, xml);
            xml.push_str("</m:limLow></m:e>");
            wrap("lim", &argument(2), run, xml);
            xml.push_str("</m:limUpp>");
        }
        "mtable" => table(element, run, xml),
        "mphantom" => {
            xml.push_str(r#"<m:phant><m:phantPr><m:show m:val="0"/></m:phantPr><m:e>"#);
            row(&element.children, run, xml);
            xml.push_str("</m:e></m:phant>");
        }
        "annotation" | "annotation-xml" | "mprescripts" => {}
        "mrow" if fenced(element, run, xml) => {}
        _ => row(&element.children, run, xml),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::tests::html;

    fn find<'a>(nodes: &'a [HtmlNode], tag: &str) -> Option<&'a HtmlElement> {
        nodes.iter().find_map(|node| match node {
            HtmlNode::Element(element) if &*name(element) == tag => Some(element),
            HtmlNode::Element(element) => find(&element.children, tag),
            _ => None,
        })
    }

    fn convert(source: &str) -> String {
        let document = html(source).0;
        omml(find(&document.root().children, "math").unwrap(), "")
    }

    #[test]
    fn fractions_scripts_and_roots_become_office_math() {
        let fraction = convert("$ (a + b)/2 $");
        assert!(fraction.starts_with("<m:oMath><m:f><m:num>"), "{fraction}");
        assert!(
            fraction.contains("<m:den><m:r><m:t xml:space=\"preserve\">2</m:t></m:r></m:den>"),
            "{fraction}"
        );

        let scripts = convert("$ x_i^2 + y^n $");
        assert!(scripts.contains("<m:sSubSup><m:e>"), "{scripts}");
        assert!(scripts.contains("<m:sSup><m:e>"), "{scripts}");

        let roots = convert("$ sqrt(x) + root(3, y) $");
        assert!(
            roots.contains(r#"<m:rad><m:radPr><m:degHide m:val="1"/>"#),
            "{roots}"
        );
        assert!(roots.contains("<m:rad><m:deg><m:r>"), "{roots}");

        let binomial = convert("$ binom(n, k) $");
        assert!(
            binomial.contains(r#"<m:type m:val="noBar"/>"#),
            "{binomial}"
        );
    }

    #[test]
    fn matrices_fences_sums_and_cases_keep_their_structure() {
        let matrix = convert("$ mat(1, 2; 3, 4) $");
        assert!(
            matrix.contains(r#"<m:begChr m:val="("/><m:endChr m:val=")"/>"#),
            "{matrix}"
        );
        assert!(matrix.contains(r#"<m:count m:val="2"/>"#), "{matrix}");
        assert_eq!(matrix.matches("<m:mr>").count(), 2, "{matrix}");

        let sum = convert("$ sum_(i=0)^n x_i $");
        assert!(
            sum.contains(r#"<m:chr m:val="∑"/><m:limLoc m:val="undOvr"/>"#),
            "{sum}"
        );
        assert!(sum.contains("</m:sup><m:e><m:sSub>"), "{sum}");

        let integral = convert("$ integral_0^1 x dif x $");
        assert!(
            integral.contains(r#"<m:chr m:val="∫"/><m:limLoc m:val="subSup"/>"#),
            "{integral}"
        );

        let cases = convert("$ f(x) = cases(1 \"if\" x > 0, 0) $");
        assert!(
            cases.contains(r#"<m:begChr m:val="{"/><m:endChr m:val=""/>"#),
            "{cases}"
        );
        assert!(cases.contains("<m:eqArr>"), "{cases}");
        assert!(
            cases.contains(r#"<m:sty m:val="p"/></m:rPr><m:t xml:space="preserve">if"#),
            "{cases}"
        );

        let accent = convert("$ hat(x) $");
        assert!(accent.contains("<m:acc><m:accPr><m:chr"), "{accent}");
    }

    #[test]
    fn the_host_run_properties_go_into_every_run_and_text_is_escaped() {
        let document = html("$ a < b $").0;
        let xml = omml(find(&document.root().children, "math").unwrap(), "<w:rPr/>");
        assert_eq!(xml.matches("<m:r><w:rPr/>").count(), 3, "{xml}");
        assert!(xml.contains("&lt;"), "{xml}");
    }
}
