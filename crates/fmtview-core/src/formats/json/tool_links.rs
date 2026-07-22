use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

mod scanner;

use scanner::{JsonProperty, property_at, starts_new_root, string_end};

const MAX_TOOL_ID_BYTES: usize = 256;
const MAX_TOOL_ID_KEY_BYTES: usize = 64;
const MAX_IDS_PER_CONTAINER: usize = 16;
const MAX_PENDING_CALLS: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ToolRelationMark {
    #[default]
    None,
    MatchedCall,
    MatchedResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolLinkStatus {
    Matched,
    Unmatched,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolLink {
    pub(crate) id: Arc<str>,
    pub(crate) call_line: Option<usize>,
    pub(crate) result_line: usize,
    pub(crate) status: ToolLinkStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolLineMark {
    pub(crate) relation: ToolRelationMark,
    pub(crate) link: Option<ToolLink>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolLinkTracker {
    containers: Vec<ToolContainer>,
    pending_calls: VecDeque<PendingCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolContainer {
    kind: ContainerKind,
    context: ContainerContext,
    start_line: usize,
    role_tool: bool,
    toolish_type: bool,
    tool_result_type: bool,
    ids: Vec<IdCandidate>,
    provisional_link: Option<ToolLink>,
    pending_child: Option<ContainerContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Object,
    Array,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ContainerContext {
    #[default]
    Other,
    ToolCallCollection,
    ToolCallObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdCandidate {
    key: String,
    value: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCall {
    ids: Vec<IdCandidate>,
    line: usize,
}

#[derive(Debug, Clone)]
struct ToolDiscovery {
    start_line: usize,
    link: ToolLink,
}

#[derive(Debug)]
enum ToolMatchDecision {
    Matched {
        id: Arc<str>,
        call_index: usize,
    },
    Ambiguous {
        id: Arc<str>,
        call_indexes: Vec<usize>,
    },
    Unmatched {
        id: Arc<str>,
    },
}

impl ToolLinkTracker {
    pub(crate) fn apply_line(&mut self, line: &str, line_number: usize) {
        self.scan_line(line, line_number);
    }

    #[cfg(test)]
    pub(crate) fn mark_lines(&mut self, lines: &[String], first_line: usize) -> Vec<ToolLineMark> {
        self.mark_lines_with_lookahead(lines, &[], first_line)
    }

    pub(crate) fn mark_lines_with_lookahead(
        &mut self,
        visible_lines: &[String],
        lookahead_lines: &[String],
        first_line: usize,
    ) -> Vec<ToolLineMark> {
        let mut marks = vec![ToolLineMark::default(); visible_lines.len()];
        let mut results = BTreeMap::<usize, (usize, ToolLink)>::new();

        for (offset, line) in visible_lines.iter().chain(lookahead_lines).enumerate() {
            let line_number = first_line.saturating_add(offset);
            for ToolDiscovery { start_line, link } in self.scan_line(line, line_number) {
                results.insert(start_line, (offset, link));
            }
            if let Some((start_line, link)) = self.active_result() {
                results.insert(start_line, (offset, link.clone()));
            }
        }

        for (start_line, (last_offset, link)) in results {
            let start = start_line.saturating_sub(first_line);
            let end = last_offset.saturating_add(1).min(visible_lines.len());
            for mark in marks.iter_mut().take(end).skip(start) {
                mark.link = Some(link.clone());
            }
            if let Some(mark) = mark_at_line(&mut marks, first_line, start_line) {
                mark.relation = result_relation(link.status);
            }
            if let Some(call_line) = link.call_line
                && let Some(mark) = mark_at_line(&mut marks, first_line, call_line)
            {
                mark.relation = ToolRelationMark::MatchedCall;
                mark.link = Some(link);
            }
        }

        marks
    }

    fn scan_line(&mut self, line: &str, line_number: usize) -> Vec<ToolDiscovery> {
        if starts_new_root(line) && !self.containers.is_empty() {
            self.containers.clear();
        }

        let mut discoveries = Vec::new();
        let mut cursor = 0_usize;
        while cursor < line.len() {
            let Some(ch) = line[cursor..].chars().next() else {
                break;
            };
            match ch {
                '"' => {
                    let Some(end) = string_end(line, cursor) else {
                        break;
                    };
                    if self
                        .containers
                        .last()
                        .is_some_and(|container| container.kind == ContainerKind::Object)
                        && let Some(property) = property_at(line, cursor, end)
                    {
                        let value_end = property.value_end;
                        self.apply_property(property);
                        self.refresh_active_result();
                        cursor = value_end;
                        continue;
                    }
                    cursor = end;
                    continue;
                }
                '{' => self.push_container(ContainerKind::Object, line_number),
                '[' => self.push_container(ContainerKind::Array, line_number),
                '}' => {
                    if let Some(discovery) = self.pop_container(ContainerKind::Object) {
                        discoveries.push(discovery);
                    }
                }
                ']' => {
                    self.pop_container(ContainerKind::Array);
                }
                _ => {}
            }
            cursor += ch.len_utf8();
        }
        discoveries
    }

    fn push_container(&mut self, kind: ContainerKind, line_number: usize) {
        let context = self
            .containers
            .last_mut()
            .and_then(|parent| parent.pending_child.take())
            .or_else(|| {
                self.containers.last().and_then(|parent| {
                    (parent.kind == ContainerKind::Array
                        && parent.context == ContainerContext::ToolCallCollection
                        && kind == ContainerKind::Object)
                        .then_some(ContainerContext::ToolCallObject)
                })
            })
            .unwrap_or_default();
        self.containers.push(ToolContainer {
            kind,
            context,
            start_line: line_number,
            role_tool: false,
            toolish_type: false,
            tool_result_type: false,
            ids: Vec::new(),
            provisional_link: None,
            pending_child: None,
        });
    }

    fn pop_container(&mut self, expected: ContainerKind) -> Option<ToolDiscovery> {
        while let Some(container) = self.containers.pop() {
            if container.kind != expected {
                continue;
            }
            if expected == ContainerKind::Object {
                if container.role_tool || container.tool_result_type {
                    let link = self.link_result(container.start_line, &container.ids);
                    return link.map(|link| ToolDiscovery {
                        start_line: container.start_line,
                        link,
                    });
                }
                if (container.context == ContainerContext::ToolCallObject || container.toolish_type)
                    && !container.ids.is_empty()
                {
                    self.pending_calls.push_back(PendingCall {
                        ids: container.ids,
                        line: container.start_line,
                    });
                    while self.pending_calls.len() > MAX_PENDING_CALLS {
                        self.pending_calls.pop_front();
                    }
                    return None;
                }
            }
            break;
        }
        None
    }

    fn apply_property(&mut self, property: JsonProperty) {
        let Some(container) = self.containers.last_mut() else {
            return;
        };
        let key = property.key;
        if let Some(value) = property.string_value {
            match key.as_str() {
                "role" => container.role_tool = value.eq_ignore_ascii_case("tool"),
                "type" => {
                    container.toolish_type = is_tool_call_type(&value);
                    container.tool_result_type = is_tool_result_type(&value);
                }
                _ if is_id_key(&key)
                    && key.len() <= MAX_TOOL_ID_KEY_BYTES
                    && value.len() <= MAX_TOOL_ID_BYTES =>
                {
                    remember_id_candidate(&mut container.ids, IdCandidate { key, value });
                }
                _ => {}
            }
        } else if property.child_container
            && let Some(context) = child_context(&key)
        {
            container.pending_child = Some(context);
        }
    }

    fn active_result(&self) -> Option<(usize, &ToolLink)> {
        self.containers.iter().rev().find_map(|container| {
            container
                .provisional_link
                .as_ref()
                .map(|link| (container.start_line, link))
        })
    }

    fn refresh_active_result(&mut self) {
        let Some(container) = self.containers.last() else {
            return;
        };
        let link = (container.role_tool || container.tool_result_type)
            .then(|| self.preview_result(container.start_line, &container.ids))
            .flatten();
        if let Some(container) = self.containers.last_mut() {
            container.provisional_link = link;
        }
    }

    fn link_result(&mut self, result_line: usize, ids: &[IdCandidate]) -> Option<ToolLink> {
        let decision = self.result_decision(ids)?;
        let link = self.link_from_decision(result_line, &decision);
        match &decision {
            ToolMatchDecision::Matched { call_index, .. } => {
                self.remove_pending_calls(&[*call_index]);
            }
            ToolMatchDecision::Ambiguous { call_indexes, .. } => {
                self.remove_pending_calls(call_indexes);
            }
            ToolMatchDecision::Unmatched { .. } => {}
        }
        Some(link)
    }

    fn preview_result(&self, result_line: usize, ids: &[IdCandidate]) -> Option<ToolLink> {
        let first_rank = result_candidates(ids)
            .next()
            .map(|candidate| result_id_rank(&candidate.key))?;
        let decision = self.result_decision(ids)?;
        if first_rank < 2 && !matches!(decision, ToolMatchDecision::Matched { .. }) {
            return None;
        }
        Some(self.link_from_decision(result_line, &decision))
    }

    fn result_decision(&self, ids: &[IdCandidate]) -> Option<ToolMatchDecision> {
        let candidates = result_candidates(ids).collect::<Vec<_>>();
        let first_id = candidates.first()?.value.clone();

        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let matching_indexes = self.matching_call_indexes(candidate);
            if matching_indexes.is_empty() {
                if candidate_index == 0 && result_id_rank(&candidate.key) >= 4 {
                    return Some(ToolMatchDecision::Unmatched {
                        id: candidate.value.clone(),
                    });
                }
                continue;
            }
            if matching_indexes.len() == 1 {
                return Some(ToolMatchDecision::Matched {
                    id: candidate.value.clone(),
                    call_index: matching_indexes[0],
                });
            }

            let mut narrowed = matching_indexes;
            for lower in candidates.iter().skip(candidate_index + 1) {
                let lower_indexes = self.matching_call_indexes(lower);
                let intersection = narrowed
                    .iter()
                    .copied()
                    .filter(|index| lower_indexes.contains(index))
                    .collect::<Vec<_>>();
                if intersection.len() == 1 {
                    return Some(ToolMatchDecision::Matched {
                        id: lower.value.clone(),
                        call_index: intersection[0],
                    });
                }
                if !intersection.is_empty() {
                    narrowed = intersection;
                }
            }
            return Some(ToolMatchDecision::Ambiguous {
                id: candidate.value.clone(),
                call_indexes: narrowed,
            });
        }

        Some(ToolMatchDecision::Unmatched { id: first_id })
    }

    fn link_from_decision(&self, result_line: usize, decision: &ToolMatchDecision) -> ToolLink {
        match decision {
            ToolMatchDecision::Matched { id, call_index } => ToolLink {
                id: id.clone(),
                call_line: Some(self.pending_calls[*call_index].line),
                result_line,
                status: ToolLinkStatus::Matched,
            },
            ToolMatchDecision::Ambiguous { id, .. } => ToolLink {
                id: id.clone(),
                call_line: None,
                result_line,
                status: ToolLinkStatus::Ambiguous,
            },
            ToolMatchDecision::Unmatched { id } => ToolLink {
                id: id.clone(),
                call_line: None,
                result_line,
                status: ToolLinkStatus::Unmatched,
            },
        }
    }

    fn matching_call_indexes(&self, result_id: &IdCandidate) -> Vec<usize> {
        self.pending_calls
            .iter()
            .enumerate()
            .filter(|(_, call)| {
                call.ids
                    .iter()
                    .any(|call_id| ids_are_compatible(call_id, result_id))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn remove_pending_calls(&mut self, matching_indexes: &[usize]) {
        let mut index = 0_usize;
        self.pending_calls.retain(|_| {
            let keep = !matching_indexes.contains(&index);
            index = index.saturating_add(1);
            keep
        });
    }
}

fn mark_at_line(
    marks: &mut [ToolLineMark],
    first_line: usize,
    line: usize,
) -> Option<&mut ToolLineMark> {
    marks.get_mut(line.checked_sub(first_line)?)
}

fn result_relation(status: ToolLinkStatus) -> ToolRelationMark {
    match status {
        ToolLinkStatus::Matched => ToolRelationMark::MatchedResult,
        ToolLinkStatus::Unmatched | ToolLinkStatus::Ambiguous => ToolRelationMark::None,
    }
}

fn remember_id_candidate(ids: &mut Vec<IdCandidate>, candidate: IdCandidate) {
    if ids
        .iter()
        .any(|known| known.key == candidate.key && known.value == candidate.value)
    {
        return;
    }
    if ids.len() < MAX_IDS_PER_CONTAINER {
        ids.push(candidate);
        return;
    }

    let Some((lowest_index, lowest_rank)) = ids
        .iter()
        .enumerate()
        .map(|(index, known)| (index, storage_id_rank(&known.key)))
        .min_by_key(|(_, rank)| *rank)
    else {
        return;
    };
    if storage_id_rank(&candidate.key) > lowest_rank {
        ids[lowest_index] = candidate;
    }
}

fn storage_id_rank(key: &str) -> u8 {
    if matches!(
        key,
        "tool_call_id" | "tool_use_id" | "call_id" | "invocation_id"
    ) {
        6
    } else if key.eq_ignore_ascii_case("id") {
        5
    } else if is_toolish_id_key(key) {
        4
    } else {
        result_id_rank(key)
    }
}

fn result_candidates(ids: &[IdCandidate]) -> impl Iterator<Item = &IdCandidate> {
    let mut candidates = ids.iter().collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(result_id_rank(&candidate.key)));
    candidates.into_iter()
}

fn result_id_rank(key: &str) -> u8 {
    if matches!(
        key,
        "tool_call_id" | "tool_use_id" | "call_id" | "invocation_id"
    ) {
        4
    } else if is_toolish_id_key(key) {
        3
    } else if key != "id" {
        2
    } else {
        1
    }
}

fn ids_are_compatible(call: &IdCandidate, result: &IdCandidate) -> bool {
    call.value == result.value
        && (call.key.eq_ignore_ascii_case(&result.key)
            || (call.key.eq_ignore_ascii_case("id") && is_toolish_id_key(&result.key))
            || (is_toolish_id_key(&call.key) && is_toolish_id_key(&result.key)))
}

fn is_id_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "id"
        || lower.ends_with("_id")
        || lower.ends_with("-id")
        || key.ends_with("Id")
        || key.ends_with("ID")
}

fn is_toolish_id_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    is_id_key(key)
        && ["tool", "call", "use", "invocation"]
            .iter()
            .any(|part| lower.contains(part))
}

fn is_tool_call_type(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "tool_call" | "tool_use" | "function_call"
    )
}

fn is_tool_result_type(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "tool_result" | "tool_response" | "function_result" | "function_response"
    )
}

fn child_context(key: &str) -> Option<ContainerContext> {
    let lower = key.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "tool_calls" | "tool_uses" | "function_calls"
    ) {
        Some(ContainerContext::ToolCallCollection)
    } else if matches!(lower.as_str(), "tool_call" | "tool_use" | "function_call") {
        Some(ContainerContext::ToolCallObject)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
