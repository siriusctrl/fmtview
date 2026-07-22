mod sniff;

#[cfg(test)]
mod tests;

use std::ffi::OsStr;

use anyhow::Result;

use crate::{
    formats::{self, ContentShape, FORMAT_SPECS, FormatSpec},
    input::InputSource,
    transform::{FormatKind, FormatOptions, TransformStrategy},
};

use sniff::TypeSample;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeProfile {
    pub(crate) content: FormatKind,
    pub(crate) shape: ContentShape,
    pub(crate) load: crate::load::LoadPlan,
    pub(crate) transform: TransformStrategy,
}

impl FormatSpec {
    fn profile(self) -> TypeProfile {
        TypeProfile {
            content: self.kind,
            shape: self.shape,
            load: self.load,
            transform: self.transform,
        }
    }
}

impl TypeProfile {
    pub fn resolve(source: &InputSource, options: &FormatOptions) -> Result<Self> {
        if options.kind != FormatKind::Auto {
            return Ok(explicit_profile(options.kind));
        }

        if let Some(kind) = extension_kind(source) {
            return Ok(explicit_profile(kind));
        }

        let sample = TypeSample::read(source)?;
        if sample.looks_like_record_stream() {
            return Ok(explicit_profile(FormatKind::Jsonl));
        }

        Ok(match sample.first_non_ws {
            Some(b'<') => explicit_profile(formats::detect_markup_kind(&sample.markup_prefix)),
            Some(b'{' | b'[') => explicit_profile(FormatKind::Json),
            _ => explicit_profile(FormatKind::Plain),
        })
    }

    /// Resolved content kind used by highlighting and rendering.
    pub const fn content_kind(self) -> FormatKind {
        self.content
    }

    /// Coarse input shape used to select shared runtime behavior.
    pub const fn content_shape(self) -> ContentShape {
        self.shape
    }

    /// Loading strategy selected for interactive viewing.
    pub const fn load_plan(self) -> crate::load::LoadPlan {
        self.load
    }

    pub fn format_options(self, indent: usize) -> FormatOptions {
        FormatOptions {
            kind: self.content,
            indent,
        }
    }
}

fn explicit_profile(kind: FormatKind) -> TypeProfile {
    FORMAT_SPECS
        .iter()
        .copied()
        .find(|spec| spec.kind == kind)
        .map(FormatSpec::profile)
        .unwrap_or_else(|| unreachable!("auto must be resolved before building a type profile"))
}

fn extension_kind(source: &InputSource) -> Option<FormatKind> {
    let extension = source
        .path()
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)?;
    formats::kind_for_extension(&extension)
}
