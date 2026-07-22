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
    fn byte_len(&self) -> u64;
    fn byte_offset_for_line(&self, line: usize) -> u64;
    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>>;
    fn preload(&self, _max_lines: usize, _max_records: usize, _budget: Duration) -> Result<bool> {
        Ok(false)
    }
    fn is_follow_source(&self) -> bool {
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
    fn open_raw_record(&self, _line: usize) -> Result<Option<Box<dyn ViewFile>>> {
        Ok(None)
    }
}
