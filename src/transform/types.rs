use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatKind {
    Auto,
    Json,
    Jsonl,
    Xml,
}

#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    pub kind: FormatKind,
    pub indent: usize,
}
