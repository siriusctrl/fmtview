use std::collections::{HashSet, VecDeque};

use anyhow::Result;

use crate::{
    formats::json::chat::{CHAT_ROLE_LOOKAHEAD_LINES, ChatRoleMark, ChatRoleTracker},
    formats::json::tool_links::{
        ToolLineMark, ToolLink, ToolLinkStatus, ToolLinkTracker, ToolRelationMark,
    },
    load::ViewFile,
};

const CHECKPOINT_INTERVAL: usize = 512;
const MAX_CHECKPOINTS: usize = 64;
const MAX_PREFIX_SCAN_LINES: usize = 4_096;
const SCAN_CHUNK_LINES: usize = 512;
const WINDOW_MIN_LINES: usize = 256;
const WINDOW_VIEW_MULTIPLIER: usize = 4;
const MAX_KNOWN_LINKS: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::viewer) struct ConversationLineMark {
    pub(in crate::viewer) role: ChatRoleMark,
    pub(in crate::viewer) tool: ToolLineMark,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::viewer) struct ConversationViewMarks {
    pub(in crate::viewer) roles: Vec<ChatRoleMark>,
    pub(in crate::viewer) tools: Vec<ToolLineMark>,
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct JsonChatRoleCache {
    role_checkpoints: VecDeque<RoleCheckpoint>,
    role_recent: Option<RoleCheckpoint>,
    tool_checkpoints: VecDeque<ToolCheckpoint>,
    tool_recent: Option<ToolCheckpoint>,
    window: Option<ConversationWindow>,
    known_links: VecDeque<ToolLink>,
}

#[derive(Debug, Clone)]
struct RoleCheckpoint {
    line: usize,
    tracker: ChatRoleTracker,
}

#[derive(Debug, Clone)]
struct ToolCheckpoint {
    line: usize,
    origin: usize,
    tracker: ToolLinkTracker,
}

#[derive(Debug)]
struct ConversationWindow {
    start: usize,
    marks: Vec<ConversationLineMark>,
    roles_resolved: bool,
}

impl JsonChatRoleCache {
    pub(in crate::viewer) fn marks_for_view(
        &mut self,
        file: &dyn ViewFile,
        top: usize,
        visible_lines: &[String],
        resolve_roles: bool,
    ) -> Result<ConversationViewMarks> {
        if visible_lines.is_empty() || file.line_count() == 0 {
            return Ok(ConversationViewMarks::default());
        }

        let top = top.min(file.line_count().saturating_sub(1));
        let requested_end = top.saturating_add(visible_lines.len());
        if !self.window.as_ref().is_some_and(|window| {
            top >= window.start
                && requested_end <= window.start.saturating_add(window.marks.len())
                && (!resolve_roles || window.roles_resolved)
        }) {
            self.fill_window(file, top, visible_lines.len(), resolve_roles)?;
        }

        let Some(window) = self.window.as_ref() else {
            return Ok(ConversationViewMarks::default());
        };
        let start = top.saturating_sub(window.start);
        let end = start
            .saturating_add(visible_lines.len())
            .min(window.marks.len());
        let mut marks = window.marks[start..end].to_vec();
        self.remember_visible_links(&marks);
        self.overlay_known_links(&mut marks, top);
        Ok(ConversationViewMarks {
            roles: marks.iter().map(|mark| mark.role).collect(),
            tools: marks.into_iter().map(|mark| mark.tool).collect(),
        })
    }

    fn fill_window(
        &mut self,
        file: &dyn ViewFile,
        top: usize,
        visible_len: usize,
        resolve_roles: bool,
    ) -> Result<()> {
        let window_len = visible_len
            .saturating_mul(WINDOW_VIEW_MULTIPLIER)
            .max(WINDOW_MIN_LINES);
        let lines = file.read_window(top, window_len)?;
        let lookahead_start = top.saturating_add(lines.len());
        let lookahead = file.read_window(lookahead_start, CHAT_ROLE_LOOKAHEAD_LINES)?;

        let role_marks = if resolve_roles {
            let checkpoint = self.role_checkpoint_before(top);
            let mut roles = self.scan_roles_to(file, checkpoint.tracker, checkpoint.line, top)?;
            let role_marks = roles.mark_lines_with_lookahead(&lines, &lookahead, top);
            self.role_recent = Some(RoleCheckpoint {
                line: lookahead_start.saturating_add(lookahead.len()),
                tracker: roles,
            });
            role_marks
        } else {
            vec![ChatRoleMark::default(); lines.len()]
        };

        let checkpoint = self.tool_checkpoint_before(top);
        let origin = checkpoint.origin;
        let mut tools =
            self.scan_tools_to(file, checkpoint.tracker, checkpoint.line, top, origin)?;
        let tool_marks = tools.mark_lines_with_lookahead(&lines, &lookahead, top);
        let marks = role_marks
            .into_iter()
            .zip(tool_marks)
            .map(|(role, tool)| ConversationLineMark { role, tool })
            .collect::<Vec<_>>();
        let scanned_end = lookahead_start.saturating_add(lookahead.len());
        self.tool_recent = Some(ToolCheckpoint {
            line: scanned_end,
            origin,
            tracker: tools,
        });
        self.window = Some(ConversationWindow {
            start: top,
            marks,
            roles_resolved: resolve_roles,
        });
        Ok(())
    }

    fn scan_roles_to(
        &mut self,
        file: &dyn ViewFile,
        mut tracker: ChatRoleTracker,
        mut next_line: usize,
        end: usize,
    ) -> Result<ChatRoleTracker> {
        while next_line < end {
            let count = (end - next_line).min(SCAN_CHUNK_LINES);
            let lines = file.read_window(next_line, count)?;
            if lines.is_empty() {
                break;
            }
            for line in &lines {
                tracker.apply_line(line, next_line);
                next_line = next_line.saturating_add(1);
                self.remember_role_checkpoint(next_line, &tracker);
                if next_line >= end {
                    break;
                }
            }
        }
        Ok(tracker)
    }

    fn scan_tools_to(
        &mut self,
        file: &dyn ViewFile,
        mut tracker: ToolLinkTracker,
        mut next_line: usize,
        end: usize,
        origin: usize,
    ) -> Result<ToolLinkTracker> {
        while next_line < end {
            let count = (end - next_line).min(SCAN_CHUNK_LINES);
            let lines = file.read_window(next_line, count)?;
            if lines.is_empty() {
                break;
            }
            for line in &lines {
                tracker.apply_line(line, next_line);
                next_line = next_line.saturating_add(1);
                self.remember_tool_checkpoint(next_line, origin, &tracker);
                if next_line >= end {
                    break;
                }
            }
        }
        Ok(tracker)
    }

    fn role_checkpoint_before(&self, line: usize) -> RoleCheckpoint {
        let interval = self
            .role_checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.line <= line)
            .max_by_key(|checkpoint| checkpoint.line);
        let recent = self
            .role_recent
            .as_ref()
            .filter(|checkpoint| checkpoint.line <= line);

        match (interval, recent) {
            (Some(interval), Some(recent)) if interval.line >= recent.line => interval.clone(),
            (_, Some(recent)) => recent.clone(),
            (Some(interval), None) => interval.clone(),
            (None, None) => RoleCheckpoint {
                line: 0,
                tracker: ChatRoleTracker::default(),
            },
        }
    }

    fn tool_checkpoint_before(&self, line: usize) -> ToolCheckpoint {
        let earliest = line.saturating_sub(MAX_PREFIX_SCAN_LINES);
        let interval = self
            .tool_checkpoints
            .iter()
            .filter(|checkpoint| {
                checkpoint.origin <= earliest
                    && checkpoint.line >= earliest
                    && checkpoint.line <= line
            })
            .max_by_key(|checkpoint| checkpoint.line);
        let recent = self.tool_recent.as_ref().filter(|checkpoint| {
            checkpoint.origin <= earliest && checkpoint.line >= earliest && checkpoint.line <= line
        });

        match (interval, recent) {
            (Some(interval), Some(recent)) if interval.line >= recent.line => interval.clone(),
            (_, Some(recent)) => recent.clone(),
            (Some(interval), None) => interval.clone(),
            (None, None) => ToolCheckpoint {
                line: earliest,
                origin: earliest,
                tracker: ToolLinkTracker::default(),
            },
        }
    }

    fn remember_role_checkpoint(&mut self, line: usize, tracker: &ChatRoleTracker) {
        if line == 0 || line % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        if let Some(index) = self
            .role_checkpoints
            .iter()
            .position(|checkpoint| checkpoint.line == line)
        {
            self.role_checkpoints.remove(index);
        }
        self.role_checkpoints.push_back(RoleCheckpoint {
            line,
            tracker: tracker.clone(),
        });
        while self.role_checkpoints.len() > MAX_CHECKPOINTS {
            self.role_checkpoints.pop_front();
        }
    }

    fn remember_tool_checkpoint(&mut self, line: usize, origin: usize, tracker: &ToolLinkTracker) {
        if line == 0 || line % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        if let Some(index) = self
            .tool_checkpoints
            .iter()
            .position(|checkpoint| checkpoint.line == line)
        {
            self.tool_checkpoints.remove(index);
        }
        self.tool_checkpoints.push_back(ToolCheckpoint {
            line,
            origin,
            tracker: tracker.clone(),
        });
        while self.tool_checkpoints.len() > MAX_CHECKPOINTS {
            self.tool_checkpoints.pop_front();
        }
    }

    fn remember_visible_links(&mut self, marks: &[ConversationLineMark]) {
        let mut seen_results = HashSet::new();
        for link in marks
            .iter()
            .filter_map(|mark| mark.tool.link.as_ref())
            .filter(|link| link.status == ToolLinkStatus::Matched)
            .filter(|link| seen_results.insert(link.result_line))
        {
            if let Some(index) = self
                .known_links
                .iter()
                .position(|known| known.result_line == link.result_line)
            {
                self.known_links.remove(index);
            }
            self.known_links.push_back(link.clone());
            while self.known_links.len() > MAX_KNOWN_LINKS {
                self.known_links.pop_front();
            }
        }
    }

    fn overlay_known_links(&self, marks: &mut [ConversationLineMark], first_line: usize) {
        for link in &self.known_links {
            let Some(call_line) = link.call_line else {
                continue;
            };
            if let Some(mark) = conversation_mark_at_line(marks, first_line, call_line) {
                mark.tool.relation = ToolRelationMark::MatchedCall;
                mark.tool.link = Some(link.clone());
            }
            if let Some(mark) = conversation_mark_at_line(marks, first_line, link.result_line) {
                mark.tool.relation = ToolRelationMark::MatchedResult;
                mark.tool.link = Some(link.clone());
            }
        }
    }
}

fn conversation_mark_at_line(
    marks: &mut [ConversationLineMark],
    first_line: usize,
    line: usize,
) -> Option<&mut ConversationLineMark> {
    marks.get_mut(line.checked_sub(first_line)?)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };

    use super::*;

    struct TestFile {
        lines: Vec<String>,
        lines_read: Cell<usize>,
    }

    impl TestFile {
        fn new(lines: Vec<String>) -> Self {
            Self {
                lines,
                lines_read: Cell::new(0),
            }
        }
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
            let lines = self
                .lines
                .iter()
                .skip(start)
                .take(count)
                .cloned()
                .collect::<Vec<_>>();
            self.lines_read
                .set(self.lines_read.get().saturating_add(lines.len()));
            Ok(lines)
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
        let file = TestFile::new(lines);
        let mut cache = JsonChatRoleCache::default();

        let deep_lines = file.read_window(650, 4).unwrap();
        let deep = cache.marks_for_view(&file, 650, &deep_lines, true).unwrap();
        let unlabeled_top = file.line_count() - 3;
        let unlabeled_lines = file.read_window(unlabeled_top, 2).unwrap();
        let unlabeled = cache
            .marks_for_view(&file, unlabeled_top, &unlabeled_lines, true)
            .unwrap();

        assert!(
            deep.roles
                .iter()
                .all(|mark| mark.role == Some(crate::formats::json::chat::ChatRole::User))
        );
        assert!(deep.roles.iter().all(|mark| !mark.label));
        assert!(unlabeled.roles.iter().all(|mark| mark.role.is_none()));
    }

    #[test]
    fn role_recovery_remains_exact_beyond_the_tool_prefix_horizon() {
        let mut lines = vec!["[".to_owned(), "  {".to_owned()];
        lines.push(r#"    "role": "user","#.to_owned());
        for index in 0..5_000 {
            lines.push(format!(r#"    "field_{index}": {index},"#));
        }
        lines.extend(["  }".to_owned(), "]".to_owned()]);
        let file = TestFile::new(lines);
        let top = 4_900;
        let visible = file.lines[top..top + 4].to_vec();
        let mut cache = JsonChatRoleCache::default();

        let marks = cache.marks_for_view(&file, top, &visible, true).unwrap();

        assert!(
            marks
                .roles
                .iter()
                .all(|mark| mark.role == Some(crate::formats::json::chat::ChatRole::User))
        );
    }

    #[test]
    fn cache_links_tool_results_and_marks_only_the_direction_endpoints() {
        let lines = [
            "[",
            "  {",
            r#"    "role": "assistant","#,
            r#"    "tool_calls": ["#,
            r#"      {"id": "call_7", "name": "lookup"}"#,
            "    ]",
            "  },",
            "  {",
            r#"    "role": "tool","#,
            r#"    "tool_call_id": "call_7","#,
            r#"    "content": "ok""#,
            "  }",
            "]",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let file = TestFile::new(lines);
        let mut cache = JsonChatRoleCache::default();
        let visible = file.read_window(0, file.line_count()).unwrap();

        let marks = cache.marks_for_view(&file, 0, &visible, true).unwrap();
        let link = marks.tools[7].link.as_ref().unwrap().clone();
        assert_eq!(link.call_line, Some(4));
        assert_eq!(link.result_line, 7);
        assert_eq!(marks.tools[4].relation, ToolRelationMark::MatchedCall);
        assert_eq!(marks.tools[7].relation, ToolRelationMark::MatchedResult);
        assert_eq!(marks.tools[5].relation, ToolRelationMark::None);

        assert_eq!(link.status, ToolLinkStatus::Matched);
    }

    #[test]
    fn cache_matches_tool_result_beyond_the_lookahead_window() {
        let mut lines = vec![r#"{"tool_calls":[{"id":"call_far"}]}"#.to_owned()];
        for index in 0..700 {
            lines.push(format!(r#"{{"event":{index}}}"#));
        }
        let result_line = lines.len();
        lines.push(r#"{"role":"tool","tool_call_id":"call_far","content":"ok"}"#.to_owned());
        let file = TestFile::new(lines);
        let visible = file.read_window(result_line, 1).unwrap();
        let mut cache = JsonChatRoleCache::default();

        let marks = cache
            .marks_for_view(&file, result_line, &visible, false)
            .unwrap();

        let link = marks.tools[0].link.as_ref().unwrap();
        assert_eq!(link.call_line, Some(0));
        assert_eq!(link.result_line, result_line);
        assert_eq!(link.status, ToolLinkStatus::Matched);
    }

    #[test]
    fn cold_deep_view_scans_a_bounded_prefix_and_caps_checkpoints() {
        let lines = (0..20_000)
            .map(|index| format!(r#"{{"event":{index}}}"#))
            .collect::<Vec<_>>();
        let file = TestFile::new(lines);
        let top = 19_000;
        let visible = file.lines[top..top + 48].to_vec();
        let mut cache = JsonChatRoleCache::default();

        cache.marks_for_view(&file, top, &visible, false).unwrap();

        assert!(
            file.lines_read.get()
                <= MAX_PREFIX_SCAN_LINES + WINDOW_MIN_LINES + CHAT_ROLE_LOOKAHEAD_LINES
        );
        assert!(cache.tool_checkpoints.len() <= MAX_CHECKPOINTS);
    }

    #[test]
    fn truncated_tool_checkpoints_are_not_reused_outside_their_origin() {
        let mut lines = (0..12_000)
            .map(|index| format!(r#"{{"event":{index}}}"#))
            .collect::<Vec<_>>();
        lines[5_000] = r#"{"tool_calls":[{"id":"call_back"}]}"#.to_owned();
        lines[6_500] = r#"{"role":"tool","tool_call_id":"call_back"}"#.to_owned();
        let file = TestFile::new(lines);
        let mut cache = JsonChatRoleCache::default();

        let deep = file.lines[10_000..10_048].to_vec();
        cache.marks_for_view(&file, 10_000, &deep, false).unwrap();
        let result = file.lines[6_500..6_501].to_vec();
        let marks = cache.marks_for_view(&file, 6_500, &result, false).unwrap();

        assert_eq!(marks.tools[0].link.as_ref().unwrap().call_line, Some(5_000));
    }

    #[test]
    fn checkpoint_history_is_bounded_by_recent_use() {
        let roles = ChatRoleTracker::default();
        let tools = ToolLinkTracker::default();
        let mut cache = JsonChatRoleCache::default();

        for index in 1..=MAX_CHECKPOINTS + 8 {
            let line = index * CHECKPOINT_INTERVAL;
            cache.remember_role_checkpoint(line, &roles);
            cache.remember_tool_checkpoint(line, 0, &tools);
        }

        assert_eq!(cache.role_checkpoints.len(), MAX_CHECKPOINTS);
        assert_eq!(cache.tool_checkpoints.len(), MAX_CHECKPOINTS);
        assert_eq!(
            cache.role_checkpoints.front().unwrap().line,
            9 * CHECKPOINT_INTERVAL
        );
        assert_eq!(
            cache.tool_checkpoints.front().unwrap().line,
            9 * CHECKPOINT_INTERVAL
        );
    }

    #[test]
    #[ignore = "viewer performance smoke"]
    fn perf_chat_context_adjacent_scroll() {
        let mut lines = vec!["[".to_owned()];
        for message in 0..1_200 {
            let role = match message % 4 {
                0 => "system",
                1 => "user",
                2 => "assistant",
                _ => "tool",
            };
            lines.extend([
                "  {".to_owned(),
                format!(r#"    "role": "{role}","#),
                format!(r#"    "content": "message {message}","#),
                r#"    "metadata": {"index": true}"#.to_owned(),
                "  },".to_owned(),
            ]);
        }
        lines.push("]".to_owned());
        let file = TestFile::new(lines);
        let mut cache = JsonChatRoleCache::default();
        let started = Instant::now();
        let mut marked = 0_usize;

        for top in 0..800 {
            let visible = file.read_window(top, 48).unwrap();
            marked += cache
                .marks_for_view(&file, top, &visible, true)
                .unwrap()
                .roles
                .iter()
                .filter(|mark| mark.role.is_some())
                .count();
        }

        println!(
            "chat context adjacent scroll: {:?}, windows=800, marked={marked}, bytes=0, background_cells=0",
            started.elapsed()
        );
        assert!(marked > 30_000);
    }

    #[test]
    #[ignore = "viewer performance smoke"]
    fn perf_tool_context_adjacent_scroll() {
        let mut lines = vec!["[".to_owned()];
        for exchange in 0..600 {
            lines.extend([
                "  {".to_owned(),
                r#"    "role": "assistant","#.to_owned(),
                format!(r#"    "tool_calls": [{{"id":"call_{exchange}"}}]"#),
                "  },".to_owned(),
                "  {".to_owned(),
                r#"    "role": "tool","#.to_owned(),
                format!(r#"    "tool_call_id": "call_{exchange}","#),
                r#"    "content": "ok""#.to_owned(),
                "  },".to_owned(),
            ]);
        }
        lines.push("]".to_owned());
        let file = TestFile::new(lines);
        let mut cache = JsonChatRoleCache::default();
        let started = Instant::now();
        let mut linked = 0_usize;

        for top in 0..800 {
            let visible = file.read_window(top, 48).unwrap();
            linked += cache
                .marks_for_view(&file, top, &visible, false)
                .unwrap()
                .tools
                .iter()
                .filter(|mark| mark.link.is_some())
                .count();
        }

        println!(
            "tool context adjacent scroll: {:?}, windows=800, linked={linked}, bytes=0, background_cells=0",
            started.elapsed()
        );
        assert!(linked > 20_000);
    }

    #[test]
    #[ignore = "viewer performance smoke"]
    fn perf_tool_context_cold_deep_jump() {
        let lines = (0..100_000)
            .map(|index| format!(r#"{{"event":{index}}}"#))
            .collect::<Vec<_>>();
        let file = TestFile::new(lines);
        let top = 95_000;
        let visible = file.lines[top..top + 48].to_vec();
        let mut cache = JsonChatRoleCache::default();
        let started = Instant::now();

        cache.marks_for_view(&file, top, &visible, false).unwrap();

        let lines_read = file.lines_read.get();
        println!(
            "tool context cold deep jump: {:?}, top={top}, lines_read={lines_read}, bytes=0, background_cells=0",
            started.elapsed()
        );
        assert!(lines_read <= MAX_PREFIX_SCAN_LINES + WINDOW_MIN_LINES + CHAT_ROLE_LOOKAHEAD_LINES);
    }
}
