use std::net::{Ipv4Addr, Ipv6Addr};

use super::rules::{Check, Rules};

fn iban(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| *c != ' ').collect();
    if !(15..=34).contains(&compact.len()) {
        return false;
    }
    let (head, tail) = compact.split_at(4);
    let mut rest = 0u32;
    for c in tail.chars().chain(head.chars()) {
        let Some(value) = c.to_digit(36) else {
            return false;
        };
        rest = if value < 10 {
            (rest * 10 + value) % 97
        } else {
            (rest * 100 + value) % 97
        };
    }
    rest == 1
}

fn luhn(text: &str) -> bool {
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(place, &digit)| match (place % 2, digit * 2) {
            (0, _) => digit,
            (_, doubled) if doubled > 9 => doubled - 9,
            (_, doubled) => doubled,
        })
        .sum();
    sum.is_multiple_of(10)
}

fn ipv6(text: &str) -> bool {
    text.matches(':').count() >= 2
        && text.chars().filter(char::is_ascii_hexdigit).count() >= 4
        && text.parse::<Ipv6Addr>().is_ok()
}

fn phone(text: &str) -> bool {
    let digits = text.chars().filter(char::is_ascii_digit).count();
    let enough = if text.starts_with('+') { 8 } else { 9 };
    (enough..=15).contains(&digits)
}

fn amount(text: &str, after: &str) -> bool {
    !(text.starts_with('$') && after.starts_with('$')) && !text.ends_with('$')
}

fn valid(check: Option<Check>, text: &str, after: &str) -> bool {
    match check {
        None => true,
        Some(Check::Iban) => iban(text),
        Some(Check::Luhn) => luhn(text),
        Some(Check::Ipv4) => text.parse::<Ipv4Addr>().is_ok(),
        Some(Check::Ipv6) => ipv6(text),
        Some(Check::Phone) => phone(text),
        Some(Check::Amount) => amount(text, after),
    }
}

pub fn find<'a>(rules: &'a Rules, text: &str, amounts: bool) -> Vec<(usize, usize, &'a str)> {
    let mut found = Vec::new();
    for pattern in rules
        .patterns
        .iter()
        .filter(|pattern| amounts || !pattern.amounts)
    {
        for hit in pattern.regex.find_iter(text) {
            let end = hit.start() + hit.as_str().trim_end_matches(pattern.trim.as_slice()).len();
            let piece = &text[hit.start()..end];
            if !piece.is_empty()
                && !piece.contains('\n')
                && valid(pattern.check, piece, &text[end..])
            {
                found.push((hit.start(), end, pattern.kind.as_str()));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::rules::rules;
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, &'static str)> {
        let mut found = find(rules().unwrap(), text, true);
        found.sort();
        found
            .into_iter()
            .map(|(start, end, kind)| (&text[start..end], kind))
            .collect()
    }

    #[test]
    fn contact_details_and_numbers_are_found_and_checked() {
        assert_eq!(
            kinds("Mail jan.de-vries@acme.example.nl or see https://acme.example/x?y=1."),
            [
                ("jan.de-vries@acme.example.nl", "EMAIL"),
                ("https://acme.example/x?y=1", "URL")
            ]
        );
        let ibans: Vec<(&str, &str)> =
            kinds("IBAN NL91 ABNA 0417 1643 00 and NL91 ABNA 0417 1643 01")
                .into_iter()
                .filter(|(_, kind)| *kind == "IBAN")
                .collect();
        assert_eq!(ibans, [("NL91 ABNA 0417 1643 00", "IBAN")]);
        assert_eq!(
            kinds("card 4111 1111 1111 1111, not 4111 1111 1111 1112"),
            [("4111 1111 1111 1111", "CARD")]
        );
        assert_eq!(
            kinds("hosts 10.0.0.1, 999.1.1.1 and fe80::1:2:3 at 12:30"),
            [("10.0.0.1", "IP"), ("fe80::1:2:3", "IP")]
        );
        assert_eq!(
            kinds(r"Typst escapes it as first\_last\@acme.example"),
            [(r"first\_last\@acme.example", "EMAIL")]
        );
        assert_eq!(
            kinds("call +31 6 1234 5678 or 010-123 4567, not 2024-10-14"),
            [("+31 6 1234 5678", "PHONE"), ("010-123 4567", "PHONE")]
        );
    }

    #[test]
    fn amounts_need_a_currency_and_typst_math_is_left_alone() {
        assert_eq!(
            kinds("costs € 1.250,00 or EUR 40k, 300 USD and \\$12 but not $5$ or 300 apples"),
            [
                ("€ 1.250,00", "AMOUNT"),
                ("EUR 40k", "AMOUNT"),
                ("300 USD", "AMOUNT"),
                ("\\$12", "AMOUNT")
            ]
        );
        assert!(find(rules().unwrap(), "€ 10", false).is_empty());
        assert_eq!(
            kinds("€ 5 mln and 2 miljoen EUR"),
            [("€ 5 mln", "AMOUNT"), ("2 miljoen EUR", "AMOUNT")]
        );
    }
}
