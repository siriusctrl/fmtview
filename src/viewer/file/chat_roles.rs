use anyhow::Result;

use crate::{
    formats::json::chat::{CHAT_ROLE_LOOKAHEAD_LINES, ChatRoleMark, ChatRoleTracker},
    load::ViewFile,
};

const CHECKPOINT_INTERVAL: usize = 512;
const SCAN_CHUNK_LINES: usize = 512;

#[derive(Debug, Default)]
pub(in crate::viewer) struct JsonChatRoleCache {
    checkpoints: Vec<ChatRoleCheckpoint>,
    recent: Option<ChatRoleCheckpoint>,
}

#[derive(Debug, Clone)]
struct ChatRoleCheckpoint {
    line: usize,
    tracker: ChatRoleTracker,
}

impl JsonChatRoleCache {
    pub(in crate::viewer) fn marks_for_view(
        &mut self,
        file: &dyn ViewFile,
        top: usize,
        visible_lines: &[String],
    ) -> Result<Vec<ChatRoleMark>> {
        if visible_lines.is_empty() || file.line_count() == 0 {
            return Ok(Vec::new());
        }

        let top = top.min(file.line_count().saturating_sub(1));
        let checkpoint = self.checkpoint_before(top);
        let mut tracker = checkpoint.tracker;
        let mut next_line = checkpoint.line;

        while next_line < top {
            let count = (top - next_line).min(SCAN_CHUNK_LINES);
            let lines = file.read_window(next_line, count)?;
            if lines.is_empty() {
                break;
            }
            for line in &lines {
                tracker.apply_line(line, next_line);
                next_line = next_line.saturating_add(1);
                self.remember_checkpoint(next_line, &tracker);
                if next_line >= top {
                    break;
                }
            }
        }

        self.recent = Some(ChatRoleCheckpoint {
            line: next_line,
            tracker: tracker.clone(),
        });

        let lookahead_start = top.saturating_add(visible_lines.len());
        let lookahead = file.read_window(lookahead_start, CHAT_ROLE_LOOKAHEAD_LINES)?;
        Ok(tracker.mark_lines_with_lookahead(visible_lines, &lookahead, top))
    }

    fn checkpoint_before(&self, line: usize) -> ChatRoleCheckpoint {
        let interval = self
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.line <= line);
        let recent = self
            .recent
            .as_ref()
            .filter(|checkpoint| checkpoint.line <= line);

        match (interval, recent) {
            (Some(interval), Some(recent)) if interval.line >= recent.line => interval.clone(),
            (_, Some(recent)) => recent.clone(),
            (Some(interval), None) => interval.clone(),
            (None, None) => ChatRoleCheckpoint {
                line: 0,
                tracker: ChatRoleTracker::default(),
            },
        }
    }

    fn remember_checkpoint(&mut self, line: usize, tracker: &ChatRoleTracker) {
        if line == 0 || line % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&line, |checkpoint| checkpoint.line)
        {
            Ok(index) => self.checkpoints[index].tracker = tracker.clone(),
            Err(index) => self.checkpoints.insert(
                index,
                ChatRoleCheckpoint {
                    line,
                    tracker: tracker.clone(),
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    struct TestFile {
        lines: Vec<String>,
    }

    impl ViewFile for TestFile {
        fn label(&self) -> &str {
            "test"
        }

        fn line_count(&self) -> usize {
            self.lines.len()
        }

        fn line_count_exact(&self) -> bool {
            true
        }

        fn byte_len(&self) -> u64 {
            self.lines.iter().map(|line| line.len() as u64 + 1).sum()
        }

        fn byte_offset_for_line(&self, line: usize) -> u64 {
            self.lines
                .iter()
                .take(line)
                .map(|line| line.len() as u64 + 1)
                .sum()
        }

        fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
            Ok(self.lines.iter().skip(start).take(count).cloned().collect())
        }

        fn preload(
            &self,
            _max_lines: usize,
            _max_records: usize,
            _budget: Duration,
        ) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn cache_recovers_role_for_a_view_starting_deep_inside_a_message() {
        let mut lines = vec!["[".to_owned(), "  {".to_owned()];
        lines.push(r#"    "role": "user","#.to_owned());
        for index in 0..700 {
            lines.push(format!(r#"    "field_{index}": {index},"#));
        }
        lines.extend(["  },".to_owned(), "  {".to_owned()]);
        lines.push(r#"    "content": "unlabeled""#.to_owned());
        lines.extend(["  }".to_owned(), "]".to_owned()]);
        let file = TestFile { lines };
        let mut cache = JsonChatRoleCache::default();

        let deep_lines = file.read_window(650, 4).unwrap();
        let deep = cache.marks_for_view(&file, 650, &deep_lines).unwrap();
        let unlabeled_top = file.line_count() - 3;
        let unlabeled_lines = file.read_window(unlabeled_top, 2).unwrap();
        let unlabeled = cache
            .marks_for_view(&file, unlabeled_top, &unlabeled_lines)
            .unwrap();

        assert!(
            deep.iter()
                .all(|mark| mark.role == Some(crate::formats::json::chat::ChatRole::User))
        );
        assert!(deep.iter().all(|mark| !mark.label));
        assert!(unlabeled.iter().all(|mark| mark.role.is_none()));
    }
}
