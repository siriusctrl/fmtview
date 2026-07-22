use fmtview::view::{
    self, RecordId, RecordLoadLimit, RecordTimeline, Result, TimelineRead, TimelineReadNext,
    TimelineRecord, TimelineRefresh, TimelineSnapshot, ViewOptions,
};

struct DemoTimeline {
    records: Vec<TimelineRecord>,
    older_cursor: usize,
    newer_cursor: usize,
    end_offset: u64,
}

impl DemoTimeline {
    fn new(records: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut offset = 0_u64;
        let records = records
            .into_iter()
            .map(|raw| {
                let start_offset = offset;
                offset += raw.len() as u64;
                TimelineRecord {
                    id: RecordId {
                        epoch: 1,
                        start_offset,
                        end_offset: offset,
                    },
                    raw,
                }
            })
            .collect::<Vec<_>>();
        let cursor = records.len();
        Self {
            records,
            older_cursor: cursor,
            newer_cursor: cursor,
            end_offset: offset,
        }
    }

    fn snapshot_value(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            epoch: 1,
            committed_end: self.end_offset,
            observed_end: self.end_offset,
            pending_bytes: 0,
        }
    }

    fn forward_batch(&self, start: usize, limit: RecordLoadLimit) -> Vec<TimelineRecord> {
        let mut bytes = 0_usize;
        self.records[start..]
            .iter()
            .take(limit.max_records.max(1))
            .take_while(|record| {
                if bytes > 0 && bytes.saturating_add(record.raw.len()) > limit.max_bytes.max(1) {
                    return false;
                }
                bytes = bytes.saturating_add(record.raw.len());
                true
            })
            .cloned()
            .collect()
    }
}

impl RecordTimeline for DemoTimeline {
    fn label(&self) -> &str {
        "embedded conversation"
    }

    fn snapshot(&self) -> TimelineSnapshot {
        self.snapshot_value()
    }

    fn probe_prefix(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead> {
        let records = self.forward_batch(0, limit);
        if records.is_empty() {
            return Ok(TimelineRead::End);
        }
        let next = if records.len() == self.records.len() {
            TimelineReadNext::End
        } else {
            TimelineReadNext::More
        };
        Ok(TimelineRead::Records { records, next })
    }

    fn load_older(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead> {
        if self.older_cursor == 0 {
            return Ok(TimelineRead::End);
        }

        let mut start = self.older_cursor;
        let mut bytes = 0_usize;
        let mut count = 0_usize;
        while start > 0 && count < limit.max_records.max(1) {
            let next = &self.records[start - 1];
            if count > 0 && bytes.saturating_add(next.raw.len()) > limit.max_bytes.max(1) {
                break;
            }
            start -= 1;
            count += 1;
            bytes = bytes.saturating_add(next.raw.len());
        }
        let records = self.records[start..self.older_cursor].to_vec();
        self.older_cursor = start;
        let next = if start == 0 {
            TimelineReadNext::End
        } else {
            TimelineReadNext::More
        };
        Ok(TimelineRead::Records { records, next })
    }

    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead> {
        if self.newer_cursor == self.records.len() {
            return Ok(TimelineRead::End);
        }
        let records = self.forward_batch(self.newer_cursor, limit);
        self.newer_cursor += records.len();
        let next = if self.newer_cursor == self.records.len() {
            TimelineReadNext::End
        } else {
            TimelineReadNext::More
        };
        Ok(TimelineRead::Records { records, next })
    }

    fn refresh(&mut self) -> Result<TimelineRefresh> {
        Ok(TimelineRefresh::End(self.snapshot_value()))
    }
}

fn main() -> Result<()> {
    let timeline = DemoTimeline::new([
        b"{\"ref\":\"m1\",\"role\":\"user\",\"content\":\"run the tests\"}\n".to_vec(),
        b"{\"ref\":\"m2\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_call\",\"id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"{\\\"cmd\\\":\\\"cargo test\\\"}\"}]}\n".to_vec(),
        b"{\"ref\":\"m3\",\"role\":\"tool\",\"content\":[{\"type\":\"tool_result\",\"call_id\":\"call_1\",\"content\":\"ok\"}]}\n".to_vec(),
    ]);
    let mut options = ViewOptions::default();
    options.notice = Some("embedded through fmtview::view".to_owned());
    view::run(Box::new(timeline), options)
}
