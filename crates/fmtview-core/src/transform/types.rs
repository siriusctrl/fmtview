#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Auto,
    Csv,
    Tsv,
    Xsv,
    Json,
    Jsonl,
    Xml,
    Html,
    Toml,
    Markdown,
    Plain,
    Jinja,
}

impl FormatKind {
    pub const fn is_delimited(self) -> bool {
        matches!(self, Self::Csv | Self::Tsv | Self::Xsv)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    pub kind: FormatKind,
    pub indent: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformStrategy {
    PrettyPrint,
    RecordPrettyPrint,
    Passthrough,
}
