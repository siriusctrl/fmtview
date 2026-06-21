use ratatui::{style::Style, text::Span};

use crate::transform::FormatKind;
use crate::tui::palette::{escape_style, plain_style, string_style};

const JSON_VISIBLE_COMPOSITE_LANDMARK_LINES: usize = 5;

/// HTML void elements: they never have a closing tag or content. The same set
/// is also treated as self-contained by XML structure jumps, which keeps
/// `<br>` and `<img>` from opening a phantom block in either format.
pub(crate) const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub(crate) fn is_void_tag(tag: &str) -> bool {
    VOID_TAGS.contains(&tag)
}

/// Decide whether a bounded markup prefix is HTML or XML for auto-detection.
///
/// This is a cheap, pure heuristic. It only runs when the input has no known
/// extension and the first non-whitespace byte is `<`. Strong signals (`<?xml`,
/// `<!doctype html>`, an `<html>` root) are checked first. Anything ambiguous
/// falls back to XML, which is the wider category and preserves the previous
/// behavior.
pub(crate) fn detect_markup_kind(prefix: &[u8]) -> FormatKind {
    let prefix = trim_ascii_ws_prefix(prefix);
    let leading = take_ascii_prefix(prefix, 4096);
    let lower = to_ascii_lower_lossy(leading.as_slice());
    let text = lower.as_slice();

    if text.starts_with(b"<?xml") {
        return FormatKind::Xml;
    }
    if starts_text_ignore_leading_ws(text, b"<!doctype html") {
        return FormatKind::Html;
    }
    if contains_tag(text, b"html") || contains_tag(text, b"head") || contains_tag(text, b"body") {
        return FormatKind::Html;
    }
    // Void elements are matched against the original-case prefix: only a
    // lowercase `<br>`/`<img>`/... counts. A capitalized `<Link>` or `<Source>`
    // is almost certainly a custom or XML element, not HTML's void tag, so it
    // must not trigger HTML detection.
    if contains_void_tag_unsat(leading.as_slice()) {
        return FormatKind::Html;
    }
    // XML-leaning signals: namespaces, xml declarations anywhere in prefix,
    // or known XML root tags.
    if text.windows(4).any(|w| w == b"xmlns") || text.windows(5).any(|w| w == b"<?xml") {
        return FormatKind::Xml;
    }
    if contains_tag(text, b"rss")
        || contains_tag(text, b"feed")
        || contains_tag(text, b"sitemap")
        || contains_tag(text, b"urlset")
        || contains_tag(text, b"svg")
    {
        return FormatKind::Xml;
    }
    FormatKind::Xml
}

fn trim_ascii_ws_prefix(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    &bytes[start..]
}

fn take_ascii_prefix(bytes: &[u8], limit: usize) -> Vec<u8> {
    bytes.iter().take(limit).copied().collect()
}

fn to_ascii_lower_lossy(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(|b| b.to_ascii_lowercase()).collect()
}

fn starts_text_ignore_leading_ws(text: &[u8], needle: &[u8]) -> bool {
    let trimmed = trim_ascii_ws_prefix(text);
    trimmed.starts_with(needle)
}

/// True if `name` appears as a start or end tag, e.g. `<html`, `</html>`.
/// Matching a tag boundary avoids matching `name` inside attribute values or
/// text.
fn contains_tag(text: &[u8], name: &[u8]) -> bool {
    let mut search = text;
    while let Some(index) = search.iter().position(|b| *b == b'<') {
        let rest = &search[index + 1..];
        let after_slash = rest.strip_prefix(b"/").unwrap_or(rest);
        if after_slash.starts_with(name) {
            if let Some(next) = after_slash.get(name.len()) {
                if !is_name_byte(*next) {
                    return true;
                }
            } else {
                return true;
            }
        }
        search = &search[index + 1..];
    }
    false
}

/// True if a void element appears without an immediate self-closing slash,
/// e.g. `<br>` or `<img src=x>`. That shape is invalid XML and a strong HTML
/// signal. `<br/>` (slash immediately after the name) is treated as XML-style
/// and does not count.
fn contains_void_tag_unsat(text: &[u8]) -> bool {
    for tag in VOID_TAGS {
        let needle = [b"<", tag.as_bytes()].concat();
        let mut search = text;
        while let Some(index) = search.windows(needle.len()).position(|w| w == needle) {
            let next = search.get(index + needle.len()).copied();
            match next {
                Some(b'/') | None => { /* self-closed or truncated; keep scanning */ }
                Some(byte) if is_name_byte(byte) => {
                    // The void tag name is only a prefix of a longer custom/XML
                    // tag such as `<bridge>` or `<linkage>`.
                }
                _ => return true,
            }
            search = &search[index + needle.len()..];
        }
    }
    false
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructureCandidateKind {
    JsonChatMessage,
    JsonRecordStart,
    JsonArrayItemStart,
    JsonCompositeField,
    JsonRootStart,
    XmlStartTag,
    MarkdownHeading,
    TomlTable,
    JinjaBlock,
    PlainParagraph,
}

impl StructureCandidateKind {
    pub(crate) fn is_landmark_when_visible(self, line_span: Option<usize>) -> bool {
        match self {
            StructureCandidateKind::JsonChatMessage
            | StructureCandidateKind::JsonRecordStart
            | StructureCandidateKind::JsonArrayItemStart
            | StructureCandidateKind::JsonRootStart
            | StructureCandidateKind::MarkdownHeading
            | StructureCandidateKind::TomlTable
            | StructureCandidateKind::JinjaBlock
            | StructureCandidateKind::PlainParagraph => true,
            StructureCandidateKind::XmlStartTag => line_span.is_none_or(|span| span > 1),
            StructureCandidateKind::JsonCompositeField => {
                line_span.is_some_and(|span| span >= JSON_VISIBLE_COMPOSITE_LANDMARK_LINES)
            }
        }
    }
}

pub(crate) fn highlight_string_segment_window(
    source: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if end <= window_start {
        return;
    }

    let mut index = start;
    let mut plain_start = start;

    while index < end {
        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= end)
        {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            push_span_window(
                spans,
                source,
                index,
                escape_end,
                escape_style(),
                window_start,
                window_end,
            );
            index = escape_end;
            plain_start = index;
            continue;
        }

        let Some(ch) = source[index..end].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        end,
        string_style(),
        window_start,
        window_end,
    );
}

pub(crate) fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

pub(crate) fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

pub(crate) fn escaped_quoted_end(text: &str, start: usize, quote: char) -> usize {
    let pattern = if quote == '"' { "\\\"" } else { "\\'" };
    text[start + pattern.len()..]
        .find(pattern)
        .map(|offset| start + pattern.len() + offset + pattern.len())
        .unwrap_or(text.len())
}

pub(crate) fn escape_token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    if !rest.starts_with('\\') {
        return None;
    }

    let mut chars = rest.chars();
    chars.next()?;
    let escaped = chars.next()?;
    let escaped_start = start + '\\'.len_utf8();
    let escaped_end = escaped_start + escaped.len_utf8();

    if escaped == 'u' {
        let unicode_end = escaped_end + 4;
        if text
            .get(escaped_end..unicode_end)
            .is_some_and(|digits| digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        {
            return Some(unicode_end);
        }
    }

    Some(escaped_end)
}

pub(crate) fn push_span_window(
    spans: &mut Vec<Span<'static>>,
    source: &str,
    start: usize,
    end: usize,
    style: Style,
    window_start: usize,
    window_end: usize,
) {
    let start = floor_char_boundary(source, start.min(source.len()));
    let end = floor_char_boundary(source, end.min(source.len()));
    let window_start = floor_char_boundary(source, window_start.min(source.len()));
    let window_end = floor_char_boundary(source, window_end.min(source.len()));
    let overlap_start = start.max(window_start);
    let overlap_end = end.min(window_end);
    if overlap_start < overlap_end {
        let style = normalize_span_style(style);
        push_text_span(spans, source[overlap_start..overlap_end].to_owned(), style);
    }
}

pub(crate) fn leading_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(crate) fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}

pub(crate) fn max_observed_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    if lines.is_empty() || viewport_bottom < read_start {
        return None;
    }
    Some((viewport_bottom - read_start).min(lines.len() - 1))
}

pub(crate) fn max_boundary_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    max_observed_offset(lines, read_start, viewport_bottom.saturating_add(1))
}

pub(crate) fn following_lines(
    lines: &[String],
    start_offset: usize,
    max_offset: usize,
) -> impl Iterator<Item = (usize, &String)> {
    lines
        .iter()
        .enumerate()
        .take(max_offset + 1)
        .skip(start_offset + 1)
}

pub(crate) fn eof_block_end(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    if !line_count_exact || line_count == 0 {
        return None;
    }
    let eof_line = line_count - 1;
    let read_end = read_start.saturating_add(lines.len());
    (eof_line <= viewport_bottom && eof_line < read_end).then_some(eof_line)
}

pub(crate) fn indent_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_indent = leading_indent(lines.get(start_offset)?);
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_indent(line) <= start_indent {
            if is_same_indent_closing_line(line) {
                return Some(read_start + offset);
            }
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn is_same_indent_closing_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.starts_with("</")
        || crate::formats::jinja::structure::keyword(trimmed)
            .is_some_and(|keyword| keyword.starts_with("end"))
}

pub(crate) fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn normalize_span_style(style: Style) -> Style {
    if style == plain_style() {
        Style::default()
    } else {
        style
    }
}

fn push_text_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if text.is_empty() {
        return;
    }

    if style == Style::default()
        && let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push_str(&text);
        return;
    }

    spans.push(Span::styled(text, style));
}

#[cfg(test)]
mod tests {
    use super::detect_markup_kind;
    use crate::transform::FormatKind;

    #[test]
    fn xml_declaration_is_xml() {
        assert_eq!(
            detect_markup_kind(b"<?xml version=\"1.0\"?>\n<root/>"),
            FormatKind::Xml
        );
    }

    #[test]
    fn doctype_html_is_html() {
        assert_eq!(
            detect_markup_kind(b"<!DOCTYPE html>\n<html>"),
            FormatKind::Html
        );
    }

    #[test]
    fn html_root_is_html() {
        assert_eq!(
            detect_markup_kind(b"<html><body>x</body></html>"),
            FormatKind::Html
        );
    }

    #[test]
    fn unclosed_void_tag_is_html() {
        assert_eq!(detect_markup_kind(b"<div><br>ok</div>"), FormatKind::Html);
    }

    #[test]
    fn self_closed_void_tag_is_not_html_signal() {
        // <br/> is XML-compatible; with nothing else HTML-leaning, default to Xml.
        assert_eq!(detect_markup_kind(b"<root><br/></root>"), FormatKind::Xml);
    }

    #[test]
    fn capitalized_void_name_is_not_html_signal() {
        assert_eq!(
            detect_markup_kind(b"<Link><child/></Link>"),
            FormatKind::Xml
        );
    }

    #[test]
    fn void_tag_prefix_inside_name_is_not_html_signal() {
        assert_eq!(
            detect_markup_kind(b"<bridge><item/></bridge>"),
            FormatKind::Xml
        );
    }

    #[test]
    fn rss_feed_is_xml() {
        assert_eq!(
            detect_markup_kind(b"<rss version=\"2.0\"><channel>"),
            FormatKind::Xml
        );
    }

    #[test]
    fn svg_with_xmlns_is_xml() {
        assert_eq!(
            detect_markup_kind(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            FormatKind::Xml
        );
    }

    #[test]
    fn empty_prefix_defaults_to_xml() {
        assert_eq!(detect_markup_kind(b""), FormatKind::Xml);
    }

    #[test]
    fn leading_whitespace_is_skipped() {
        assert_eq!(
            detect_markup_kind(b"\n\n  <!doctype html>"),
            FormatKind::Html
        );
    }
}
