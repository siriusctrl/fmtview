use std::{ffi::OsStr, io::Read, path::Path};

use anyhow::{Context, Result};
use csv::{ByteRecord, ReaderBuilder};

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const SNIFF_RECORDS: usize = 24;
const SNIFF_BYTES: u64 = 256 * 1024;
const DELIMITER_CANDIDATES: &[u8] = b",\t|;";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimitedDialect {
    delimiter: u8,
}

impl DelimitedDialect {
    pub const fn delimiter(self) -> u8 {
        self.delimiter
    }

    pub fn display(self) -> &'static str {
        match self.delimiter {
            b',' => ",",
            b'\t' => "TAB",
            b'|' => "|",
            b';' => ";",
            _ => "custom",
        }
    }
}

pub(crate) fn resolve_dialect(path: &Path, kind: FormatKind) -> Result<DelimitedDialect> {
    let delimiter = match kind {
        FormatKind::Csv => b',',
        FormatKind::Tsv => b'\t',
        FormatKind::Xsv => match path
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("psv") => b'|',
            Some("ssv") => b';',
            _ => sniff_delimiter(path)?,
        },
        _ => anyhow::bail!("{kind:?} is not a delimited format"),
    };
    Ok(DelimitedDialect { delimiter })
}

fn sniff_delimiter(path: &Path) -> Result<u8> {
    let mut best = None;
    for &delimiter in DELIMITER_CANDIDATES {
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        let mut reader = ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(file.take(SNIFF_BYTES));
        let mut record = ByteRecord::new();
        let mut counts = Vec::new();
        while counts.len() < SNIFF_RECORDS && reader.read_byte_record(&mut record)? {
            if !record.is_empty() {
                counts.push(record.len());
            }
        }
        let Some((&columns, consistent)) = most_common_count(&counts) else {
            continue;
        };
        if columns <= 1 {
            continue;
        }
        let score = (consistent, columns);
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((delimiter, score));
        }
    }
    Ok(best.map(|(delimiter, _)| delimiter).unwrap_or(b','))
}

fn most_common_count(counts: &[usize]) -> Option<(&usize, usize)> {
    counts
        .iter()
        .map(|candidate| {
            let occurrences = counts.iter().filter(|count| *count == candidate).count();
            (candidate, occurrences)
        })
        .max_by_key(|(columns, occurrences)| (*occurrences, **columns))
}

pub(crate) const CSV_SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Csv,
    extensions: &["csv"],
    shape: ContentShape::DelimitedRecords,
    load: LoadPlan::LazyDelimitedRecords,
    transform: TransformStrategy::Passthrough,
};

pub(crate) const TSV_SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Tsv,
    extensions: &["tsv", "tab"],
    shape: ContentShape::DelimitedRecords,
    load: LoadPlan::LazyDelimitedRecords,
    transform: TransformStrategy::Passthrough,
};

pub(crate) const XSV_SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Xsv,
    extensions: &["xsv", "psv", "ssv"],
    shape: ContentShape::DelimitedRecords,
    load: LoadPlan::LazyDelimitedRecords,
    transform: TransformStrategy::Passthrough,
};

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::Builder;

    use super::*;

    #[test]
    fn xsv_sniff_does_not_read_past_its_byte_budget() {
        let mut temp = Builder::new().suffix(".xsv").tempfile().unwrap();
        temp.write_all(&vec![b'x'; SNIFF_BYTES as usize + 32])
            .unwrap();
        temp.write_all(b"|outside|budget\n").unwrap();
        temp.flush().unwrap();

        let dialect = resolve_dialect(temp.path(), FormatKind::Xsv).unwrap();

        assert_eq!(dialect.delimiter(), b',');
    }
}
