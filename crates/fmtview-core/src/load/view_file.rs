use std::time::Duration;

use anyhow::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewFileChange {
    pub inserted_at: usize,
    pub inserted_lines: usize,
    pub appended_lines: usize,
    pub reset: bool,
}

impl ViewFileChange {
    pub fn changed(self) -> bool {
        self.inserted_lines > 0 || self.appended_lines > 0 || self.reset
    }
}

pub trait ViewFile {
    fn label(&self) -> &str;
    fn line_count(&self) -> usize;
    fn line_count_exact(&self) -> bool {
        true
    }
    /// Original one-based input line for a zero-based display line. Tail-first
    /// sources may not know this until their older boundary is discovered.
    fn source_line(&self, line: usize) -> Option<usize> {
        (line < self.line_count()).then_some(line.saturating_add(1))
    }
    /// Known original line count, or None while the source prefix is unknown.
    fn source_line_count(&self) -> Option<usize> {
        Some(self.line_count())
    }
    /// Locate the first display line at or after an original input line.
    /// None means that more input must be loaded before the jump can resolve.
    fn display_line_for_source(&self, requested: usize) -> Option<usize> {
        let count = self.line_count();
        if count == 0 {
            return self.line_count_exact().then_some(0);
        }
        let last = self.source_line(count - 1)?;
        if requested > last && !self.at_newer_boundary() {
            return None;
        }
        let requested = requested.max(1).min(last);
        let mut lo = 0;
        let mut hi = count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.source_line(mid)? < requested {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Some(lo.min(count - 1))
    }
    fn byte_len(&self) -> u64;
    fn byte_offset_for_line(&self, line: usize) -> u64;
    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>>;
    fn preload(&self, _max_lines: usize, _max_records: usize, _budget: Duration) -> Result<bool> {
        Ok(false)
    }
    fn is_follow_source(&self) -> bool {
        false
    }
    /// Whether the initially loaded window is anchored at the newer boundary.
    fn starts_at_tail(&self) -> bool {
        false
    }
    fn has_older_records(&self) -> bool {
        false
    }
    fn at_newer_boundary(&self) -> bool {
        self.line_count_exact()
    }
    fn load_older_records(&self, _max_records: usize, _max_bytes: usize) -> Result<ViewFileChange> {
        Ok(ViewFileChange::default())
    }
    fn refresh_records(&self, _max_records: usize, _max_bytes: usize) -> Result<ViewFileChange> {
        Ok(ViewFileChange::default())
    }
    fn take_notice(&self) -> Option<String> {
        None
    }
    fn supports_raw_records(&self) -> bool {
        false
    }
    fn open_raw_record(&self, _line: usize) -> Result<Option<Box<dyn ViewFile>>> {
        Ok(None)
    }
}
