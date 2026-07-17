use std::sync::Arc;

use super::{MAX_TOOL_ID_BYTES, MAX_TOOL_ID_KEY_BYTES, is_id_key};

#[derive(Debug)]
pub(super) struct JsonProperty {
    pub(super) key: String,
    pub(super) string_value: Option<Arc<str>>,
    pub(super) child_container: bool,
    pub(super) value_end: usize,
}

pub(super) fn property_at(line: &str, key_start: usize, key_end: usize) -> Option<JsonProperty> {
    const MAX_RELEVANT_KEY_RAW_BYTES: usize = MAX_TOOL_ID_KEY_BYTES * 6;
    const MAX_ROLE_OR_TYPE_RAW_BYTES: usize = 64;
    const MAX_TOOL_ID_RAW_BYTES: usize = MAX_TOOL_ID_BYTES * 6;

    let raw_key = line.get(key_start + 1..key_end.checked_sub(1)?)?;
    let key = if raw_key.len() <= MAX_RELEVANT_KEY_RAW_BYTES {
        decoded_string_content(line, key_start, key_end)?
    } else {
        String::new()
    };
    let mut cursor = skip_ws(line, key_end);
    if !line[cursor..].starts_with(':') {
        return None;
    }
    cursor += ':'.len_utf8();
    cursor = skip_ws(line, cursor);
    if !line[cursor..].starts_with('"') {
        return Some(JsonProperty {
            key,
            string_value: None,
            child_container: matches!(line[cursor..].chars().next(), Some('{' | '[')),
            value_end: key_end,
        });
    }
    let value_end = string_end(line, cursor)?;
    let raw_value_bytes = value_end.saturating_sub(cursor + 2);
    let decode_limit = match key.as_str() {
        "role" | "type" => MAX_ROLE_OR_TYPE_RAW_BYTES,
        _ if is_id_key(&key) && key.len() <= MAX_TOOL_ID_KEY_BYTES => MAX_TOOL_ID_RAW_BYTES,
        _ => 0,
    };
    let value = (raw_value_bytes <= decode_limit && decode_limit > 0)
        .then(|| decoded_string_content(line, cursor, value_end))
        .flatten()
        .map(Arc::from);
    Some(JsonProperty {
        key,
        string_value: value,
        child_container: false,
        value_end,
    })
}

pub(super) fn starts_new_root(line: &str) -> bool {
    line.as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
}

fn skip_ws(line: &str, mut cursor: usize) -> usize {
    while cursor < line.len() {
        let Some(ch) = line[cursor..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn decoded_string_content(line: &str, start: usize, end: usize) -> Option<String> {
    let content = line.get(start + 1..end.checked_sub(1)?)?;
    let mut output = String::with_capacity(content.len());
    let mut chars = content.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next()? {
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            '/' => output.push('/'),
            'b' => output.push('\u{0008}'),
            'f' => output.push('\u{000c}'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'u' => output.push(decode_unicode_escape(&mut chars)?),
            _ => return None,
        }
    }
    Some(output)
}

fn decode_unicode_escape(chars: &mut impl Iterator<Item = char>) -> Option<char> {
    let first = read_hex_quad(chars)?;
    if (0xd800..=0xdbff).contains(&first) {
        if chars.next()? != '\\' || chars.next()? != 'u' {
            return None;
        }
        let second = read_hex_quad(chars)?;
        if !(0xdc00..=0xdfff).contains(&second) {
            return None;
        }
        let scalar = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
        char::from_u32(scalar)
    } else if (0xdc00..=0xdfff).contains(&first) {
        None
    } else {
        char::from_u32(first)
    }
}

fn read_hex_quad(chars: &mut impl Iterator<Item = char>) -> Option<u32> {
    let mut value = 0_u32;
    for _ in 0..4 {
        value = value
            .checked_mul(16)?
            .checked_add(chars.next()?.to_digit(16)?)?;
    }
    Some(value)
}

pub(super) fn string_end(line: &str, start: usize) -> Option<usize> {
    if !line.get(start..)?.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    let mut index = start + 1;
    while index < line.len() {
        let ch = line[index..].chars().next()?;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(index + ch.len_utf8());
        }
        index += ch.len_utf8();
    }
    None
}
