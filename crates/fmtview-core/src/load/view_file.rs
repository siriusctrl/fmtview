use std::time::Duration;

use anyhow::Result;

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
    fn take_notice(&self) -> Option<String> {
        None
    }
}
