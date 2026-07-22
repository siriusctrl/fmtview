//! Embed fmtview's interactive JSONL timeline viewer in another application.
//!
//! The facade keeps crossterm events and terminal lifecycle private while
//! exposing the backend-neutral record source contract used by the viewer.
//! A source starts with both directional cursors at its committed tail.
//!
//! ```no_run
//! use fmtview::view::{
//!     self, RecordLoadLimit, RecordTimeline, Result, TimelineRead, TimelineRefresh,
//!     TimelineSnapshot, ViewOptions,
//! };
//!
//! struct EmptyTimeline;
//!
//! impl RecordTimeline for EmptyTimeline {
//!     fn label(&self) -> &str { "embedded run" }
//!     fn snapshot(&self) -> TimelineSnapshot {
//!         TimelineSnapshot {
//!             epoch: 1,
//!             committed_end: 0,
//!             observed_end: 0,
//!             pending_bytes: 0,
//!         }
//!     }
//!     fn probe_prefix(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
//!         Ok(TimelineRead::End)
//!     }
//!     fn load_older(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
//!         Ok(TimelineRead::End)
//!     }
//!     fn load_newer(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
//!         Ok(TimelineRead::End)
//!     }
//!     fn refresh(&mut self) -> Result<TimelineRefresh> {
//!         Ok(TimelineRefresh::End(self.snapshot()))
//!     }
//! }
//!
//! fn main() -> Result<()> {
//!     let mut options = ViewOptions::default();
//!     options.follow = true;
//!     view::run(Box::new(EmptyTimeline), options)
//! }
//! ```

use std::io::{self, IsTerminal};

use anyhow::bail;
use fmtview_core::{FormatKind, FormatOptions, RecordTimelineViewFile, ViewFile};

pub use fmtview_core::{
    RecordId, RecordLoadLimit, RecordTimeline, TimelineRead, TimelineReadNext, TimelineRecord,
    TimelineRefresh, TimelineResetReason, TimelineSnapshot,
};

/// Result type used by the embedding facade and [`RecordTimeline`] methods.
///
/// Re-exporting the error type through this alias lets a downstream source
/// implementation depend on `fmtview` without also naming `anyhow` directly.
pub type Result<T> = anyhow::Result<T>;

/// Options for an embedded JSONL record-timeline viewer.
///
/// Snapshot mode is the default. It opens at the current committed tail and
/// lazily loads older records without refreshing newer records or exposing
/// follow controls. Set [`Self::follow`] to continuously refresh the source.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ViewOptions {
    /// Number of spaces used when formatting each JSONL record.
    pub indent: usize,
    /// Optional transient message shown when the viewer opens.
    pub notice: Option<String>,
    /// Refresh newer records and enable attached/detached/paused follow state.
    pub follow: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            indent: 2,
            notice: None,
            follow: false,
        }
    }
}

/// Run the interactive terminal viewer for a backend-neutral record timeline.
///
/// Each yielded record is interpreted as one exact JSONL source record. The
/// source decides which records are committed; fmtview owns bounded tail-first
/// loading, formatting and raw spools, search/navigation, follow state, the
/// crossterm event loop, and terminal cleanup.
pub fn run(source: Box<dyn RecordTimeline>, options: ViewOptions) -> Result<()> {
    validate_options(&options)?;
    if !io::stdout().is_terminal() {
        bail!("embedded fmtview requires an interactive terminal on stdout");
    }

    let notice = options.notice.clone();
    let file = open_timeline(source, &options)?;
    crate::viewer::run(file, FormatKind::Jsonl, notice)
}

fn validate_options(options: &ViewOptions) -> Result<()> {
    if options.indent == 0 || options.indent > 16 {
        bail!("view indent must be between 1 and 16");
    }
    Ok(())
}

fn open_timeline(
    source: Box<dyn RecordTimeline>,
    options: &ViewOptions,
) -> Result<Box<dyn ViewFile>> {
    let format = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: options.indent,
    };
    let file = if options.follow {
        RecordTimelineViewFile::new(source, format)?
    } else {
        RecordTimelineViewFile::snapshot(source, format)?
    };
    Ok(Box::new(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyTimeline;

    impl RecordTimeline for EmptyTimeline {
        fn label(&self) -> &str {
            "empty"
        }

        fn snapshot(&self) -> TimelineSnapshot {
            TimelineSnapshot {
                epoch: 1,
                committed_end: 0,
                observed_end: 0,
                pending_bytes: 0,
            }
        }

        fn probe_prefix(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
            Ok(TimelineRead::End)
        }

        fn load_older(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
            Ok(TimelineRead::End)
        }

        fn load_newer(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
            Ok(TimelineRead::End)
        }

        fn refresh(&mut self) -> Result<TimelineRefresh> {
            Ok(TimelineRefresh::End(self.snapshot()))
        }
    }

    #[test]
    fn defaults_to_a_non_refreshing_snapshot() {
        let options = ViewOptions::default();
        let file = open_timeline(Box::new(EmptyTimeline), &options).unwrap();

        assert_eq!(options.indent, 2);
        assert!(!options.follow);
        assert!(!file.is_follow_source());
    }

    #[test]
    fn follow_option_selects_live_refresh_behavior() {
        let options = ViewOptions {
            follow: true,
            ..ViewOptions::default()
        };
        let file = open_timeline(Box::new(EmptyTimeline), &options).unwrap();

        assert!(file.is_follow_source());
    }

    #[test]
    fn rejects_invalid_jsonl_indent_before_entering_the_terminal() {
        let options = ViewOptions {
            indent: 0,
            ..ViewOptions::default()
        };

        assert!(validate_options(&options).is_err());
    }
}
