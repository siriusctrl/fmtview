use super::{following_lines, max_observed_offset};

pub(super) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let start_keyword = keyword(lines.get(start_offset)?)?;
    let Some(close_keyword) = close_keyword(start_keyword) else {
        return Some(read_start + start_offset);
    };
    let open_keyword = open_keyword(start_keyword);
    let mut depth = 1_usize;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        let Some(keyword) = keyword(line) else {
            continue;
        };
        if keyword == open_keyword {
            depth = depth.saturating_add(1);
        } else if keyword == close_keyword {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(read_start + offset);
            }
        }
    }
    None
}

pub(super) fn is_block(line: &str) -> bool {
    keyword(line).is_some_and(|keyword| {
        matches!(
            keyword,
            "block"
                | "endblock"
                | "if"
                | "elif"
                | "else"
                | "endif"
                | "for"
                | "endfor"
                | "macro"
                | "endmacro"
                | "filter"
                | "endfilter"
                | "call"
                | "endcall"
                | "include"
                | "extends"
                | "set"
                | "endset"
                | "with"
                | "endwith"
        )
    })
}

pub(super) fn keyword(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("{%")?;
    rest.split_whitespace().next()
}

fn open_keyword(keyword: &str) -> &str {
    match keyword {
        "elif" | "else" => "if",
        _ => keyword,
    }
}

fn close_keyword(keyword: &str) -> Option<&'static str> {
    match keyword {
        "block" => Some("endblock"),
        "if" | "elif" | "else" => Some("endif"),
        "for" => Some("endfor"),
        "macro" => Some("endmacro"),
        "filter" => Some("endfilter"),
        "call" => Some("endcall"),
        "set" => Some("endset"),
        "with" => Some("endwith"),
        _ => None,
    }
}
