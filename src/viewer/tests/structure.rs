use super::*;

mod detection;
mod formats;
mod interaction;
mod json_ranking;
mod json_records;
mod json_visibility;
mod target;

fn indexed_file(lines: &[&str]) -> IndexedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(temp, "{line}").unwrap();
    }
    temp.flush().unwrap();
    IndexedTempFile::new("test".to_owned(), temp).unwrap()
}

fn structure_viewport(top: usize, bottom: usize) -> StructureViewport {
    StructureViewport {
        top,
        top_row_offset: 0,
        bottom,
        bottom_line_end: true,
        x: 0,
        width: 80,
        wrap: true,
    }
}
