use std::io::{BufRead, Write};

use anyhow::{Context, Result, anyhow};
use quick_xml::{Reader as XmlReader, Writer as XmlWriter, events::Event};

pub(super) fn format_xml_reader<R: BufRead, W: Write>(
    input: R,
    output: &mut W,
    indent: usize,
) -> Result<()> {
    let mut reader = XmlReader::from_reader(input);
    reader.config_mut().trim_text(false);
    let mut writer = XmlWriter::new_with_indent(&mut *output, b' ', indent);
    let mut buf = Vec::with_capacity(8192);

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(event) => writer
                .write_event(event)
                .context("failed to write XML event")?,
            Err(error) => return Err(anyhow!(error)),
        }
        buf.clear();
    }

    writeln!(output)?;
    Ok(())
}
