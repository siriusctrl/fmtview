use std::io::{BufRead, BufReader, Cursor, Write};

use anyhow::{Context, Result, anyhow, bail};

use crate::input::InputSource;

use super::types::FormatOptions;

pub(super) fn format_json<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    format_json_value(BufReader::new(source.open()?), output, options.indent)
        .context("failed to parse JSON")?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn format_jsonl<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    let mut reader = BufReader::new(source.open()?);
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
pub(super) fn format_json_value<R: BufRead, W: Write>(
    input: R,
    output: &mut W,
    indent: usize,
) -> Result<()> {
    let mut formatter = JsonFormatter {
        input,
        indent: vec![b' '; indent],
        offset: 0,
    };
    formatter.format(output)
}

struct JsonFormatter<R> {
    input: R,
    indent: Vec<u8>,
    offset: usize,
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
        self.expect_byte(b'{')?;
        output.write_all(b"{")?;
        self.skip_ws()?;
        if self.consume_if(b'}')? {
            output.write_all(b"}")?;
            return Ok(());
        }

        self.write_pretty_object(output, depth)
    }

    fn write_pretty_object<W: Write>(&mut self, output: &mut W, depth: usize) -> Result<()> {
        loop {
            output.write_all(b"\n")?;
            self.write_indent(output, depth + 1)?;
            self.skip_ws()?;
            self.write_string(output)?;
            self.skip_ws()?;
            self.expect_byte(b':')?;
            output.write_all(b": ")?;
            self.write_value(output, depth + 1)?;

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

pub(super) fn trim_line_end(mut line: &[u8]) -> &[u8] {
    if line.ends_with(b"\n") {
        line = &line[..line.len() - 1];
    }
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    line
}
