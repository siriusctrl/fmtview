use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    fs::File,
    hash::{DefaultHasher, Hasher},
    io::{Read, Seek, SeekFrom, Write},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::{
    timeline::{
        RecordLoadLimit, RecordTimeline, TimelineRead, TimelineReadNext, TimelineRecord,
        TimelineRefresh,
    },
    transform::{self, FormatOptions},
};

use super::{RawRecordViewFile, ViewFile, ViewFileChange};

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
    follow: bool,
    state: RefCell<TimelineViewState>,
}

impl RecordTimelineViewFile {
    /// Open a live timeline at its current tail and refresh it while viewed.
    pub fn new(timeline: Box<dyn RecordTimeline>, options: FormatOptions) -> Result<Self> {
        Self::with_initial_limit(
            timeline,
            options,
            RecordLoadLimit::new(INITIAL_TAIL_RECORDS, INITIAL_TAIL_BYTES),
        )
    }

    /// Open a timeline at its current tail without refreshing newer records.
    ///
    /// Older records remain available through bounded lazy loads. This is
    /// useful when an embedder wants a stable snapshot of a source that may
    /// continue growing in the background.
    pub fn snapshot(timeline: Box<dyn RecordTimeline>, options: FormatOptions) -> Result<Self> {
        Self::snapshot_with_initial_limit(
            timeline,
            options,
            RecordLoadLimit::new(INITIAL_TAIL_RECORDS, INITIAL_TAIL_BYTES),
        )
    }

    /// Open a live timeline with an explicit initial tail-load limit.
    pub fn with_initial_limit(
        timeline: Box<dyn RecordTimeline>,
        options: FormatOptions,
        limit: RecordLoadLimit,
    ) -> Result<Self> {
        Self::with_initial_limit_and_follow(timeline, options, limit, true)
    }

    /// Open a non-refreshing timeline snapshot with an explicit initial limit.
    pub fn snapshot_with_initial_limit(
        timeline: Box<dyn RecordTimeline>,
        options: FormatOptions,
        limit: RecordLoadLimit,
    ) -> Result<Self> {
        Self::with_initial_limit_and_follow(timeline, options, limit, false)
    }

    fn with_initial_limit_and_follow(
        timeline: Box<dyn RecordTimeline>,
        options: FormatOptions,
        limit: RecordLoadLimit,
        follow: bool,
    ) -> Result<Self> {
        let label = timeline.label().to_owned();
        let mut state = TimelineViewState::new(timeline, options)?;
        state.load_older(limit)?;
        Ok(Self {
            label,
            follow,
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
        self.follow
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

    fn open_raw_record(&self, line_index: usize) -> Result<Option<Box<dyn ViewFile>>> {
        let state = self.state.borrow();
        let Some(line) = state.lines.get(line_index) else {
            return Ok(None);
        };
        let raw = RawRecordViewFile::new(
            state
                .raw_spool
                .reopen()
                .context("failed to reopen raw timeline spool")?,
            &self.label,
            line.raw_offset,
            u64::try_from(line.raw_len).context("raw timeline record was too large")?,
            line_index,
        )?;
        Ok(Some(Box::new(raw)))
    }

    fn supports_raw_records(&self) -> bool {
        true
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
    reset_overlap_ids: HashSet<crate::timeline::RecordId>,
    reset_pending: bool,
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
            reset_overlap_ids: HashSet::new(),
            reset_pending: false,
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
            TimelineRead::Records { records, next } => {
                let records = self.filter_reset_overlap(records);
                let inserted_at = self.older_insert_at;
                let inserted_record_at = self.older_record_insert_at;
                let spooled = self.spool_records(&records, true)?;
                let inserted_lines = spooled.lines.len();
                self.lines.splice(inserted_at..inserted_at, spooled.lines);
                self.records
                    .splice(inserted_record_at..inserted_record_at, spooled.records);
                self.older_end = next == TimelineReadNext::End;
                if self.older_end {
                    self.reset_overlap_ids.clear();
                }
                Ok(ViewFileChange {
                    inserted_at,
                    inserted_lines,
                    ..ViewFileChange::default()
                })
            }
            TimelineRead::Pending => Ok(ViewFileChange::default()),
            TimelineRead::End => {
                self.older_end = true;
                self.reset_overlap_ids.clear();
                Ok(ViewFileChange::default())
            }
        }
    }

    fn refresh(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        if self.reset_pending {
            return self.load_reset_tail(limit);
        }
        let refresh = self.timeline.refresh()?;
        if matches!(refresh, TimelineRefresh::End(_)) {
            self.newer_end = true;
            self.at_newer_boundary = true;
            return Ok(ViewFileChange::default());
        }
        if matches!(refresh, TimelineRefresh::Reset { .. }) {
            self.reset_pending = true;
            return self.load_reset_tail(limit);
        }
        self.load_newer(limit)
    }

    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<ViewFileChange> {
        match self.timeline.load_newer(limit)? {
            TimelineRead::Records { records, next } => {
                let old_len = self.lines.len();
                let spooled = self.spool_records(&records, true)?;
                self.lines.extend(spooled.lines);
                self.records.extend(spooled.records);
                self.at_newer_boundary = next != TimelineReadNext::More;
                self.newer_end = next == TimelineReadNext::End;
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
        let prefix = match self.timeline.probe_prefix(RecordLoadLimit::new(
            RESET_OVERLAP_RECORDS,
            RESET_OVERLAP_BYTES,
        ))? {
            TimelineRead::Records { records, .. } => records,
            TimelineRead::Pending | TimelineRead::End => Vec::new(),
        };
        self.older_insert_at = self.lines.len();
        self.older_record_insert_at = self.records.len();
        self.reset_overlap_ids.clear();
        self.older_end = false;
        self.newer_end = false;
        self.at_newer_boundary = true;
        let (records, next) = match self.timeline.load_older(limit)? {
            TimelineRead::Records { records, next } => (records, next),
            TimelineRead::Pending => {
                self.reset_pending = false;
                return Ok(ViewFileChange {
                    reset: true,
                    ..ViewFileChange::default()
                });
            }
            TimelineRead::End => {
                self.older_end = true;
                self.reset_pending = false;
                return Ok(ViewFileChange {
                    reset: true,
                    ..ViewFileChange::default()
                });
            }
        };

        let overlap = self.reset_tail_overlap(&prefix)?;
        self.reset_overlap_ids
            .extend(prefix[..overlap].iter().map(|record| record.id));
        self.older_end = next == TimelineReadNext::End;

        let records = self.filter_reset_overlap(records);
        if self.older_end {
            self.reset_overlap_ids.clear();
        }
        self.reset_pending = false;
        let old_len = self.lines.len();
        let spooled = self.spool_records(&records, true)?;
        self.lines.extend(spooled.lines);
        self.records.extend(spooled.records);
        Ok(ViewFileChange {
            appended_lines: self.lines.len().saturating_sub(old_len),
            reset: true,
            ..ViewFileChange::default()
        })
    }

    fn filter_reset_overlap(&self, mut records: Vec<TimelineRecord>) -> Vec<TimelineRecord> {
        if !self.reset_overlap_ids.is_empty() {
            records.retain(|record| !self.reset_overlap_ids.contains(&record.id));
        }
        records
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
            let bytes = match transform::format_record_display_bytes(&record.raw, self.options) {
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
                raw_offset,
                raw_len: record.raw.len(),
            });
            start = newline + 1;
        }
        if start < bytes.len() || bytes.is_empty() {
            refs.push(TimelineLine {
                spool_offset: record_start + start as u64,
                len: bytes.len().saturating_sub(start),
                source_offset: record.id.start_offset,
                raw_offset,
                raw_len: record.raw.len(),
            });
        }
        let record_ref = TimelineRecordRef {
            id: record.id,
            raw_offset,
            raw_len: record.raw.len(),
            raw_hash: hash_raw(&record.raw),
        };
        Ok((refs, record_ref))
    }

    fn reset_tail_overlap(&self, records: &[TimelineRecord]) -> Result<usize> {
        let max = bounded_prefix_len(records);
        self.longest_suffix_overlap(self.records.len(), &records[..max])
    }

    fn longest_suffix_overlap(&self, old_end: usize, new: &[TimelineRecord]) -> Result<usize> {
        let max = old_end.min(new.len());
        let new_hashes = new
            .iter()
            .map(|record| hash_raw(&record.raw))
            .collect::<Vec<_>>();
        let mut spool =
            File::open(self.raw_spool.path()).context("failed to open raw timeline spool")?;
        let mut scratch = vec![0_u8; RESET_COMPARE_CHUNK_BYTES];
        for count in (1..=max).rev() {
            let old_start = old_end - count;
            let new_start = 0;
            let mut matches = true;
            for index in 0..count {
                if !record_ref_matches(
                    &mut spool,
                    &mut scratch,
                    &self.records[old_start + index],
                    &new[new_start + index],
                    new_hashes[new_start + index],
                )? {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Ok(count);
            }
        }
        Ok(0)
    }
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
}

#[derive(Debug, Clone, Copy)]
struct TimelineLine {
    spool_offset: u64,
    len: usize,
    source_offset: u64,
    raw_offset: u64,
    raw_len: usize,
}

fn record_ref_matches(
    spool: &mut File,
    scratch: &mut [u8],
    old: &TimelineRecordRef,
    new: &TimelineRecord,
    new_hash: u64,
) -> Result<bool> {
    if old.id == new.id {
        return Ok(true);
    }
    if old.raw_len != new.raw.len() || old.raw_hash != new_hash {
        return Ok(false);
    }
    spool
        .seek(SeekFrom::Start(old.raw_offset))
        .context("failed to seek raw timeline spool")?;
    for expected in new.raw.chunks(scratch.len()) {
        let actual = &mut scratch[..expected.len()];
        spool
            .read_exact(actual)
            .context("failed to compare raw timeline spool")?;
        if actual != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn hash_raw(raw: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(raw);
    hasher.finish()
}

fn bounded_prefix_len(records: &[TimelineRecord]) -> usize {
    let mut bytes = 0_usize;
    for (index, record) in records.iter().take(RESET_OVERLAP_RECORDS).enumerate() {
        if index > 0 && bytes.saturating_add(record.raw.len()) > RESET_OVERLAP_BYTES {
            return index;
        }
        bytes = bytes.saturating_add(record.raw.len());
    }
    records.len().min(RESET_OVERLAP_RECORDS)
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
        assert!(prefix < RESET_OVERLAP_RECORDS);
        assert!(
            records[..prefix]
                .iter()
                .map(|record| record.raw.len())
                .sum::<usize>()
                <= RESET_OVERLAP_BYTES
        );
    }
}
