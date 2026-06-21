use crate::transform::FormatKind;

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
