//! Bidirectional record timelines for tail-first and growing inputs.

use std::{
    fs::{File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use memchr::{memchr, memrchr};

const REVERSE_SCAN_CHUNK_BYTES: usize = 64 * 1024;
const FORWARD_SCAN_CHUNK_BYTES: usize = 16 * 1024;
const REFRESH_SHORT_READ_ATTEMPTS: usize = 3;
const RANGE_SAMPLE_BYTES: u64 = 64;

/// Stable identity for one committed record within a source epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordId {
    pub epoch: u64,
    pub start_offset: u64,
    pub end_offset: u64,
}

/// One committed source record with its exact bytes, including its line ending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRecord {
    pub id: RecordId,
    pub raw: Vec<u8>,
}

/// A bounded request to move through a record timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordLoadLimit {
    pub max_records: usize,
    pub max_bytes: usize,
}

impl RecordLoadLimit {
    pub const fn new(max_records: usize, max_bytes: usize) -> Self {
        Self {
            max_records,
            max_bytes,
        }
    }

    fn normalized(self) -> Self {
        Self {
            max_records: self.max_records.max(1),
            max_bytes: self.max_bytes.max(1),
        }
    }
}

/// Result of moving toward older or newer records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineRead {
    Records(Vec<TimelineRecord>),
    /// The source may produce more committed records later.
    Pending,
    /// This direction has a terminal boundary.
    End,
}

/// Why a live source started a new identity epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineResetReason {
    Truncated,
    Replaced,
    IdentityChanged,
}

/// Current committed and observed source boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineSnapshot {
    pub epoch: u64,
    pub committed_end: u64,
    pub observed_end: u64,
    pub pending_bytes: u64,
}

/// Result of refreshing a live source snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineRefresh {
    Appended(TimelineSnapshot),
    NoChange(TimelineSnapshot),
    Pending(TimelineSnapshot),
    End(TimelineSnapshot),
    Reset {
        reason: TimelineResetReason,
        snapshot: TimelineSnapshot,
    },
}

/// Backend-neutral, bidirectional source contract used by the core viewer.
///
/// Implementations decide when a record is valid and committed. Every yielded
/// record must be stable for its `RecordId`, and its `raw` bytes must be exact.
/// `Pending` means a live boundary may advance; `End` is terminal.
pub trait RecordTimeline {
    fn label(&self) -> &str;
    fn snapshot(&self) -> TimelineSnapshot;
    fn load_older(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead>;
    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead>;
    fn refresh(&mut self) -> Result<TimelineRefresh>;
}

/// Read-work instrumentation for deterministic tail-open assertions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTimelineInstrumentation {
    pub bytes_read: u64,
    pub read_operations: u64,
    pub records_yielded: u64,
}

/// A growing newline-delimited file opened at its current committed tail.
pub struct FileRecordTimeline {
    path: PathBuf,
    label: String,
    file: File,
    identity: FileIdentity,
    change_stamp: FileChangeStamp,
    epoch: u64,
    committed_end: u64,
    observed_end: u64,
    older_cursor: u64,
    newer_cursor: u64,
    committed_sample: Vec<u8>,
    pending_sample: Vec<u8>,
    instrumentation: FileTimelineInstrumentation,
}

impl FileRecordTimeline {
    /// Open without indexing the whole file. Only the uncommitted EOF suffix is
    /// inspected to locate the current committed boundary.
    pub fn open(path: impl Into<PathBuf>, label: impl Into<String>) -> Result<Self> {
        let path = path.into();
        let label = label.into();
        let mut file =
            File::open(&path).with_context(|| format!("failed to open growing source {label}"))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("failed to stat growing source {label}"))?;
        if !metadata.is_file() {
            bail!("follow source is not a file: {}", path.display());
        }
        let observed_end = metadata.len();
        let mut instrumentation = FileTimelineInstrumentation::default();
        let committed_end =
            find_committed_end(&mut file, observed_end, &mut instrumentation, &label)?;
        let committed_sample =
            read_committed_sample(&mut file, committed_end, &mut instrumentation, &label)?;
        let pending_sample = read_pending_sample(
            &mut file,
            committed_end,
            observed_end,
            &mut instrumentation,
            &label,
        )?;

        Ok(Self {
            path,
            label,
            file,
            identity: FileIdentity::from_metadata(&metadata),
            change_stamp: FileChangeStamp::from_metadata(&metadata),
            epoch: 1,
            committed_end,
            observed_end,
            older_cursor: committed_end,
            newer_cursor: committed_end,
            committed_sample,
            pending_sample,
            instrumentation,
        })
    }

    pub fn instrumentation(&self) -> FileTimelineInstrumentation {
        self.instrumentation
    }

    fn reset(
        &mut self,
        mut file: File,
        metadata: &Metadata,
        reason: TimelineResetReason,
    ) -> Result<TimelineRefresh> {
        let observed_end = metadata.len();
        let committed_end = find_committed_end(
            &mut file,
            observed_end,
            &mut self.instrumentation,
            &self.label,
        )?;
        let committed_sample = read_committed_sample(
            &mut file,
            committed_end,
            &mut self.instrumentation,
            &self.label,
        )?;
        let pending_sample = read_pending_sample(
            &mut file,
            committed_end,
            observed_end,
            &mut self.instrumentation,
            &self.label,
        )?;
        self.file = file;
        self.identity = FileIdentity::from_metadata(metadata);
        self.change_stamp = FileChangeStamp::from_metadata(metadata);
        self.epoch = self.epoch.saturating_add(1);
        self.observed_end = observed_end;
        self.committed_end = committed_end;
        self.older_cursor = committed_end;
        self.newer_cursor = committed_end;
        self.committed_sample = committed_sample;
        self.pending_sample = pending_sample;
        Ok(TimelineRefresh::Reset {
            reason,
            snapshot: self.snapshot(),
        })
    }

    fn committed_sample_still_matches(&mut self, file: &mut File) -> Result<bool> {
        if self.committed_sample.is_empty() {
            return Ok(true);
        }
        let current = read_committed_sample(
            file,
            self.committed_end,
            &mut self.instrumentation,
            &self.label,
        )?;
        Ok(current == self.committed_sample)
    }

    fn refresh_once(&mut self) -> Result<TimelineRefresh> {
        let mut replacement = File::open(&self.path)
            .with_context(|| format!("failed to reopen growing source {}", self.label))?;
        let metadata = replacement
            .metadata()
            .with_context(|| format!("failed to stat growing source {}", self.label))?;
        let identity = FileIdentity::from_metadata(&metadata);
        let change_stamp = FileChangeStamp::from_metadata(&metadata);
        if identity != self.identity {
            return self.reset(replacement, &metadata, TimelineResetReason::IdentityChanged);
        }

        let observed_end = metadata.len();
        if observed_end < self.committed_end {
            return self.reset(replacement, &metadata, TimelineResetReason::Truncated);
        }

        if self.committed_end > 0 && !self.committed_sample_still_matches(&mut replacement)? {
            return self.reset(replacement, &metadata, TimelineResetReason::Replaced);
        }

        let previous_committed_end = self.committed_end;
        let prior_pending_sample =
            if self.observed_end > previous_committed_end && observed_end >= self.observed_end {
                Some(read_pending_sample(
                    &mut replacement,
                    previous_committed_end,
                    self.observed_end,
                    &mut self.instrumentation,
                    &self.label,
                )?)
            } else {
                None
            };
        let prior_pending_matches = prior_pending_sample
            .as_ref()
            .is_none_or(|sample| sample == &self.pending_sample);
        let scan_start = if observed_end > self.observed_end {
            if prior_pending_matches {
                self.observed_end.max(previous_committed_end)
            } else {
                previous_committed_end
            }
        } else if observed_end == self.observed_end && observed_end == previous_committed_end {
            observed_end
        } else if observed_end == self.observed_end
            && change_stamp == self.change_stamp
            && prior_pending_matches
        {
            // The pending range still has the same size and cheap change
            // signals, so retain its previously established newline-free
            // boundary without rereading the record body.
            observed_end
        } else {
            // The uncommitted suffix was rewritten or truncated without
            // disturbing committed history, so its delimiter status must be
            // established again from the last committed boundary.
            previous_committed_end
        };
        let committed_end = find_last_newline_reverse(
            &mut replacement,
            scan_start,
            observed_end,
            &mut self.instrumentation,
            &self.label,
        )?
        .map_or(previous_committed_end, |offset| offset.saturating_add(1));
        let committed_sample = read_committed_sample(
            &mut replacement,
            committed_end,
            &mut self.instrumentation,
            &self.label,
        )?;
        let pending_sample =
            if committed_end == previous_committed_end && observed_end == self.observed_end {
                prior_pending_sample.unwrap_or_default()
            } else {
                read_pending_sample(
                    &mut replacement,
                    committed_end,
                    observed_end,
                    &mut self.instrumentation,
                    &self.label,
                )?
            };

        // Commit the new snapshot only after every read against the statted
        // length succeeds. A concurrent shrink therefore leaves the prior
        // snapshot intact for the retry below.
        self.file = replacement;
        self.change_stamp = change_stamp;
        self.observed_end = observed_end;
        self.committed_end = committed_end;
        self.committed_sample = committed_sample;
        self.pending_sample = pending_sample;

        let snapshot = self.snapshot();
        if self.committed_end > previous_committed_end {
            Ok(TimelineRefresh::Appended(snapshot))
        } else if snapshot.pending_bytes > 0 {
            Ok(TimelineRefresh::Pending(snapshot))
        } else {
            Ok(TimelineRefresh::NoChange(snapshot))
        }
    }
}

impl RecordTimeline for FileRecordTimeline {
    fn label(&self) -> &str {
        &self.label
    }

    fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            epoch: self.epoch,
            committed_end: self.committed_end,
            observed_end: self.observed_end,
            pending_bytes: self.observed_end.saturating_sub(self.committed_end),
        }
    }

    fn load_older(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead> {
        if self.older_cursor == 0 {
            return Ok(TimelineRead::End);
        }
        let limit = limit.normalized();
        let start = find_older_start(
            &mut self.file,
            self.older_cursor,
            limit,
            &mut self.instrumentation,
            &self.label,
        )?;
        let records = read_committed_range(
            &mut self.file,
            self.epoch,
            start,
            self.older_cursor,
            &mut self.instrumentation,
            &self.label,
        )?;
        self.older_cursor = start;
        self.instrumentation.records_yielded = self
            .instrumentation
            .records_yielded
            .saturating_add(records.len() as u64);
        Ok(TimelineRead::Records(records))
    }

    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead> {
        if self.newer_cursor >= self.committed_end {
            return Ok(TimelineRead::Pending);
        }
        let limit = limit.normalized();
        let mut records = Vec::new();
        let mut bytes = 0_usize;
        while self.newer_cursor < self.committed_end && records.len() < limit.max_records {
            let (end, raw) = read_next_record(
                &mut self.file,
                self.newer_cursor,
                self.committed_end,
                &mut self.instrumentation,
                &self.label,
            )?;
            bytes = bytes.saturating_add(raw.len());
            records.push(TimelineRecord {
                id: RecordId {
                    epoch: self.epoch,
                    start_offset: self.newer_cursor,
                    end_offset: end,
                },
                raw,
            });
            self.newer_cursor = end;
            if bytes >= limit.max_bytes {
                break;
            }
        }
        self.instrumentation.records_yielded = self
            .instrumentation
            .records_yielded
            .saturating_add(records.len() as u64);
        Ok(TimelineRead::Records(records))
    }

    fn refresh(&mut self) -> Result<TimelineRefresh> {
        for attempt in 1..=REFRESH_SHORT_READ_ATTEMPTS {
            match self.refresh_once() {
                Ok(refresh) => return Ok(refresh),
                Err(error)
                    if attempt < REFRESH_SHORT_READ_ATTEMPTS
                        && error.chain().any(|cause| {
                            cause
                                .downcast_ref::<io::Error>()
                                .is_some_and(|error| error.kind() == io::ErrorKind::UnexpectedEof)
                        }) => {}
                Err(error) => return Err(error),
            }
        }
        unreachable!("refresh retry loop always returns")
    }
}

fn find_committed_end(
    file: &mut File,
    len: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<u64> {
    if len == 0 {
        return Ok(0);
    }
    let mut end = len;
    let mut buffer = vec![0_u8; REVERSE_SCAN_CHUNK_BYTES];
    while end > 0 {
        let start = end.saturating_sub(REVERSE_SCAN_CHUNK_BYTES as u64);
        let count = usize::try_from(end - start).unwrap_or(REVERSE_SCAN_CHUNK_BYTES);
        read_exact_at(file, start, &mut buffer[..count], instrumentation, label)?;
        if let Some(index) = memrchr(b'\n', &buffer[..count]) {
            return Ok(start + index as u64 + 1);
        }
        end = start;
    }
    Ok(0)
}

fn find_last_newline_reverse(
    file: &mut File,
    start: u64,
    end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<Option<u64>> {
    if start >= end {
        return Ok(None);
    }
    let mut cursor = end;
    let mut buffer = vec![0_u8; REVERSE_SCAN_CHUNK_BYTES];
    while cursor > start {
        let chunk_start = cursor.saturating_sub(buffer.len() as u64).max(start);
        let count = usize::try_from(cursor - chunk_start).unwrap_or(buffer.len());
        read_exact_at(
            file,
            chunk_start,
            &mut buffer[..count],
            instrumentation,
            label,
        )?;
        if let Some(index) = memrchr(b'\n', &buffer[..count]) {
            return Ok(Some(chunk_start + index as u64));
        }
        cursor = chunk_start;
    }
    Ok(None)
}

fn find_older_start(
    file: &mut File,
    end: u64,
    limit: RecordLoadLimit,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<u64> {
    let mut cursor = end.saturating_sub(1);
    let mut delimiters = 0_usize;
    let mut scanned = 0_usize;
    let mut buffer = vec![0_u8; REVERSE_SCAN_CHUNK_BYTES];
    while cursor > 0 {
        let start = cursor.saturating_sub(REVERSE_SCAN_CHUNK_BYTES as u64);
        let count = usize::try_from(cursor - start).unwrap_or(REVERSE_SCAN_CHUNK_BYTES);
        read_exact_at(file, start, &mut buffer[..count], instrumentation, label)?;
        scanned = scanned.saturating_add(count);
        for index in memchr::memrchr_iter(b'\n', &buffer[..count]) {
            delimiters = delimiters.saturating_add(1);
            if delimiters >= limit.max_records || (scanned >= limit.max_bytes && delimiters >= 1) {
                return Ok(start + index as u64 + 1);
            }
        }
        cursor = start;
    }
    Ok(0)
}

fn read_committed_range(
    file: &mut File,
    epoch: u64,
    start: u64,
    end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<Vec<TimelineRecord>> {
    let len = usize::try_from(end.saturating_sub(start))
        .context("committed record batch was too large to address")?;
    let mut bytes = vec![0_u8; len];
    read_exact_at(file, start, &mut bytes, instrumentation, label)?;
    let mut records = Vec::new();
    let mut record_start = 0_usize;
    for newline in memchr::memchr_iter(b'\n', &bytes) {
        let record_end = newline + 1;
        records.push(TimelineRecord {
            id: RecordId {
                epoch,
                start_offset: start + record_start as u64,
                end_offset: start + record_end as u64,
            },
            raw: bytes[record_start..record_end].to_vec(),
        });
        record_start = record_end;
    }
    if record_start != bytes.len() {
        bail!("source {label} returned an uncommitted record from a committed range");
    }
    Ok(records)
}

fn read_next_record(
    file: &mut File,
    start: u64,
    committed_end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<(u64, Vec<u8>)> {
    let mut cursor = start;
    let mut raw = Vec::new();
    let mut buffer = vec![0_u8; FORWARD_SCAN_CHUNK_BYTES];
    while cursor < committed_end {
        let count = usize::try_from((committed_end - cursor).min(buffer.len() as u64))
            .unwrap_or(buffer.len());
        read_exact_at(file, cursor, &mut buffer[..count], instrumentation, label)?;
        if let Some(index) = memchr(b'\n', &buffer[..count]) {
            raw.extend_from_slice(&buffer[..=index]);
            return Ok((cursor + index as u64 + 1, raw));
        }
        raw.extend_from_slice(&buffer[..count]);
        cursor = cursor.saturating_add(count as u64);
    }
    bail!("source {label} ended before its committed record delimiter")
}

fn read_pending_sample(
    file: &mut File,
    start: u64,
    end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<Vec<u8>> {
    read_range_sample(file, start, end, instrumentation, label)
}

fn read_committed_sample(
    file: &mut File,
    committed_end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<Vec<u8>> {
    read_range_sample(file, 0, committed_end, instrumentation, label)
}

fn read_range_sample(
    file: &mut File,
    start: u64,
    end: u64,
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<Vec<u8>> {
    if start >= end {
        return Ok(Vec::new());
    }
    let width = (end - start).min(RANGE_SAMPLE_BYTES);
    let mut offsets = [
        start,
        start + (end - start - width) / 2,
        end.saturating_sub(width),
    ];
    offsets.sort_unstable();
    let mut sample = Vec::with_capacity(width as usize * offsets.len());
    let mut previous = None;
    for offset in offsets {
        if previous == Some(offset) {
            continue;
        }
        let sample_start = sample.len();
        sample.resize(sample_start + width as usize, 0);
        read_exact_at(
            file,
            offset,
            &mut sample[sample_start..],
            instrumentation,
            label,
        )?;
        previous = Some(offset);
    }
    Ok(sample)
}

fn read_exact_at(
    file: &mut File,
    offset: u64,
    bytes: &mut [u8],
    instrumentation: &mut FileTimelineInstrumentation,
    label: &str,
) -> Result<()> {
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("failed to seek growing source {label}"))?;
    file.read_exact(bytes)
        .with_context(|| format!("failed to read growing source {label}"))?;
    instrumentation.bytes_read = instrumentation
        .bytes_read
        .saturating_add(bytes.len() as u64);
    instrumentation.read_operations = instrumentation.read_operations.saturating_add(1);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    first: u64,
    second: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileChangeStamp {
    first: u64,
    second: u64,
}

impl FileChangeStamp {
    #[cfg(unix)]
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            first: metadata.ctime() as u64,
            second: metadata.ctime_nsec() as u64,
        }
    }

    #[cfg(not(unix))]
    fn from_metadata(metadata: &Metadata) -> Self {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::SystemTime::UNIX_EPOCH).ok());
        Self {
            first: modified.map_or(0, |duration| duration.as_secs()),
            second: modified.map_or(0, |duration| duration.subsec_nanos() as u64),
        }
    }
}

impl FileIdentity {
    #[cfg(unix)]
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            first: metadata.dev(),
            second: metadata.ino(),
        }
    }

    #[cfg(not(unix))]
    fn from_metadata(metadata: &Metadata) -> Self {
        let created = metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos() as u64);
        Self {
            first: created,
            second: 0,
        }
    }
}
