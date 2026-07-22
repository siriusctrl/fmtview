use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLayout {
    Unified,
    SideBySide,
}

impl DiffLayout {
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Unified => Self::SideBySide,
            Self::SideBySide => Self::Unified,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unified => "single",
            Self::SideBySide => "side-by-side",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NumberedDiffLine {
    pub(crate) number: usize,
    pub(crate) content: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffChange {
    pub(crate) intensity: DiffIntensity,
    pub(crate) left_range: Option<DiffRange>,
    pub(crate) right_range: Option<DiffRange>,
}

impl Default for DiffChange {
    fn default() -> Self {
        Self::new(DiffIntensity::Low, None, None)
    }
}

impl DiffChange {
    pub(crate) fn new(
        intensity: DiffIntensity,
        left_range: Option<DiffRange>,
        right_range: Option<DiffRange>,
    ) -> Self {
        Self {
            intensity,
            left_range,
            right_range,
        }
    }

    pub(crate) fn full_line(end: usize) -> Self {
        Self::new(
            DiffIntensity::High,
            Some(DiffRange::full(end)),
            Some(DiffRange::full(end)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DiffIntensity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl DiffRange {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub(crate) fn full(end: usize) -> Self {
        Self::new(0, end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifiedDiffRow {
    Context {
        left: usize,
        right: usize,
        content: Arc<str>,
    },
    Delete {
        left: usize,
        content: Arc<str>,
        change: DiffChange,
    },
    Insert {
        right: usize,
        content: Arc<str>,
        change: DiffChange,
    },
    Message {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SideDiffRow {
    Context {
        unified: usize,
    },
    Change {
        left: Option<usize>,
        right: Option<usize>,
    },
    Message {
        unified: usize,
    },
}
