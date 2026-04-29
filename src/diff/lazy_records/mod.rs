use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{input::InputSource, transform::FormatOptions};

use super::{DiffModel, UnifiedDiffRow};

mod reader;
mod rows;

use reader::{FormattedRecord, LazyRecordReader};
use rows::{
    FormattedContextRecord, append_context_record, append_context_records, append_omitted_context,
    find_sync_record, full_line_change, scanning_message, scanning_model,
};

const CONTEXT_RECORDS: usize = 3;
const RESYNC_LOOKAHEAD_RECORDS: usize = 32;

pub(crate) struct LazyRecordDiff {
    left_label: String,
    right_label: String,
    left: LazyRecordReader,
    right: LazyRecordReader,
    rows: Vec<UnifiedDiffRow>,
    pending_context: VecDeque<FormattedContextRecord>,
    model: DiffModel,
    left_line: usize,
    right_line: usize,
    records_read: usize,
    pending_equal_records: usize,
    saw_change: bool,
    complete: bool,
}

impl LazyRecordDiff {
    pub(crate) fn new(
        left: &InputSource,
        right: &InputSource,
        options: FormatOptions,
    ) -> Result<Self> {
        let left_label = left.label().to_owned();
        let right_label = right.label().to_owned();
        let model = scanning_model(&left_label, &right_label, 0);
        Ok(Self {
            left_label,
            right_label,
            left: LazyRecordReader::new(left, options)?,
            right: LazyRecordReader::new(right, options)?,
            rows: Vec::new(),
            pending_context: VecDeque::new(),
            model,
            left_line: 1,
            right_line: 1,
            records_read: 0,
            pending_equal_records: 0,
            saw_change: false,
            complete: false,
        })
    }

    pub(crate) fn model(&self) -> &DiffModel {
        &self.model
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }

    pub(crate) fn preload(&mut self, max_records: usize, budget: Duration) -> Result<bool> {
        if self.complete || max_records == 0 || budget.is_zero() {
            return Ok(false);
        }

        let started = Instant::now();
        let before_rows = self.rows.len();
        let before_records = self.records_read;
        let before_complete = self.complete;
        let mut processed = 0_usize;

        while processed < max_records && started.elapsed() < budget && !self.complete {
            if !self.read_next_pair()? {
                break;
            }
            processed += 1;
        }

        let changed = before_rows != self.rows.len()
            || before_records != self.records_read
            || before_complete != self.complete;
        if changed {
            self.refresh_model();
        }
        Ok(changed)
    }

    fn read_next_pair(&mut self) -> Result<bool> {
        let left = self.left.read_record()?;
        let right = self.right.read_record()?;

        if left.is_none() && right.is_none() {
            self.complete = true;
            return Ok(false);
        }

        match (left, right) {
            (Some(left), Some(right)) if left == right => {
                self.push_equal_record(left);
                self.records_read = self.records_read.saturating_add(1);
            }
            (Some(left), Some(right)) => {
                self.push_changed_records(left, right)?;
            }
            (Some(left), None) => {
                self.begin_change();
                self.push_deleted_record(left);
                self.records_read = self.records_read.saturating_add(1);
            }
            (None, Some(right)) => {
                self.begin_change();
                self.push_inserted_record(right);
                self.records_read = self.records_read.saturating_add(1);
            }
            (None, None) => unreachable!("both EOF was handled before matching"),
        }
        Ok(true)
    }

    fn push_changed_records(
        &mut self,
        left: FormattedRecord,
        right: FormattedRecord,
    ) -> Result<()> {
        let mut left_window = vec![left];
        let mut right_window = vec![right];
        self.left
            .fill_window(&mut left_window, RESYNC_LOOKAHEAD_RECORDS)?;
        self.right
            .fill_window(&mut right_window, RESYNC_LOOKAHEAD_RECORDS)?;

        let sync = find_sync_record(&left_window, &right_window);
        let (consume_left, consume_right, consume_sync) = sync
            .map(|(left, right)| (left, right, true))
            .unwrap_or_else(|| {
                if left_window.len() < RESYNC_LOOKAHEAD_RECORDS
                    && right_window.len() < RESYNC_LOOKAHEAD_RECORDS
                {
                    (left_window.len(), right_window.len(), false)
                } else {
                    (1, 1, false)
                }
            });

        self.begin_change();
        for raw in left_window.drain(..consume_left) {
            self.push_deleted_record(raw);
        }
        for raw in right_window.drain(..consume_right) {
            self.push_inserted_record(raw);
        }

        if consume_sync {
            let left = left_window.remove(0);
            let right = right_window.remove(0);
            debug_assert_eq!(left, right);
            self.push_equal_record(left);
        }

        let consumed = consume_left.max(consume_right) + usize::from(consume_sync);
        self.records_read = self.records_read.saturating_add(consumed.max(1));
        self.left.unread_front(left_window);
        self.right.unread_front(right_window);
        Ok(())
    }

    fn push_equal_record(&mut self, record: FormattedRecord) {
        let record = FormattedContextRecord {
            left_start: self.left_line,
            right_start: self.right_line,
            lines: record.lines,
        };
        let line_count = record.lines.len();
        self.left_line = self.left_line.saturating_add(line_count);
        self.right_line = self.right_line.saturating_add(line_count);
        self.pending_equal_records = self.pending_equal_records.saturating_add(1);
        self.pending_context.push_back(record);
        while self.pending_context.len() > CONTEXT_RECORDS {
            self.pending_context.pop_front();
        }
    }

    fn push_deleted_record(&mut self, record: FormattedRecord) {
        let line_count = record.lines.len();
        let change = full_line_change(record.lines.iter().map(String::as_str));
        for (offset, line) in record.lines.into_iter().enumerate() {
            self.rows.push(UnifiedDiffRow::Delete {
                left: self.left_line + offset,
                content: Arc::from(line),
                change,
            });
        }
        self.left_line = self.left_line.saturating_add(line_count);
    }

    fn push_inserted_record(&mut self, record: FormattedRecord) {
        let line_count = record.lines.len();
        let change = full_line_change(record.lines.iter().map(String::as_str));
        for (offset, line) in record.lines.into_iter().enumerate() {
            self.rows.push(UnifiedDiffRow::Insert {
                right: self.right_line + offset,
                content: Arc::from(line),
                change,
            });
        }
        self.right_line = self.right_line.saturating_add(line_count);
    }

    fn begin_change(&mut self) {
        if self.saw_change {
            append_omitted_context(
                &mut self.rows,
                self.pending_equal_records
                    .saturating_sub(self.pending_context.len()),
            );
        }
        self.flush_pending_context();
        self.pending_equal_records = 0;
        self.saw_change = true;
    }

    fn flush_pending_context(&mut self) {
        while let Some(record) = self.pending_context.pop_front() {
            append_context_record(&mut self.rows, record);
        }
    }

    fn refresh_model(&mut self) {
        self.model = if self.rows.is_empty() && !self.complete {
            scanning_model(&self.left_label, &self.right_label, self.records_read)
        } else {
            let mut rows = self.rows.clone();
            if self.saw_change && self.pending_equal_records > 0 {
                append_omitted_context(
                    &mut rows,
                    self.pending_equal_records
                        .saturating_sub(self.pending_context.len()),
                );
                append_context_records(&mut rows, self.pending_context.iter().cloned());
            }
            if !self.complete {
                rows.push(UnifiedDiffRow::Message {
                    text: scanning_message(self.records_read),
                });
            }
            DiffModel::from_rows(self.left_label.clone(), self.right_label.clone(), rows)
        };
    }
}
