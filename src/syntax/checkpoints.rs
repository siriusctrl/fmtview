use super::xml::XmlPairState;

const HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES: usize = 32 * 1024;

#[derive(Debug, Default)]
pub(crate) struct HighlightCheckpointIndex {
    pub(crate) json_value_strings: Vec<XmlHighlightCheckpoint>,
    pub(crate) xml_lines: Vec<XmlHighlightCheckpoint>,
}

#[derive(Debug, Clone)]
pub(crate) struct XmlHighlightCheckpoint {
    pub(crate) byte: usize,
    pub(crate) state: XmlPairState,
}

impl HighlightCheckpointIndex {
    pub(crate) fn json_value_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.json_value_strings, byte)
    }

    pub(crate) fn xml_line_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.xml_lines, byte)
    }

    pub(crate) fn remember_json_value(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.json_value_strings, byte, state);
    }

    pub(crate) fn remember_xml_line(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.xml_lines, byte, state);
    }
}

pub(crate) fn checkpoint_before(
    checkpoints: &[XmlHighlightCheckpoint],
    byte: usize,
) -> Option<XmlHighlightCheckpoint> {
    checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.byte <= byte)
        .cloned()
}

pub(crate) fn remember_xml_checkpoint(
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
