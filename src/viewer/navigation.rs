pub(in crate::viewer) mod structure;

pub(super) use structure::{
    StructureDirection, StructureViewport, process_structure_step, start_structure_navigation,
};

#[cfg(test)]
pub(super) use structure::is_structure_point;
