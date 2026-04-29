mod detect;
mod engine;
mod json;
mod types;
mod xml;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use engine::format_record_to_string;
pub(crate) use engine::{format_record_lines, parseable_record_line};
pub(crate) use engine::{format_source_to_temp, trim_record_line_end};
pub(crate) use types::{FormatKind, FormatOptions};
