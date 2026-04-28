mod detect;
mod engine;
mod json;
mod types;
mod xml;

#[cfg(test)]
mod tests;

pub use engine::{format_record_to_string, format_source_to_temp, trim_record_line_end};
pub use types::{FormatKind, FormatOptions};
