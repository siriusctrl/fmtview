use anyhow::Result;

use crate::{
    input::InputSource,
    profile::TypeProfile,
    transform::{self, FormatKind, FormatOptions, TransformStrategy},
};

use super::{IndexedTempFile, LazyTransformedRecordsFile, LoadPlan, ViewFile};

pub struct OpenedViewFile {
    pub file: Box<dyn ViewFile>,
    pub content: FormatKind,
    pub notice: Option<String>,
}

pub fn open_view_file(
    input: &InputSource,
    options: &FormatOptions,
    profile: TypeProfile,
) -> Result<OpenedViewFile> {
    open_view_file_with_fallback(input, options, profile, false)
}

pub fn open_view_file_with_fallback(
    input: &InputSource,
    options: &FormatOptions,
    profile: TypeProfile,
    allow_plain_fallback: bool,
) -> Result<OpenedViewFile> {
    match profile.load {
        LoadPlan::LazyTransformedRecords => Ok(OpenedViewFile {
            file: Box::new(LazyTransformedRecordsFile::new(input, *options)?),
            content: profile.content,
            notice: None,
        }),
        LoadPlan::EagerTransformedDocument | LoadPlan::EagerIndexedSource => {
            match open_indexed(input, options, profile.transform) {
                Ok(file) => Ok(OpenedViewFile {
                    file,
                    content: profile.content,
                    notice: None,
                }),
                Err(_)
                    if allow_plain_fallback
                        && profile.transform != TransformStrategy::Passthrough =>
                {
                    let fallback_options = FormatOptions {
                        kind: FormatKind::Plain,
                        indent: options.indent,
                    };
                    Ok(OpenedViewFile {
                        file: open_indexed(
                            input,
                            &fallback_options,
                            TransformStrategy::Passthrough,
                        )?,
                        content: FormatKind::Plain,
                        notice: Some(fallback_notice(profile.content)),
                    })
                }
                Err(error) => Err(error),
            }
        }
    }
}

fn open_indexed(
    input: &InputSource,
    options: &FormatOptions,
    transform: TransformStrategy,
) -> Result<Box<dyn ViewFile>> {
    let formatted = transform::transform_source_to_temp(input, options, transform)?;
    Ok(Box::new(IndexedTempFile::new(
        input.label().to_owned(),
        formatted,
    )?))
}

fn fallback_notice(kind: FormatKind) -> String {
    format!(
        "auto-detected {} could not be formatted; showing plain text. Use --type to choose a type",
        kind.label()
    )
}

trait FormatKindLabel {
    fn label(self) -> &'static str;
}

impl FormatKindLabel for FormatKind {
    fn label(self) -> &'static str {
        match self {
            FormatKind::Auto => "input",
            FormatKind::Json => "JSON",
            FormatKind::Jsonl => "JSONL",
            FormatKind::Xml => "XML",
            FormatKind::Html => "HTML",
            FormatKind::Toml => "TOML",
            FormatKind::Markdown => "Markdown",
            FormatKind::Plain => "plain text",
            FormatKind::Jinja => "Jinja",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::Builder as TempFileBuilder;

    use super::*;
    use crate::{input::InputSource, profile::TypeProfile};

    fn invalid_json_source() -> (tempfile::NamedTempFile, InputSource) {
        let mut temp = TempFileBuilder::new().suffix(".json").tempfile().unwrap();
        write!(temp, "not json\nstill useful text\n").unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        (temp, source)
    }

    #[test]
    fn auto_inferred_format_error_can_fallback_to_plain_view_file() {
        let (_temp, source) = invalid_json_source();
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let profile = TypeProfile::resolve(&source, &options).unwrap();
        let resolved_options = profile.format_options(options.indent);

        let opened =
            open_view_file_with_fallback(&source, &resolved_options, profile, true).unwrap();

        assert_eq!(opened.content, FormatKind::Plain);
        assert!(
            opened
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("--type"))
        );
        assert_eq!(
            opened.file.read_window(0, 3).unwrap(),
            vec!["not json", "still useful text"]
        );
    }

    #[test]
    fn explicit_or_non_fallback_format_error_still_fails() {
        let (_temp, source) = invalid_json_source();
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let profile = TypeProfile::resolve(&source, &options).unwrap();
        let resolved_options = profile.format_options(options.indent);

        let error = match open_view_file_with_fallback(&source, &resolved_options, profile, false) {
            Ok(_) => panic!("invalid inferred JSON should fail when fallback is disabled"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("failed to format"));
    }
}
