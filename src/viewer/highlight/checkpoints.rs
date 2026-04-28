use super::super::HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES;
use super::xml::XmlPairState;

#[derive(Debug, Default)]
pub(in crate::viewer) struct HighlightCheckpointIndex {
    pub(in crate::viewer) json_value_strings: Vec<XmlHighlightCheckpoint>,
    pub(in crate::viewer) xml_lines: Vec<XmlHighlightCheckpoint>,
}

#[derive(Debug, Clone)]
pub(in crate::viewer) struct XmlHighlightCheckpoint {
    pub(in crate::viewer) byte: usize,
    pub(in crate::viewer) state: XmlPairState,
}

impl HighlightCheckpointIndex {
    pub(in crate::viewer) fn json_value_before(
        &self,
        byte: usize,
    ) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.json_value_strings, byte)
    }

    pub(in crate::viewer) fn xml_line_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.xml_lines, byte)
    }

    pub(in crate::viewer) fn remember_json_value(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.json_value_strings, byte, state);
    }

    pub(in crate::viewer) fn remember_xml_line(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.xml_lines, byte, state);
    }
}

pub(in crate::viewer) fn checkpoint_before(
    checkpoints: &[XmlHighlightCheckpoint],
    byte: usize,
) -> Option<XmlHighlightCheckpoint> {
    checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.byte <= byte)
        .cloned()
}

pub(in crate::viewer) fn remember_xml_checkpoint(
    checkpoints: &mut Vec<XmlHighlightCheckpoint>,
    byte: usize,
    state: &XmlPairState,
) {
    let next_byte = checkpoints
        .last()
        .map(|checkpoint| {
            checkpoint
                .byte
                .saturating_add(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES)
        })
        .unwrap_or(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES);
    if byte < next_byte {
        return;
    }

    match checkpoints.binary_search_by_key(&byte, |checkpoint| checkpoint.byte) {
        Ok(_) => {}
        Err(position) => checkpoints.insert(
            position,
            XmlHighlightCheckpoint {
                byte,
                state: state.clone(),
            },
        ),
    }
}
