//! `core/text-lines.ts` — `<br>` splitting + italic-run parsing.

/// `DiagramTextRun`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    pub italic: bool,
}

/// `DiagramTextLine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLine {
    pub text: String,
    pub runs: Vec<TextRun>,
}

/// `parseDiagramTextLines` — splits on `<br>`/`<br/>`/`<br />` and
/// parses `<i>`/`<em>` toggle runs.
#[must_use]
pub fn parse_diagram_text_lines(value: &str) -> Vec<TextLine> {
    split_br(value)
        .into_iter()
        .map(|line| {
            let source = line.trim();
            let mut runs: Vec<TextRun> = Vec::new();
            let mut italic = false;
            let mut offset = 0usize;

            let mut index = 0usize;
            let bytes = source.as_bytes();
            while index < bytes.len() {
                // match /<\\/?(?:i|em)\\s*>/gi
                if bytes[index] == b'<' {
                    let rest = &source[index..];
                    let tag_len = italic_tag_len(rest);
                    if let Some(tag_len) = tag_len {
                        if index > offset {
                            runs.push(TextRun { text: source[offset..index].to_string(), italic });
                        }
                        italic = !rest.starts_with("</");
                        offset = index + tag_len;
                        index = offset;
                        continue;
                    }
                }
                index += 1;
            }
            if offset < source.len() {
                runs.push(TextRun { text: source[offset..].to_string(), italic });
            }

            let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            TextLine { text, runs }
        })
        .collect()
}

/// `/^<br\\s*\\/?\\s*>/i` split (upstream `/<br\\s*\\/?>/i` — the JS
/// regex tolerates `<br/ >` spacing through `\\s*` around the slash).
fn split_br(value: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '<' {
            let rest: String = chars[index..].iter().collect();
            if let Some(len) = br_tag_len(&rest) {
                lines.push(std::mem::take(&mut current));
                index += len;
                continue;
            }
        }
        current.push(chars[index]);
        index += 1;
    }
    lines.push(current);
    lines
}

/// The consumed length when `value` starts with a `<br>` variant
/// (`/<br\s*\/?>/i`: whitespace before the optional slash only).
fn br_tag_len(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    let rest = lower.strip_prefix("<br")?;
    let bytes = rest.as_bytes();
    let mut offset = 0usize;
    while offset < bytes.len() && (bytes[offset] as char).is_whitespace() {
        offset += 1;
    }
    if offset < bytes.len() && bytes[offset] == b'/' {
        offset += 1;
    }
    if offset < bytes.len() && bytes[offset] == b'>' {
        Some("<br".len() + offset + 1)
    } else {
        None
    }
}

/// The consumed length when `value` starts with `<i>`, `</i>`, `<em>`,
/// `</em>` or their whitespace-padded forms.
fn italic_tag_len(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    let rest = lower.strip_prefix('<')?;
    let rest = rest.strip_prefix('/');
    let after_slash = rest.is_some();
    let body = rest.unwrap_or(&lower[1..]);
    let name = body
        .strip_prefix("em")
        .map(|rest| (rest, "em".len()))
        .or_else(|| body.strip_prefix('i').map(|rest| (rest, "i".len())))?;
    let (rest, name_len) = name;
    let mut offset = 0usize;
    let bytes = rest.as_bytes();
    while offset < bytes.len() && (bytes[offset] as char).is_whitespace() {
        offset += 1;
    }
    if offset < bytes.len() && bytes[offset] == b'>' {
        let total = 1 + usize::from(after_slash) + name_len + offset + 1;
        Some(total)
    } else {
        None
    }
}

/// `splitDiagramLines` — just the text content.
#[must_use]
pub fn split_diagram_lines(value: &str) -> Vec<String> {
    parse_diagram_text_lines(value).into_iter().map(|line| line.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_br_tags_case_insensitively() {
        assert_eq!(split_diagram_lines("a<br>b<BR/>c<br />d"), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn parses_italic_runs() {
        let lines = parse_diagram_text_lines("<i>italic</i> plain <em>emph</em>");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "italic plain emph");
        assert_eq!(lines[0].runs[0].italic, true);
        assert_eq!(lines[0].runs[1].italic, false);
        assert_eq!(lines[0].runs[2].italic, true);
    }
}
