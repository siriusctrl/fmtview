use std::io::{BufRead, Write};

use anyhow::{Context, Result, anyhow};

/// Format HTML-compatible markup with structural indentation.
///
/// Unlike the XML formatter, this uses a tolerant tokenizer: void elements
/// (`<br>`, `<img>`, ...), optional closing tags (`<p>`, `<li>`, `<td>`, ...),
/// and unquoted attributes are accepted. Markup and text-node content are
/// preserved, while formatting-only whitespace between elements may be
/// normalized into one-element-per-line indentation. Missing close tags are not
/// synthesized and stray close tags are not dropped, so the visible text stays
/// faithful to the input.
pub(crate) fn format_html_reader<R: BufRead, W: Write>(
    mut input: R,
    output: &mut W,
    indent: usize,
) -> Result<()> {
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .context("failed to read HTML input")?;
    let _ = std::str::from_utf8(&bytes).map_err(|error| anyhow!(error))?;

    let mut formatter = HtmlFormatter::new(&bytes, indent);
    formatter.run()?;
    formatter.flush(output)?;
    writeln!(output)?;
    Ok(())
}

struct HtmlFormatter<'a> {
    src: &'a [u8],
    indent: usize,
    out: Vec<u8>,
    pos: usize,
    depth: usize,
    stack: Vec<Vec<u8>>,
}

/// Elements whose content is raw text or whitespace-significant. Their
/// `start ... end` span is emitted verbatim as a single unit so internal
/// whitespace and newlines are preserved exactly (no added indentation, no
/// added newlines around the content).
const RAW_TEXT_TAGS: &[&str] = &["script", "style", "pre", "textarea", "title"];

impl<'a> HtmlFormatter<'a> {
    fn new(src: &'a [u8], indent: usize) -> Self {
        Self {
            src,
            indent,
            out: Vec::with_capacity(src.len() + src.len() / 4),
            pos: 0,
            depth: 0,
            stack: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<()> {
        while self.pos < self.src.len() {
            let Some(lt) = next_lt(self.src, self.pos) else {
                self.emit_text(&self.src[self.pos..]);
                self.pos = self.src.len();
                break;
            };
            if lt > self.pos {
                self.emit_text(&self.src[self.pos..lt]);
            }
            self.pos = lt;
            if !self.handle_markup() {
                // `<` that is not a real tag start: treat as text.
                self.out.push(b'<');
                self.pos = lt + 1;
            }
        }
        Ok(())
    }

    fn flush(&mut self, output: &mut impl Write) -> Result<()> {
        output
            .write_all(&self.out)
            .context("failed to write formatted HTML")?;
        self.out.clear();
        Ok(())
    }

    /// Handle a markup construct starting at `self.pos` (which points at `<`).
    /// Returns `false` if the `<` is not actually a tag start.
    fn handle_markup(&mut self) -> bool {
        let src = self.src;
        let rest = &src[self.pos..];

        if rest.starts_with(b"<!--") {
            let end = find_subsequence(src, self.pos + 4, b"-->")
                .map(|index| index + 3)
                .unwrap_or(src.len());
            self.emit_verbatim(&src[self.pos..end]);
            self.pos = end;
            return true;
        }
        if rest.starts_with(b"<!") {
            let end = scan_tag_close(src, self.pos).unwrap_or(src.len());
            self.emit_verbatim(&src[self.pos..end]);
            self.pos = end;
            return true;
        }
        if rest.starts_with(b"<?") {
            let end = find_subsequence(src, self.pos + 2, b"?>")
                .map(|index| index + 2)
                .or_else(|| scan_tag_close(src, self.pos))
                .unwrap_or(src.len());
            self.emit_verbatim(&src[self.pos..end]);
            self.pos = end;
            return true;
        }
        if rest.starts_with(b"</") {
            let end = scan_tag_close(src, self.pos).unwrap_or(src.len());
            let raw = &src[self.pos..end];
            if let Some(name) = end_tag_name(raw) {
                self.handle_end_tag(name.to_ascii_lowercase());
            }
            self.emit_verbatim(raw);
            self.pos = end;
            return true;
        }
        if let Some(name_len) = start_tag_name_len(rest) {
            let end = scan_tag_close(src, self.pos).unwrap_or(src.len());
            let raw = &src[self.pos..end];
            let name = rest[1..1 + name_len].to_ascii_lowercase();
            let self_closing = raw.ends_with(b"/>");
            let self_contained = self_closing || is_void_tag_bytes(&name);

            // Apply optional-close rules before emitting so a sibling start tag
            // (e.g. a second `<li>`) is indented at the parent's depth, not one
            // level too deep. This only adjusts the indent stack; it never
            // emits or drops tags. Self-contained tags like `<br>` do not
            // start a new block and should not implicitly close `<p>`.
            if !self_contained {
                self.implicit_close(&name);
            }

            if RAW_TEXT_TAGS
                .iter()
                .any(|tag| tag.as_bytes() == name.as_slice())
                && !self_contained
            {
                self.emit_raw_element(&name, raw);
                return true;
            }

            if !self_contained && self.try_inline_leaf(&name, raw) {
                return true;
            }

            self.emit_verbatim(raw);
            self.pos = end;

            if !self_contained {
                self.stack.push(name);
                self.depth += 1;
            }
            return true;
        }
        false
    }

    fn handle_end_tag(&mut self, name: Vec<u8>) {
        if let Some(position) = self
            .stack
            .iter()
            .rposition(|open| open.eq_ignore_ascii_case(&name))
        {
            let removed = self.stack.len() - position;
            self.stack.truncate(position);
            self.depth = self.depth.saturating_sub(removed);
        }
        // Stray close tags with no matching open are emitted verbatim but do
        // not change the indent stack.
    }

    /// Emit an entire raw-text element (`<script>...</script>`, `<pre>...</pre>`)
    /// as one verbatim unit so internal whitespace is preserved exactly.
    fn emit_raw_element(&mut self, name: &[u8], start_raw: &[u8]) {
        let body_start = self.pos + start_raw.len();
        let close_end = match find_close_tag(self.src, body_start, name) {
            Some(index) => index + close_tag_len(self.src, index, name),
            None => self.src.len(),
        };

        // Indent only the start tag line; content and end tag keep their
        // original bytes (and original internal newlines).
        self.push_indent();
        self.out.extend_from_slice(start_raw);
        self.out.extend_from_slice(&self.src[body_start..close_end]);
        self.out.push(b'\n');
        self.pos = close_end;
    }

    /// Apply HTML optional-close rules before pushing a new start tag. This
    /// only adjusts the indent stack; it never emits or drops tags.
    fn implicit_close(&mut self, new: &[u8]) {
        while let Some(top) = self.stack.last() {
            if implicit_close(top, new) {
                self.stack.pop();
                self.depth = self.depth.saturating_sub(1);
            } else {
                break;
            }
        }
    }

    /// If the element starting at `start_raw` contains only text (no child
    /// tags) and no newlines until its matching close, emit the whole
    /// `<start>text</close>` on one line. This keeps leaf elements like
    /// `<h1>Hello</h1>` or `<span>JSON</span>` readable instead of spreading
    /// short text across three lines. Returns false for anything that is not a
    /// pure-text leaf, so the normal block path handles it.
    fn try_inline_leaf(&mut self, name: &[u8], start_raw: &[u8]) -> bool {
        let after = self.pos + start_raw.len();
        let Some(lt) = next_lt(self.src, after) else {
            return false;
        };
        let rest = &self.src[lt..];
        if !rest.starts_with(b"</") {
            return false;
        }
        let after_slash = &rest[2..];
        if after_slash.len() < name.len() {
            return false;
        }
        if !after_slash[..name.len()].eq_ignore_ascii_case(name) {
            return false;
        }
        if after_slash
            .get(name.len())
            .is_some_and(|byte| is_name_byte(*byte))
        {
            return false;
        }
        let text = &self.src[after..lt];
        if memchr::memchr(b'\n', text).is_some() {
            return false;
        }
        let Some(close_end) = scan_tag_close(self.src, lt) else {
            return false;
        };
        let close_raw = &self.src[lt..close_end];
        self.push_indent();
        self.out.extend_from_slice(start_raw);
        self.out.extend_from_slice(text);
        self.out.extend_from_slice(close_raw);
        self.out.push(b'\n');
        self.pos = close_end;
        true
    }

    fn emit_text(&mut self, text: &[u8]) {
        if text.is_empty() {
            return;
        }
        // Whitespace-only text that contains a newline is source formatting
        // between block elements; drop it. A whitespace-only node with no
        // newline (e.g. a single space between inline elements) is content and
        // is preserved below.
        let all_whitespace = text.iter().all(|byte| byte.is_ascii_whitespace());
        if all_whitespace && memchr::memchr(b'\n', text).is_some() {
            return;
        }

        // Trim only the leading/trailing whitespace runs that contain a
        // newline (source re-indent noise). Edge spaces without a newline are
        // significant inline content (e.g. the space in `Hello <b>world</b>`)
        // and must be preserved byte-for-byte.
        let leading_ws = text
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        let start = if memchr::memchr(b'\n', &text[..leading_ws]).is_some() {
            leading_ws
        } else {
            0
        };
        let trailing_ws = text
            .iter()
            .rev()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        let trailing_region = &text[text.len() - trailing_ws..];
        let end = if memchr::memchr(b'\n', trailing_region).is_some() {
            text.len() - trailing_ws
        } else {
            text.len()
        };

        if end <= start {
            return;
        }
        self.push_indent();
        self.out.extend_from_slice(&text[start..end]);
        self.out.push(b'\n');
    }

    fn emit_verbatim(&mut self, bytes: &[u8]) {
        self.push_indent();
        self.out.extend_from_slice(bytes);
        self.out.push(b'\n');
    }

    fn push_indent(&mut self) {
        for _ in 0..self.depth.saturating_mul(self.indent) {
            self.out.push(b' ');
        }
    }
}

fn next_lt(src: &[u8], from: usize) -> Option<usize> {
    memchr::memchr(b'<', &src[from..]).map(|index| from + index)
}

fn find_subsequence(src: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= src.len() {
        return None;
    }
    src[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|index| from + index)
}

/// Find the closing `>` of a tag starting at `start` (the `<`), respecting
/// single- and double-quoted attribute values.
fn scan_tag_close(src: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut quote = None;
    while index < src.len() {
        let byte = src[index];
        match quote {
            Some(q) if byte == q => quote = None,
            Some(_) => {}
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(index + 1),
                _ => {}
            },
        }
        index += 1;
    }
    None
}

fn start_tag_name_len(rest: &[u8]) -> Option<usize> {
    let first = rest.get(1).copied()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut len = 1;
    while let Some(&byte) = rest.get(1 + len) {
        if is_name_byte(byte) {
            len += 1;
        } else {
            break;
        }
    }
    Some(len)
}

fn end_tag_name(raw: &[u8]) -> Option<&[u8]> {
    let inner = raw.strip_prefix(b"</")?;
    let end = inner
        .iter()
        .position(|byte| !is_name_byte(*byte))
        .unwrap_or(inner.len());
    (end > 0).then_some(&inner[..end])
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
}

fn is_void_tag_bytes(name: &[u8]) -> bool {
    crate::formats::markup::VOID_TAGS
        .iter()
        .any(|tag| tag.as_bytes().eq_ignore_ascii_case(name))
}

/// Find the index of `</name` (case-insensitive) starting from `from`.
fn find_close_tag(src: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    if from >= src.len() {
        return None;
    }
    let mut search = from;
    while let Some(index) = memchr::memchr(b'<', &src[search..]) {
        let position = search + index;
        let rest = &src[position..];
        if let Some(after) = rest.strip_prefix(b"</") {
            if after
                .iter()
                .take(name.len())
                .copied()
                .eq(name.iter().copied().map(|b| b.to_ascii_lowercase()))
                && after
                    .get(name.len())
                    .is_some_and(|byte| !is_name_byte(*byte))
            {
                return Some(position);
            }
        }
        search = position + 1;
    }
    None
}

/// Length of the close tag `</name ...>` starting at `position`.
fn close_tag_len(src: &[u8], position: usize, name: &[u8]) -> usize {
    let after = position + 2 + name.len();
    match memchr::memchr(b'>', &src[after..]) {
        Some(index) => after + index + 1 - position,
        None => src.len() - position,
    }
}

/// HTML optional-close rules: does starting `new` implicitly close an open
/// `top`? Covers the common `<p>`, `<li>`, `<td>`/`<th>`, `<tr>`, `<option>`,
/// `<dd>`/`<dt>`, and table section cases. Anything more spec-accurate than
/// this is out of scope for a viewer pretty-printer.
fn implicit_close(top: &[u8], new: &[u8]) -> bool {
    // A new block-level start closes an open <p>.
    if top == b"p" && is_block_start(new) {
        return true;
    }
    if top == b"li" && new == b"li" {
        return true;
    }
    if matches!(top, b"td" | b"th") && matches!(new, b"td" | b"th" | b"tr") {
        return true;
    }
    if top == b"tr" && matches!(new, b"tr" | b"thead" | b"tbody" | b"tfoot") {
        return true;
    }
    if top == b"option" && (new == b"option" || new == b"optgroup") {
        return true;
    }
    if matches!(top, b"dd" | b"dt") && matches!(new, b"dd" | b"dt") {
        return true;
    }
    if matches!(top, b"thead" | b"tbody" | b"tfoot")
        && matches!(new, b"thead" | b"tbody" | b"tfoot")
    {
        return true;
    }
    false
}

fn is_block_start(name: &[u8]) -> bool {
    matches!(
        name,
        b"address"
            | b"article"
            | b"aside"
            | b"blockquote"
            | b"details"
            | b"div"
            | b"dl"
            | b"fieldset"
            | b"figcaption"
            | b"figure"
            | b"footer"
            | b"form"
            | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"header"
            | b"hr"
            | b"main"
            | b"menu"
            | b"nav"
            | b"ol"
            | b"p"
            | b"pre"
            | b"section"
            | b"table"
            | b"ul"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(input: &str) -> String {
        let mut output = Vec::new();
        format_html_reader(std::io::Cursor::new(input.as_bytes()), &mut output, 2).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn formats_void_elements_without_closing() {
        let out = format("<div><br><img src=x></div>");
        assert!(out.contains("<br>"), "void <br> must be preserved: {out}");
        assert!(
            out.contains(r#"<img src=x>"#),
            "unquoted attribute must be preserved: {out}"
        );
        assert!(
            !out.contains("<br/>"),
            "void must not be rewritten to self-closing: {out}"
        );
    }

    #[test]
    fn indents_nested_elements() {
        let out = format("<div><p>hello</p></div>");
        let lines: Vec<&str> = out.trim_end().lines().collect();
        assert_eq!(lines, vec!["<div>", "  <p>hello</p>", "</div>"]);
    }

    #[test]
    fn preserves_inline_text_whitespace() {
        // The space between `Hello` and `<b>` is significant inline content;
        // it must not be trimmed away.
        let out = format("<p>Hello <b>world</b></p>");
        assert!(
            out.contains("Hello \n"),
            "trailing space before inline element must be preserved: {out:?}"
        );
        assert!(
            out.contains("<b>world</b>"),
            "inline leaf must stay on one line: {out:?}"
        );
    }

    #[test]
    fn preserves_space_between_inline_siblings() {
        let out = format("<p><a>x</a> <a>y</a></p>");
        let lines: Vec<&str> = out.trim_end_matches('\n').lines().collect();
        assert_eq!(
            lines,
            vec!["<p>", "  <a>x</a>", "   ", "  <a>y</a>", "</p>"],
            "single space between inline siblings must be preserved: {out:?}"
        );
    }

    #[test]
    fn optional_close_siblings_share_indent() {
        let out = format("<ul><li>one<li>two</ul>");
        let lines: Vec<&str> = out.trim_end().lines().collect();
        assert_eq!(
            lines,
            vec!["<ul>", "  <li>", "    one", "  <li>", "    two", "</ul>"],
            "sibling <li> items must share indent without synthesizing close tags: {out:?}"
        );
    }

    #[test]
    fn void_tag_inside_paragraph_does_not_close_paragraph() {
        let out = format("<p>a<br>b</p>");
        let lines: Vec<&str> = out.trim_end().lines().collect();
        assert_eq!(
            lines,
            vec!["<p>", "  a", "  <br>", "  b", "</p>"],
            "void tags should not implicitly close their containing paragraph: {out:?}"
        );
    }

    #[test]
    fn drops_newline_formatting_whitespace_only_nodes() {
        // The `\n  ` between block elements is source formatting noise.
        let out = format("<ul>\n  <li>a</li>\n</ul>");
        let lines: Vec<&str> = out.trim_end().lines().collect();
        assert_eq!(lines, vec!["<ul>", "  <li>a</li>", "</ul>"]);
    }

    #[test]
    fn handles_unclosed_optional_tags() {
        let out = format("<ul><li>one<li>two</ul>");
        // Both <li> items remain on the page; second does not nest under first.
        let one = out.find("one").unwrap();
        let two = out.find("two").unwrap();
        let one_indent = out[..one].lines().next_back().unwrap().len() - "one".len();
        let two_indent = out[..two].lines().next_back().unwrap().len() - "two".len();
        assert_eq!(one_indent, two_indent, "siblings must share indent: {out}");
    }

    #[test]
    fn preserves_script_content_verbatim() {
        let out = format("<div><script>if (a < b) { x(); }</script></div>");
        assert!(
            out.contains("if (a < b) { x(); }"),
            "script content must be preserved verbatim: {out}"
        );
    }

    #[test]
    fn preserves_pre_whitespace() {
        let out = format("<pre>  line one\n  line two</pre>");
        assert!(
            out.contains("  line one\n  line two"),
            "pre interior whitespace must be preserved: {out}"
        );
    }

    #[test]
    fn handles_doctype_and_comments() {
        let out = format("<!doctype html>\n<!-- comment --><html></html>");
        assert!(out.contains("<!doctype html>"), "doctype preserved: {out}");
        assert!(out.contains("<!-- comment -->"), "comment preserved: {out}");
    }

    #[test]
    fn handles_attribute_with_gt_inside_quotes() {
        let out = format(r#"<a title="x > y">link</a>"#);
        assert!(
            out.contains(r#"<a title="x > y">"#),
            "quoted > must not end the tag early: {out}"
        );
    }

    #[test]
    fn stray_close_tag_does_not_crash() {
        let out = format("<div>text</span></div>");
        assert!(out.contains("</span>"), "stray close preserved: {out}");
        assert!(out.contains("</div>"));
    }

    #[test]
    fn keeps_original_attribute_text() {
        let out = format(r#"<input type=text disabled value='a "b" c'>"#);
        assert!(
            out.contains(r#"<input type=text disabled value='a "b" c'>"#),
            "attribute text must be byte-preserved: {out}"
        );
    }
}
