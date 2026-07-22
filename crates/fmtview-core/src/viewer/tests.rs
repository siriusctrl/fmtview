use std::{
    cell::Cell,
    io::Write,
    time::{Duration, Instant},
};

use anyhow::Result;
use ratatui::{
    style::Modifier,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::file::input::*;
use super::file::input::{ViewState, handle_event, handle_key_event, handle_key_event_with_count};
use super::file::render::*;
use super::file::structure::*;
use super::*;
use crate::{
    formats::{highlight_json_like, highlight_xml_line},
    input::InputSource,
    load::{LazyTransformedRecordsFile, ViewFile},
    transform::{FormatKind, FormatOptions},
};

// Correctness tests run by default and should avoid wall-clock assertions.

mod cache;
mod format_highlight;
mod input;
mod navigation;
mod perf;
mod render;
mod screen;
mod search;
mod structure;
mod support;
mod viewport;

use support::*;
