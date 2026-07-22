use std::io::{BufRead, BufReader, Cursor, Write};

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    input::InputSource,
    transform::{FormatOptions, IO_BUFFER_BYTES},
};

pub(crate) fn format_json<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    format_json_value(
        BufReader::with_capacity(IO_BUFFER_BYTES, source.open()?),
        output,
        options.indent,
    )
    .context("failed to parse JSON")?;
    writeln!(output)?;
    Ok(())
}

pub(crate) fn format_jsonl<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, source.open()?);
    let mut line = Vec::with_capacity(8192);
    let mut line_number = 0_usize;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .context("failed to read JSONL line")?;
        if read == 0 {
            break;
        }

        line_number += 1;
        let trimmed = trim_line_end(&line);
        if trimmed.iter().all(u8::is_ascii_whitespace) {
            writeln!(output)?;
            continue;
        }

        format_json_value(Cursor::new(trimmed), output, options.indent)
            .with_context(|| format!("failed to parse JSONL line {line_number}"))?;
        writeln!(output)?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum JsonSeparator {
    Comma,
    End,
}

// Token-based formatting keeps JSON numbers exact without materializing the
// whole document or coercing through native numeric types.
pub(crate) fn format_json_value<R: BufRead, W: Write>(
    input: R,
    output: &mut W,
    indent: usize,
) -> Result<()> {
    let mut formatter = JsonFormatter {
        input,
        indent: vec![b' '; indent],
        offset: 0,
        collapse_media: false,
    };
    formatter.format(output)
}

pub(crate) fn format_json_value_for_view<R: BufRead, W: Write>(
    input: R,
    output: &mut W,
    indent: usize,
) -> Result<()> {
    let mut formatter = JsonFormatter {
        input,
        indent: vec![b' '; indent],
        offset: 0,
        collapse_media: true,
    };
    formatter.format(output)
}

struct JsonFormatter<R> {
    input: R,
    indent: Vec<u8>,
    offset: usize,
    collapse_media: bool,
}

#[derive(Default)]
struct ObjectMediaContext {
    media_type: Option<String>,
    inherited_encoding: Option<Base64Encoding>,
    object_type: Option<MediaObjectType>,
    encoding_field: Option<Option<Base64Encoding>>,
}

struct StringPrefix {
    bytes: Vec<u8>,
    ended: bool,
}

struct Base64Stats {
    encoding: Base64Encoding,
    data: usize,
    padding: usize,
    invalid: bool,
    saw_padding: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base64Encoding {
    Standard,
    UrlSafe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaObjectType {
    Encoded(Base64Encoding),
    Image,
    Other,
}

impl ObjectMediaContext {
    fn encoding(&self) -> Option<Base64Encoding> {
        self.encoding_field.unwrap_or(match self.object_type {
            Some(MediaObjectType::Encoded(encoding)) => Some(encoding),
            Some(MediaObjectType::Image) => Some(Base64Encoding::Standard),
            Some(MediaObjectType::Other) => None,
            None => self.inherited_encoding,
        })
    }

    fn is_typed_image(&self) -> bool {
        self.object_type == Some(MediaObjectType::Image)
    }
}

impl<R: BufRead> JsonFormatter<R> {
    fn format<W: Write>(&mut self, output: &mut W) -> Result<()> {
        self.skip_ws()?;
        self.write_value(output, 0)?;
        self.skip_ws()?;
        if self.peek_byte()?.is_some() {
            bail!("trailing characters after JSON at byte {}", self.offset);
        }
        Ok(())
    }

    fn write_value<W: Write>(&mut self, output: &mut W, depth: usize) -> Result<()> {
        self.skip_ws()?;
        match self.peek_byte()? {
            Some(b'{') => self.write_object(output, depth),
            Some(b'[') => self.write_array(output, depth),
            Some(b'"') => self.write_string(output),
            Some(b't') => self.write_literal(output, b"true"),
            Some(b'f') => self.write_literal(output, b"false"),
            Some(b'n') => self.write_literal(output, b"null"),
            Some(b'-' | b'0'..=b'9') => self.write_number(output),
            Some(byte) => bail!(
                "expected JSON value at byte {}, found {}",
                self.offset,
                describe_byte(byte)
            ),
            None => bail!("expected JSON value at end of input"),
        }
    }

    fn write_object<W: Write>(&mut self, output: &mut W, depth: usize) -> Result<()> {
        self.write_object_with_media_hint(output, depth, None)
    }

    fn write_object_with_media_hint<W: Write>(
        &mut self,
        output: &mut W,
        depth: usize,
        inherited_encoding: Option<Base64Encoding>,
    ) -> Result<()> {
        self.expect_byte(b'{')?;
        output.write_all(b"{")?;
        self.skip_ws()?;
        if self.consume_if(b'}')? {
            output.write_all(b"}")?;
            return Ok(());
        }

        self.write_pretty_object(output, depth, inherited_encoding)
    }

    fn write_pretty_object<W: Write>(
        &mut self,
        output: &mut W,
        depth: usize,
        inherited_encoding: Option<Base64Encoding>,
    ) -> Result<()> {
        let mut media = ObjectMediaContext {
            inherited_encoding,
            ..ObjectMediaContext::default()
        };
        loop {
            output.write_all(b"\n")?;
            self.write_indent(output, depth + 1)?;
            self.skip_ws()?;
            let key = self.write_captured_string(output, 64)?;
            self.skip_ws()?;
            self.expect_byte(b':')?;
            output.write_all(b": ")?;
            if self.collapse_media && self.peek_byte()? == Some(b'"') {
                match key.as_deref() {
                    Some("media_type" | "mime_type") => {
                        let value = self.write_captured_string(output, 128)?;
                        media.media_type = value.filter(|value| valid_media_type(value));
                    }
                    Some("type") => {
                        let value = self.write_captured_string(output, 32)?;
                        media.object_type = value.as_deref().map(media_object_type);
                    }
                    Some("encoding") => {
                        let value = self.write_captured_string(output, 32)?;
                        media.encoding_field = Some(value.as_deref().and_then(base64_encoding));
                    }
                    key => self.write_string_for_view(output, key, &media)?,
                }
            } else {
                let typed_attachment = self.collapse_media
                    && key.as_deref() == Some("attachment")
                    && media.is_typed_image()
                    && self.peek_byte()? == Some(b'{');
                if typed_attachment {
                    self.write_object_with_media_hint(
                        output,
                        depth + 1,
                        Some(Base64Encoding::Standard),
                    )?;
                } else {
                    self.write_value(output, depth + 1)?;
                }
            }

            match self.read_separator(b'}')? {
                JsonSeparator::Comma => output.write_all(b",")?,
                JsonSeparator::End => {
                    output.write_all(b"\n")?;
                    self.write_indent(output, depth)?;
                    output.write_all(b"}")?;
                    return Ok(());
                }
            }
        }
    }

    fn write_array<W: Write>(&mut self, output: &mut W, depth: usize) -> Result<()> {
        self.expect_byte(b'[')?;
        output.write_all(b"[")?;
        self.skip_ws()?;
        if self.consume_if(b']')? {
            output.write_all(b"]")?;
            return Ok(());
        }

        self.write_pretty_array(output, depth)
    }

    fn write_pretty_array<W: Write>(&mut self, output: &mut W, depth: usize) -> Result<()> {
        loop {
            output.write_all(b"\n")?;
            self.write_indent(output, depth + 1)?;
            self.write_value(output, depth + 1)?;

            match self.read_separator(b']')? {
                JsonSeparator::Comma => output.write_all(b",")?,
                JsonSeparator::End => {
                    output.write_all(b"\n")?;
                    self.write_indent(output, depth)?;
                    output.write_all(b"]")?;
                    return Ok(());
                }
            }
        }
    }

    fn write_string<W: Write>(&mut self, output: &mut W) -> Result<()> {
        self.expect_byte(b'"')?;
        output.write_all(b"\"")?;
        let mut utf8 = Utf8Validator::default();

        loop {
            if utf8.is_idle() && self.write_safe_ascii_string_span(output)? {
                continue;
            }

            let byte = self.next_required("unterminated JSON string")?;
            output.write_all(&[byte])?;
            match byte {
                b'"' => {
                    utf8.finish()?;
                    return Ok(());
                }
                b'\\' => {
                    let escaped = self.next_required("unterminated JSON string escape")?;
                    output.write_all(&[escaped])?;
                    match escaped {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                let hex = self.next_required("unterminated unicode escape")?;
                                if !hex.is_ascii_hexdigit() {
                                    bail!("invalid unicode escape digit {}", describe_byte(hex));
                                }
                                output.write_all(&[hex])?;
                            }
                        }
                        _ => bail!("invalid JSON string escape {}", describe_byte(escaped)),
                    }
                }
                0x00..=0x1f => bail!("unescaped control byte in JSON string"),
                _ => utf8.accept(byte)?,
            }
        }
    }

    fn write_captured_string<W: Write>(
        &mut self,
        output: &mut W,
        max_bytes: usize,
    ) -> Result<Option<String>> {
        self.expect_byte(b'"')?;
        output.write_all(b"\"")?;
        let mut capture = Some(Vec::with_capacity(max_bytes.min(32)));
        let mut utf8 = Utf8Validator::default();

        loop {
            let byte = self.next_required("unterminated JSON string")?;
            output.write_all(&[byte])?;
            match byte {
                b'"' => {
                    utf8.finish()?;
                    return Ok(capture.and_then(|bytes| String::from_utf8(bytes).ok()));
                }
                b'\\' => {
                    let escaped = self.next_required("unterminated JSON string escape")?;
                    output.write_all(&[escaped])?;
                    match escaped {
                        b'"' => push_capture_byte(&mut capture, b'"', max_bytes),
                        b'\\' => push_capture_byte(&mut capture, b'\\', max_bytes),
                        b'/' => push_capture_byte(&mut capture, b'/', max_bytes),
                        b'b' => push_capture_byte(&mut capture, 0x08, max_bytes),
                        b'f' => push_capture_byte(&mut capture, 0x0c, max_bytes),
                        b'n' => push_capture_byte(&mut capture, b'\n', max_bytes),
                        b'r' => push_capture_byte(&mut capture, b'\r', max_bytes),
                        b't' => push_capture_byte(&mut capture, b'\t', max_bytes),
                        b'u' => {
                            let mut value = 0_u32;
                            for _ in 0..4 {
                                let hex = self.next_required("unterminated unicode escape")?;
                                if !hex.is_ascii_hexdigit() {
                                    bail!("invalid unicode escape digit {}", describe_byte(hex));
                                }
                                output.write_all(&[hex])?;
                                value = value
                                    .checked_mul(16)
                                    .and_then(|value| value.checked_add(hex_value(hex)))
                                    .expect("four hex digits fit in u32");
                            }
                            if let Some(ch) = char::from_u32(value)
                                && !(0xd800..=0xdfff).contains(&value)
                            {
                                let mut encoded = [0_u8; 4];
                                push_capture_bytes(
                                    &mut capture,
                                    ch.encode_utf8(&mut encoded).as_bytes(),
                                    max_bytes,
                                );
                            } else {
                                capture = None;
                            }
                        }
                        _ => bail!("invalid JSON string escape {}", describe_byte(escaped)),
                    }
                }
                0x00..=0x1f => bail!("unescaped control byte in JSON string"),
                byte if byte.is_ascii() => {
                    if let Some(bytes) = capture.as_mut() {
                        if bytes.len() < max_bytes {
                            bytes.push(byte);
                        } else {
                            capture = None;
                        }
                    }
                }
                _ => {
                    capture = None;
                    utf8.accept(byte)?;
                }
            }
        }
    }

    fn write_string_for_view<W: Write>(
        &mut self,
        output: &mut W,
        key: Option<&str>,
        media: &ObjectMediaContext,
    ) -> Result<()> {
        self.expect_byte(b'"')?;
        let prefix = self.read_safe_ascii_prefix(256)?;
        let data_uri = data_uri_header(&prefix.bytes);
        let sibling_media =
            key == Some("data") && media.media_type.is_some() && media.encoding().is_some();

        if key != Some("arguments")
            && let Some((media_type, payload_start)) = data_uri
        {
            return self.write_media_summary(
                output,
                media_type,
                Base64Encoding::Standard,
                &prefix.bytes[payload_start..],
                prefix.ended,
            );
        }
        if sibling_media {
            return self.write_media_summary(
                output,
                media
                    .media_type
                    .as_deref()
                    .expect("sibling media type checked"),
                media.encoding().expect("sibling media encoding checked"),
                &prefix.bytes,
                prefix.ended,
            );
        }

        output.write_all(b"\"")?;
        output.write_all(&prefix.bytes)?;
        if prefix.ended {
            output.write_all(b"\"")?;
            return Ok(());
        }
        self.write_open_string_tail(output)
    }

    fn read_safe_ascii_prefix(&mut self, limit: usize) -> Result<StringPrefix> {
        let mut bytes = Vec::with_capacity(limit.min(64));
        while bytes.len() < limit {
            match self.peek_byte()? {
                Some(b'"') => {
                    self.next_byte()?;
                    return Ok(StringPrefix { bytes, ended: true });
                }
                Some(byte) if is_safe_json_string_ascii(byte) => {
                    self.next_byte()?;
                    bytes.push(byte);
                }
                Some(_) => break,
                None => bail!("unterminated JSON string"),
            }
        }
        Ok(StringPrefix {
            bytes,
            ended: false,
        })
    }

    fn write_open_string_tail<W: Write>(&mut self, output: &mut W) -> Result<()> {
        let mut utf8 = Utf8Validator::default();
        loop {
            if utf8.is_idle() && self.write_safe_ascii_string_span(output)? {
                continue;
            }
            let byte = self.next_required("unterminated JSON string")?;
            output.write_all(&[byte])?;
            match byte {
                b'"' => {
                    utf8.finish()?;
                    return Ok(());
                }
                b'\\' => {
                    let escaped = self.next_required("unterminated JSON string escape")?;
                    output.write_all(&[escaped])?;
                    match escaped {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                let hex = self.next_required("unterminated unicode escape")?;
                                if !hex.is_ascii_hexdigit() {
                                    bail!("invalid unicode escape digit {}", describe_byte(hex));
                                }
                                output.write_all(&[hex])?;
                            }
                        }
                        _ => bail!("invalid JSON string escape {}", describe_byte(escaped)),
                    }
                }
                0x00..=0x1f => bail!("unescaped control byte in JSON string"),
                _ => utf8.accept(byte)?,
            }
        }
    }

    fn write_media_summary<W: Write>(
        &mut self,
        output: &mut W,
        media_type: &str,
        encoding: Base64Encoding,
        prefix_payload: &[u8],
        ended: bool,
    ) -> Result<()> {
        let mut stats = Base64Stats::new(encoding);
        for &byte in prefix_payload {
            stats.accept(byte);
        }
        if !ended {
            self.scan_base64_string_tail(&mut stats)?;
        }

        match stats.decoded_bytes() {
            Some(bytes) => write!(output, "\"<media {media_type}; {bytes} decoded bytes>\"")?,
            None => write!(output, "\"<media {media_type}; invalid base64>\"")?,
        }
        Ok(())
    }

    fn scan_base64_string_tail(&mut self, stats: &mut Base64Stats) -> Result<()> {
        let mut utf8 = Utf8Validator::default();
        loop {
            if utf8.is_idle() {
                let len = {
                    let buffer = self.input.fill_buf().context("failed to read JSON input")?;
                    let len = buffer
                        .iter()
                        .position(|byte| {
                            *byte == b'"' || *byte == b'\\' || *byte < 0x20 || *byte >= 0x80
                        })
                        .unwrap_or(buffer.len());
                    stats.accept_slice(&buffer[..len]);
                    len
                };
                if len > 0 {
                    self.input.consume(len);
                    self.offset = self.offset.saturating_add(len);
                    continue;
                }
            }

            let byte = self.next_required("unterminated JSON string")?;
            match byte {
                b'"' if utf8.is_idle() => return Ok(()),
                b'\\' if utf8.is_idle() => match self.consume_string_escape_ascii()? {
                    Some(byte) => stats.accept(byte),
                    None => stats.invalid = true,
                },
                0x00..=0x1f if utf8.is_idle() => {
                    bail!("unescaped control byte in JSON string")
                }
                _ => {
                    stats.invalid = true;
                    utf8.accept(byte)?;
                }
            }
        }
    }

    fn consume_string_escape_ascii(&mut self) -> Result<Option<u8>> {
        let escaped = self.next_required("unterminated JSON string escape")?;
        match escaped {
            b'"' => Ok(Some(b'"')),
            b'\\' => Ok(Some(b'\\')),
            b'/' => Ok(Some(b'/')),
            b'b' => Ok(Some(0x08)),
            b'f' => Ok(Some(0x0c)),
            b'n' => Ok(Some(b'\n')),
            b'r' => Ok(Some(b'\r')),
            b't' => Ok(Some(b'\t')),
            b'u' => {
                let mut value = 0_u32;
                for _ in 0..4 {
                    let hex = self.next_required("unterminated unicode escape")?;
                    if !hex.is_ascii_hexdigit() {
                        bail!("invalid unicode escape digit {}", describe_byte(hex));
                    }
                    value = value
                        .checked_mul(16)
                        .and_then(|value| value.checked_add(hex_value(hex)))
                        .expect("four hex digits fit in u32");
                }
                Ok(u8::try_from(value).ok())
            }
            _ => bail!("invalid JSON string escape {}", describe_byte(escaped)),
        }
    }

    fn write_safe_ascii_string_span<W: Write>(&mut self, output: &mut W) -> Result<bool> {
        let len = {
            let buffer = self.input.fill_buf().context("failed to read JSON input")?;
            let len = buffer
                .iter()
                .position(|byte| !is_safe_json_string_ascii(*byte))
                .unwrap_or(buffer.len());
            if len == 0 {
                return Ok(false);
            }
            output.write_all(&buffer[..len])?;
            len
        };
        self.input.consume(len);
        self.offset += len;
        Ok(true)
    }

    fn write_number<W: Write>(&mut self, output: &mut W) -> Result<()> {
        if self.consume_if(b'-')? {
            output.write_all(b"-")?;
        }

        match self.next_required("unexpected end of input while parsing JSON number")? {
            b'0' => output.write_all(b"0")?,
            byte @ b'1'..=b'9' => {
                output.write_all(&[byte])?;
                self.write_digits(output, false)?;
            }
            byte => bail!("invalid JSON number digit {}", describe_byte(byte)),
        }

        if self.consume_if(b'.')? {
            output.write_all(b".")?;
            self.write_digits(output, true)?;
        }

        if let Some(exponent @ (b'e' | b'E')) = self.peek_byte()? {
            self.next_byte()?;
            output.write_all(&[exponent])?;
            if let Some(sign @ (b'+' | b'-')) = self.peek_byte()? {
                self.next_byte()?;
                output.write_all(&[sign])?;
            }
            self.write_digits(output, true)?;
        }

        Ok(())
    }

    fn write_digits<W: Write>(&mut self, output: &mut W, require_one: bool) -> Result<()> {
        let mut count = 0_usize;
        while let Some(byte @ b'0'..=b'9') = self.peek_byte()? {
            self.next_byte()?;
            output.write_all(&[byte])?;
            count += 1;
        }

        if require_one && count == 0 {
            bail!("expected digit in JSON number");
        }
        Ok(())
    }

    fn write_literal<W: Write>(&mut self, output: &mut W, literal: &[u8]) -> Result<()> {
        for &expected in literal {
            let byte = self.next_required("unexpected end of input while parsing JSON literal")?;
            if byte != expected {
                bail!(
                    "invalid JSON literal at byte {}, expected {}, found {}",
                    self.offset - 1,
                    describe_byte(expected),
                    describe_byte(byte)
                );
            }
        }
        output.write_all(literal)?;
        Ok(())
    }

    fn read_separator(&mut self, end: u8) -> Result<JsonSeparator> {
        self.skip_ws()?;
        match self.next_required("unexpected end of input after JSON value")? {
            b',' => Ok(JsonSeparator::Comma),
            byte if byte == end => Ok(JsonSeparator::End),
            byte => bail!(
                "expected ',' or {}, found {}",
                describe_byte(end),
                describe_byte(byte)
            ),
        }
    }

    fn write_indent<W: Write>(&self, output: &mut W, depth: usize) -> Result<()> {
        for _ in 0..depth {
            output.write_all(&self.indent)?;
        }
        Ok(())
    }

    fn skip_ws(&mut self) -> Result<()> {
        while matches!(self.peek_byte()?, Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.next_byte()?;
        }
        Ok(())
    }

    fn consume_if(&mut self, expected: u8) -> Result<bool> {
        if self.peek_byte()? == Some(expected) {
            self.next_byte()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<()> {
        let byte = self.next_required("unexpected end of input")?;
        if byte != expected {
            bail!(
                "expected {}, found {}",
                describe_byte(expected),
                describe_byte(byte)
            );
        }
        Ok(())
    }

    fn next_required(&mut self, message: &str) -> Result<u8> {
        self.next_byte()?.ok_or_else(|| anyhow!(message.to_owned()))
    }

    fn peek_byte(&mut self) -> Result<Option<u8>> {
        let buffer = self.input.fill_buf().context("failed to read JSON input")?;
        Ok(buffer.first().copied())
    }

    fn next_byte(&mut self) -> Result<Option<u8>> {
        let byte = self.peek_byte()?;
        if byte.is_some() {
            self.input.consume(1);
            self.offset += 1;
        }
        Ok(byte)
    }
}

impl Base64Stats {
    fn new(encoding: Base64Encoding) -> Self {
        Self {
            encoding,
            data: 0,
            padding: 0,
            invalid: false,
            saw_padding: false,
        }
    }

    fn accept(&mut self, byte: u8) {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' if !self.saw_padding => {
                self.data = self.data.saturating_add(1);
            }
            b'+' | b'/' if self.encoding == Base64Encoding::Standard && !self.saw_padding => {
                self.data = self.data.saturating_add(1);
            }
            b'-' | b'_' if self.encoding == Base64Encoding::UrlSafe && !self.saw_padding => {
                self.data = self.data.saturating_add(1);
            }
            b'=' if self.padding < 2 => {
                self.padding += 1;
                self.saw_padding = true;
            }
            _ => self.invalid = true,
        }
    }

    fn decoded_bytes(&self) -> Option<usize> {
        if self.invalid {
            return None;
        }
        let total = self.data.checked_add(self.padding)?;
        if self.padding > 0 {
            if total % 4 != 0
                || (self.padding == 1 && self.data % 4 != 3)
                || (self.padding == 2 && self.data % 4 != 2)
            {
                return None;
            }
            return total
                .checked_div(4)?
                .checked_mul(3)?
                .checked_sub(self.padding);
        }
        let full = self.data.checked_div(4)?.checked_mul(3)?;
        match self.data % 4 {
            0 => Some(full),
            2 => full.checked_add(1),
            3 => full.checked_add(2),
            _ => None,
        }
    }

    fn accept_slice(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.accept(byte);
        }
    }
}

fn data_uri_header(prefix: &[u8]) -> Option<(&str, usize)> {
    let header_end = prefix
        .windows(8)
        .position(|window| window.eq_ignore_ascii_case(b";base64,"))?;
    if !prefix.get(..5)?.eq_ignore_ascii_case(b"data:") {
        return None;
    }
    let metadata = std::str::from_utf8(prefix.get(5..header_end)?).ok()?;
    let media_type = metadata.split(';').next()?;
    if !valid_media_type(media_type) {
        return None;
    }
    Some((media_type, header_end + 8))
}

fn valid_media_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 127
        && value.contains('/')
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                )
        })
}

fn base64_encoding(value: &str) -> Option<Base64Encoding> {
    if value.eq_ignore_ascii_case("base64") {
        Some(Base64Encoding::Standard)
    } else if value.eq_ignore_ascii_case("base64url") {
        Some(Base64Encoding::UrlSafe)
    } else {
        None
    }
}

fn media_object_type(value: &str) -> MediaObjectType {
    match base64_encoding(value) {
        Some(encoding) => MediaObjectType::Encoded(encoding),
        None if value.eq_ignore_ascii_case("image") => MediaObjectType::Image,
        None => MediaObjectType::Other,
    }
}

fn push_capture_byte(capture: &mut Option<Vec<u8>>, byte: u8, max_bytes: usize) {
    push_capture_bytes(capture, &[byte], max_bytes);
}

fn push_capture_bytes(capture: &mut Option<Vec<u8>>, bytes: &[u8], max_bytes: usize) {
    let Some(buffer) = capture.as_mut() else {
        return;
    };
    if buffer.len().saturating_add(bytes.len()) > max_bytes {
        *capture = None;
    } else {
        buffer.extend_from_slice(bytes);
    }
}

fn hex_value(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => u32::from(byte - b'0'),
        b'a'..=b'f' => u32::from(byte - b'a' + 10),
        b'A'..=b'F' => u32::from(byte - b'A' + 10),
        _ => unreachable!("caller validates ASCII hex digit"),
    }
}

#[derive(Default)]
struct Utf8Validator {
    remaining: u8,
    min_next: u8,
    max_next: u8,
}

impl Utf8Validator {
    fn is_idle(&self) -> bool {
        self.remaining == 0
    }

    fn accept(&mut self, byte: u8) -> Result<()> {
        if self.remaining == 0 {
            match byte {
                0x00..=0x7f => {}
                0xc2..=0xdf => self.expect_continuations(1, 0x80, 0xbf),
                0xe0 => self.expect_continuations(2, 0xa0, 0xbf),
                0xe1..=0xec | 0xee..=0xef => self.expect_continuations(2, 0x80, 0xbf),
                0xed => self.expect_continuations(2, 0x80, 0x9f),
                0xf0 => self.expect_continuations(3, 0x90, 0xbf),
                0xf1..=0xf3 => self.expect_continuations(3, 0x80, 0xbf),
                0xf4 => self.expect_continuations(3, 0x80, 0x8f),
                _ => bail!("invalid UTF-8 byte in JSON string"),
            }
            return Ok(());
        }

        if byte < self.min_next || byte > self.max_next {
            bail!("invalid UTF-8 continuation byte in JSON string");
        }
        self.remaining -= 1;
        self.min_next = 0x80;
        self.max_next = 0xbf;
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            bail!("unterminated UTF-8 sequence in JSON string")
        }
    }

    fn expect_continuations(&mut self, remaining: u8, min_next: u8, max_next: u8) {
        self.remaining = remaining;
        self.min_next = min_next;
        self.max_next = max_next;
    }
}

fn is_safe_json_string_ascii(byte: u8) -> bool {
    byte >= 0x20 && byte != b'"' && byte != b'\\' && byte < 0x80
}

fn describe_byte(byte: u8) -> String {
    if byte.is_ascii_graphic() || byte == b' ' {
        format!("'{}'", char::from(byte))
    } else {
        format!("0x{byte:02x}")
    }
}

pub(crate) fn trim_line_end(mut line: &[u8]) -> &[u8] {
    if line.ends_with(b"\n") {
        line = &line[..line.len() - 1];
    }
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    line
}
