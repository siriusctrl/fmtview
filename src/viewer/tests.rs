use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::file::input::*;
use super::file::render::*;
use super::file::structure::*;
use super::*;
use crate::{
    formats::{highlight_json_like, highlight_xml_line},
    input::InputSource,
    load::LazyTransformedRecordsFile,
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
