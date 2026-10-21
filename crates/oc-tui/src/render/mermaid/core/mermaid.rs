//! `core/mermaid.ts` — line iteration, quote stripping, and the HTML
//! entity decode (`entities`' `decodeHTMLStrict`, ported for the
//! entity set diagram labels actually use).

use crate::render::mermaid::core::text_lines::split_diagram_lines;

/// `NumberedMermaidLine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberedLine<'a> {
    pub line_number: usize,
    pub text: &'a str,
}

/// `numberedMermaidLines` — 1-based, trimmed.
pub fn numbered_mermaid_lines(content: &str) -> Vec<NumberedLine<'_>> {
    content
        .split("\r\n")
        .flat_map(|chunk| chunk.split('\n'))
        .enumerate()
        .map(|(index, line)| NumberedLine { line_number: index + 1, text: line.trim() })
        .collect()
}

/// `meaningfulNumberedMermaidLines` — non-empty, non-`%%` comment.
pub fn meaningful_numbered_mermaid_lines(content: &str) -> Vec<NumberedLine<'_>> {
    numbered_mermaid_lines(content)
        .into_iter()
        .filter(|line| !line.text.is_empty() && !line.text.starts_with("%%"))
        .collect()
}

/// `firstMeaningfulMermaidLine`.
#[must_use]
pub fn first_meaningful_mermaid_line(content: &str) -> Option<&str> {
    content
        .split("\r\n")
        .flat_map(|chunk| chunk.split('\n'))
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
}

/// `stripMermaidQuotes`.
#[must_use]
pub fn strip_mermaid_quotes(value: &str) -> String {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    let quoted = bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''));
    if quoted {
        decode_mermaid_text(&trimmed[1..trimmed.len() - 1])
    } else {
        decode_mermaid_text(trimmed)
    }
}

/// `decodeMermaidText` = `decodeHTMLStrict` — numeric forms plus the
/// common named entities (the `entities` package covers the full HTML5
/// table; diagram labels exercise this subset).
#[must_use]
pub fn decode_mermaid_text(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '&' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() && end - index <= 32 && chars[end] != ';' && chars[end] != '&' {
            end += 1;
        }
        if end >= chars.len() || chars[end] != ';' {
            out.push('&');
            index += 1;
            continue;
        }
        let body: String = chars[index + 1..end].iter().collect();
        let decoded = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = body.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            named_entity(&body)
        };
        match decoded {
            Some(value) => {
                out.push(value);
                index = end + 1;
            }
            None => {
                // `decodeHTMLStrict` leaves unknown entities verbatim.
                out.push('&');
                index += 1;
            }
        }
    }
    out
}

/// `splitDiagramLines` re-export for the diagram parsers (labels keep
/// their `<br>` structure through the decode).
#[must_use]
pub fn diagram_lines(value: &str) -> Vec<String> {
    split_diagram_lines(value)
}

fn named_entity(body: &str) -> Option<char> {
    Some(match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "plusmn" => '±',
        "middot" => '·',
        "bull" => '•',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "laquo" => '«',
        "raquo" => '»',
        "times" => '×',
        "divide" => '÷',
        "minus" => '−',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "asymp" => '≈',
        "equiv" => '≡',
        "infin" => '∞',
        "radic" => '√',
        "sum" => '∑',
        "prod" => '∏',
        "int" => '∫',
        "part" => '∂',
        "nabla" => '∇',
        "isin" => '∈',
        "notin" => '∉',
        "cap" => '∩',
        "cup" => '∪',
        "sub" => '⊂',
        "sup" => '⊃',
        "sube" => '⊆',
        "supe" => '⊇',
        "oplus" => '⊕',
        "otimes" => '⊗',
        "perp" => '⊥',
        "para" => '¶',
        "sect" => '§',
        "dagger" => '†',
        "Dagger" => '‡',
        "micro" => 'µ',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "curren" => '¤',
        "larr" => '←',
        "uarr" => '↑',
        "rarr" => '→',
        "darr" => '↓',
        "harr" => '↔',
        "Alpha" => 'Α',
        "Beta" => 'Β',
        "Gamma" => 'Γ',
        "Delta" => 'Δ',
        "epsilon" => 'ε',
        "theta" => 'θ',
        "lambda" => 'λ',
        "mu" => 'μ',
        "pi" => 'π',
        "sigma" => 'σ',
        "phi" => 'φ',
        "omega" => 'ω',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_numeric_and_named_entities() {
        assert_eq!(decode_mermaid_text("Worker &amp; signer"), "Worker & signer");
        assert_eq!(decode_mermaid_text("ack &lt;3s"), "ack <3s");
        assert_eq!(decode_mermaid_text("result &#8805; 1"), "result ≥ 1");
        assert_eq!(decode_mermaid_text("&#x2192;"), "→");
    }

    #[test]
    fn keeps_unknown_entities_verbatim() {
        assert_eq!(decode_mermaid_text("a &noentity; b"), "a &noentity; b");
        assert_eq!(decode_mermaid_text("a & b"), "a & b");
    }

    #[test]
    fn strips_matching_quotes_after_decoding() {
        assert_eq!(strip_mermaid_quotes("\"quoted\""), "quoted");
        assert_eq!(strip_mermaid_quotes(" plain "), "plain");
    }
}
