use std::{
    cell::RefCell,
    collections::VecDeque,
    fs::File,
    hash::{DefaultHasher, Hasher},
    io::{Read, Seek, SeekFrom, Write},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::{
    timeline::{RecordLoadLimit, RecordTimeline, TimelineRead, TimelineRecord, TimelineRefresh},
    transform::{self, FormatOptions},
};

use super::{ViewFile, ViewFileChange};

const INITIAL_TAIL_RECORDS: usize = 128;
const INITIAL_TAIL_BYTES: usize = 4 * 1024 * 1024;
const RESET_OVERLAP_RECORDS: usize = 256;
const RESET_OVERLAP_BYTES: usize = 4 * 1024 * 1024;
const RESET_COMPARE_CHUNK_BYTES: usize = 16 * 1024;

/// A formatted, indexed `ViewFile` backed by a bidirectional record timeline.
///
/// Formatted text lives in an append-only temporary spool. The in-memory index
/// contains only line and record locations. Exact raw bytes used for reset
/// overlap detection live in a separate on-disk spool.
pub struct RecordTimelineViewFile {
    label: String,
    state: RefCell<TimelineViewState>,
}

impl RecordTimelineViewFile {
    pub fn new(timeline: Box<dyn RecordTimeline>, options: FormatOptions) -> Result<Self> {
        Self::with_initial_limit(
            timeline,
            options,
            RecordLoadLimit::new(INITIAL_TAIL_RECORDS, INITIAL_TAIL_BYTES),
        )
    }

    pub fn with_initial_limit(
        timeline: Box<dyn RecordTimeline>,
        options: FormatOptions,
        limit: RecordLoadLimit,
    ) -> Result<Self> {
        let label = timeline.label().to_owned();
        let mut state = TimelineViewState::new(timeline, options)?;
        state.load_older(limit)?;
        Ok(Self {
            label,
            state: RefCell::new(state),
        })
    }
}

impl ViewFile for RecordTimelineViewFile {
    fn label(&self) -> &str {
        &self.label
    }

    fn line_count(&self) -> usize {
        self.state.borrow().lines.len()
    }

    fn line_count_exact(&self) -> bool {
        let state = self.state.borrow();
        state.older_end && state.newer_end
    }

    fn byte_len(&self) -> u64 {
        self.state.borrow().timeline.snapshot().observed_end
    }

    fn byte_offset_for_line(&self, line: usize) -> u64 {
        let state = self.state.borrow();
        state.lines.get(line).map_or_else(
            || state.timeline.snapshot().observed_end,
            |line| line.source_offset,
        )
    }

    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
        let state = self.state.borrow();
        if count == 0 || start >= state.lines.len() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(count).min(state.lines.len());
        let mut spool = File::open(state.spool.path()).context("failed to open timeline spool")?;
        let mut lines = Vec::with_capacity(end - start);
        for line in &state.lines[start..end] {
            spool
                .seek(SeekFrom::Start(line.spool_offset))
                .context("failed to seek timeline spool")?;
            let mut bytes = vec![0_u8; line.len];
            spool
                .read_exact(&mut bytes)
                .context("failed to read timeline spool")?;
            lines.push(String::from_utf8(bytes).context("timeline spool was not valid UTF-8")?);
        }
        Ok(lines)
    }

    fn is_follow_source(&self) -> bool {
        true
    }

    fn has_older_records(&self) -> bool {
        !self.state.borrow().older_end
    }

    fn at_newer_boundary(&self) -> bool {
        self.state.borrow().at_newer_boundary
    }

    fn load_older_records(&self, max_records: usize, max_bytes: usize) -> Result<ViewFileChange> {
        self.state
            .borrow_mut()
            .load_older(RecordLoadLimit::new(max_records, max_bytes))
    }

    fn refresh_records(&self, max_records: usize, max_bytes: usize) -> Result<ViewFileChange> {
        self.state
            .borrow_mut()
            .refresh(RecordLoadLimit::new(max_records, max_bytes))
    }

    fn take_notice(&self) -> Option<String> {
        self.state.borrow_mut().notices.pop_front()
    }
}

struct TimelineViewState {
    timeline: Box<dyn RecordTimeline>,
    options: FormatOptions,
    spool: NamedTempFile,
    spool_len: u64,
    raw_spool: NamedTempFile,
    raw_spool_len: u64,
    lines: Vec<TimelineLine>,
    older_insert_at: usize,
    records: Vec<TimelineRecordRef>,
    older_record_insert_at: usize,
    reset_boundary: Option<ResetBoundary>,
    older_end: bool,
    newer_end: bool,
    at_newer_boundary: bool,
    notices: VecDeque<String>,
}

impl TimelineViewState {
    fn new(timeline: Box<dyn RecordTimeline>, options: FormatOptions) -> Result<Self> {
        Ok(Self {
            timeline,
            options,
            spool: NamedTempFile::new().context("failed to create timeline spool")?,
            spool_len: 0,
            raw_spool: NamedTempFile::new().context("failed to create raw timeline spool")?,
            raw_spool_len: 0,
            lines: Vec::new(),
            older_insert_at: 0,
            records: Vec::new(),
            older_record_insert_at: 0,
            reset_boundary: None,
            older_end: false,
            newer_end: false,
            at_newer_boundary: true,
            notices: VecDeque::new(),
        })
    }

    fn load_older(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        if self.older_end {
            return Ok(ViewFileChange::default());
        }
        match self.timeline.load_older(limit)? {
            TimelineRead::Records(records) => {
                let reached_older_end = self.reached_older_end(&records);
                let inserted_at = self.older_insert_at;
                let inserted_record_at = self.older_record_insert_at;
                let spooled = self.spool_records(&records, true)?;
                let inserted_lines = spooled.lines.len();
                self.lines.splice(inserted_at..inserted_at, spooled.lines);
                self.records
                    .splice(inserted_record_at..inserted_record_at, spooled.records);
                let (removed_at, removed_lines) = if reached_older_end {
                    self.older_end = true;
                    self.finish_reset_reconciliation()?
                } else {
                    (0, 0)
                };
                Ok(ViewFileChange {
                    inserted_at,
                    inserted_lines,
                    removed_at,
                    removed_lines,
                    ..ViewFileChange::default()
                })
            }
            TimelineRead::Pending => Ok(ViewFileChange::default()),
            TimelineRead::End => {
                self.older_end = true;
                let (removed_at, removed_lines) = self.finish_reset_reconciliation()?;
                Ok(ViewFileChange {
                    removed_at,
                    removed_lines,
                    ..ViewFileChange::default()
                })
            }
        }
    }

    fn refresh(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        let refresh = self.timeline.refresh()?;
        if matches!(refresh, TimelineRefresh::End(_)) {
            self.newer_end = true;
            self.at_newer_boundary = true;
            return Ok(ViewFileChange::default());
        }
        if matches!(refresh, TimelineRefresh::Reset { .. }) {
            return self.load_reset_tail(limit);
        }
        self.load_newer(limit)
    }

    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        match self.timeline.load_newer(limit)? {
            TimelineRead::Records(records) => {
                let old_len = self.lines.len();
                let spooled = self.spool_records(&records, true)?;
                self.lines.extend(spooled.lines);
                self.records.extend(spooled.records);
                let snapshot = self.timeline.snapshot();
                self.at_newer_boundary = records.last().is_some_and(|record| {
                    record.id.epoch == snapshot.epoch
                        && record.id.end_offset == snapshot.committed_end
                });
                Ok(ViewFileChange {
                    appended_lines: self.lines.len().saturating_sub(old_len),
                    ..ViewFileChange::default()
                })
            }
            TimelineRead::Pending => {
                self.at_newer_boundary = true;
                Ok(ViewFileChange::default())
            }
            TimelineRead::End => {
                self.newer_end = true;
                self.at_newer_boundary = true;
                Ok(ViewFileChange::default())
            }
        }
    }

    fn load_reset_tail(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        self.older_insert_at = self.lines.len();
        self.older_record_insert_at = self.records.len();
        self.reset_boundary = Some(ResetBoundary {
            old_lines: self.lines.len(),
            old_records: self.records.len(),
        });
        self.older_end = false;
        self.newer_end = false;
        self.at_newer_boundary = true;
        let records = match self.timeline.load_older(limit)? {
            TimelineRead::Records(records) => records,
            TimelineRead::Pending => {
                return Ok(ViewFileChange {
                    reset: true,
                    ..ViewFileChange::default()
                });
            }
            TimelineRead::End => {
                self.older_end = true;
                self.reset_boundary = None;
                return Ok(ViewFileChange {
                    reset: true,
                    ..ViewFileChange::default()
                });
            }
        };
        let reached_older_end = self.reached_older_end(&records);
        let spooled = self.spool_records(&records, true)?;
        let inserted_lines = spooled.lines.len();
        self.lines.extend(spooled.lines);
        self.records.extend(spooled.records);
        let removed_lines = if reached_older_end {
            self.older_end = true;
            self.finish_reset_reconciliation()?.1
        } else {
            0
        };
        Ok(ViewFileChange {
            appended_lines: inserted_lines.saturating_sub(removed_lines),
            reset: true,
            ..ViewFileChange::default()
        })
    }

    fn spool_records(
        &mut self,
        records: &[TimelineRecord],
        notices: bool,
    ) -> Result<SpooledRecords> {
        let mut lines = Vec::new();
        let mut record_refs = Vec::new();
        let mut parse_failures = 0_usize;
        let mut first_failure_offset = None;
        for record in records {
            let bytes = match transform::format_record_bytes(&record.raw, self.options) {
                Ok(bytes) => bytes,
                Err(_) => {
                    parse_failures = parse_failures.saturating_add(1);
                    first_failure_offset.get_or_insert(record.id.start_offset);
                    String::from_utf8_lossy(transform::trim_record_line_end(&record.raw))
                        .into_owned()
                        .into_bytes()
                }
            };
            let (next_lines, record_ref) = self.write_record(record, &bytes)?;
            lines.extend(next_lines);
            record_refs.push(record_ref);
        }
        if notices && parse_failures > 0 {
            let first_offset = first_failure_offset.unwrap_or_default();
            let detail = if parse_failures == 1 {
                "showing raw record".to_owned()
            } else {
                format!(
                    "and {} more records; showing raw records",
                    parse_failures - 1
                )
            };
            self.notices.push_back(format!(
                "record at byte {first_offset} failed JSON parse; {detail}"
            ));
        }
        Ok(SpooledRecords {
            lines,
            records: record_refs,
        })
    }

    fn write_record(
        &mut self,
        record: &TimelineRecord,
        bytes: &[u8],
    ) -> Result<(Vec<TimelineLine>, TimelineRecordRef)> {
        let raw_offset = self.raw_spool_len;
        self.raw_spool
            .as_file_mut()
            .write_all(&record.raw)
            .context("failed to write raw timeline spool")?;
        self.raw_spool_len = self.raw_spool_len.saturating_add(record.raw.len() as u64);
        let record_start = self.spool_len;
        self.spool
            .as_file_mut()
            .write_all(bytes)
            .context("failed to write timeline spool")?;
        self.spool
            .as_file_mut()
            .write_all(b"\n")
            .context("failed to terminate timeline spool record")?;
        self.spool_len = self
            .spool_len
            .saturating_add(bytes.len() as u64)
            .saturating_add(1);

        let mut refs = Vec::new();
        let mut start = 0_usize;
        for newline in memchr::memchr_iter(b'\n', bytes) {
            refs.push(TimelineLine {
                spool_offset: record_start + start as u64,
                len: newline.saturating_sub(start),
                source_offset: record.id.start_offset,
            });
            start = newline + 1;
        }
        if start < bytes.len() || bytes.is_empty() {
            refs.push(TimelineLine {
                spool_offset: record_start + start as u64,
                len: bytes.len().saturating_sub(start),
                source_offset: record.id.start_offset,
            });
        }
        let record_ref = TimelineRecordRef {
            id: record.id,
            raw_offset,
            raw_len: record.raw.len(),
            raw_hash: hash_raw(&record.raw),
            line_count: refs.len(),
        };
        Ok((refs, record_ref))
    }

    fn reached_older_end(&self, records: &[TimelineRecord]) -> bool {
        let snapshot = self.timeline.snapshot();
        records.first().is_some_and(|record| {
            record.id.epoch == snapshot.epoch && record.id.start_offset == snapshot.committed_start
        })
    }

    fn finish_reset_reconciliation(&mut self) -> Result<(usize, usize)> {
        let Some(boundary) = self.reset_boundary else {
            return Ok((0, 0));
        };
        let new = &self.records[boundary.old_records..];
        let old = &self.records[..boundary.old_records];
        let max = bounded_prefix_len(new).min(bounded_suffix_len(old));
        let mut old_spool =
            File::open(self.raw_spool.path()).context("failed to open raw timeline spool")?;
        let mut new_spool =
            File::open(self.raw_spool.path()).context("failed to open raw timeline spool")?;
        let mut scratch = vec![0_u8; RESET_COMPARE_CHUNK_BYTES];
        let mut other = vec![0_u8; RESET_COMPARE_CHUNK_BYTES];
        let mut overlap = 0;
        for count in (1..=max).rev() {
            let old_start = old.len() - count;
            let mut matches = true;
            for index in 0..count {
                if !record_refs_match(
                    &mut old_spool,
                    &mut new_spool,
                    &mut scratch,
                    &mut other,
                    &old[old_start + index],
                    &new[index],
                )? {
                    matches = false;
                    break;
                }
            }
            if matches {
                overlap = count;
                break;
            }
        }
        if overlap == 0 {
            self.reset_boundary = None;
            return Ok((0, 0));
        }

        let removed_lines = self.records
            [boundary.old_records..boundary.old_records.saturating_add(overlap)]
            .iter()
            .map(|record| record.line_count)
            .sum::<usize>();
        self.records
            .drain(boundary.old_records..boundary.old_records.saturating_add(overlap));
        self.lines
            .drain(boundary.old_lines..boundary.old_lines.saturating_add(removed_lines));
        self.reset_boundary = None;
        Ok((boundary.old_lines, removed_lines))
    }
}

#[derive(Debug, Clone, Copy)]
struct ResetBoundary {
    old_lines: usize,
    old_records: usize,
}

struct SpooledRecords {
    lines: Vec<TimelineLine>,
    records: Vec<TimelineRecordRef>,
}

#[derive(Debug, Clone, Copy)]
struct TimelineRecordRef {
    id: crate::timeline::RecordId,
    raw_offset: u64,
    raw_len: usize,
    raw_hash: u64,
    line_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct TimelineLine {
    spool_offset: u64,
    len: usize,
    source_offset: u64,
}

fn record_refs_match(
    old_spool: &mut File,
    new_spool: &mut File,
    scratch: &mut [u8],
    other: &mut [u8],
    old: &TimelineRecordRef,
    new: &TimelineRecordRef,
) -> Result<bool> {
    if old.id == new.id {
        return Ok(true);
    }
    if old.raw_len != new.raw_len || old.raw_hash != new.raw_hash {
        return Ok(false);
    }
    old_spool
        .seek(SeekFrom::Start(old.raw_offset))
        .context("failed to seek raw timeline spool")?;
    new_spool
        .seek(SeekFrom::Start(new.raw_offset))
        .context("failed to seek raw timeline spool")?;
    let mut remaining = old.raw_len;
    while remaining > 0 {
        let count = remaining.min(scratch.len());
        let old_bytes = &mut scratch[..count];
        let new_bytes = &mut other[..count];
        old_spool
            .read_exact(old_bytes)
            .context("failed to compare raw timeline spool")?;
        new_spool
            .read_exact(new_bytes)
            .context("failed to compare raw timeline spool")?;
        if old_bytes != new_bytes {
            return Ok(false);
        }
        remaining -= count;
    }
    Ok(true)
}

fn hash_raw(raw: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(raw);
    hasher.finish()
}

trait RawRecordLen {
    fn raw_len(&self) -> usize;
}

impl RawRecordLen for TimelineRecord {
    fn raw_len(&self) -> usize {
        self.raw.len()
    }
}

impl RawRecordLen for TimelineRecordRef {
    fn raw_len(&self) -> usize {
        self.raw_len
    }
}

fn bounded_prefix_len<T: RawRecordLen>(records: &[T]) -> usize {
    let mut bytes = 0_usize;
    for (index, record) in records.iter().take(RESET_OVERLAP_RECORDS).enumerate() {
        if index > 0 && bytes.saturating_add(record.raw_len()) > RESET_OVERLAP_BYTES {
            return index;
        }
        bytes = bytes.saturating_add(record.raw_len());
    }
    records.len().min(RESET_OVERLAP_RECORDS)
}

fn bounded_suffix_len<T: RawRecordLen>(records: &[T]) -> usize {
    let mut bytes = 0_usize;
    let mut count = 0_usize;
    for record in records.iter().rev().take(RESET_OVERLAP_RECORDS) {
        if count > 0 && bytes.saturating_add(record.raw_len()) > RESET_OVERLAP_BYTES {
            break;
        }
        bytes = bytes.saturating_add(record.raw_len());
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::RecordId;

    #[test]
    fn reset_overlap_candidate_window_is_count_and_byte_bounded() {
        let records = (0..300)
            .map(|index| TimelineRecord {
                id: RecordId {
                    epoch: 1,
                    start_offset: index,
                    end_offset: index + 1,
                },
                raw: vec![b'x'; 20 * 1024],
            })
            .collect::<Vec<_>>();

        let prefix = bounded_prefix_len(&records);
        let suffix = bounded_suffix_len(&records);
        assert!(prefix < RESET_OVERLAP_RECORDS);
        assert!(suffix < RESET_OVERLAP_RECORDS);
        assert!(
            records[..prefix]
                .iter()
                .map(|record| record.raw.len())
                .sum::<usize>()
                <= RESET_OVERLAP_BYTES
        );
        assert!(
            records[records.len() - suffix..]
                .iter()
                .map(|record| record.raw.len())
                .sum::<usize>()
                <= RESET_OVERLAP_BYTES
        );
    }
}
