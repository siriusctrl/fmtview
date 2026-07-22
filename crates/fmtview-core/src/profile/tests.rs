use std::io::Write;

use tempfile::{Builder as TempFileBuilder, NamedTempFile};

use super::{
    ContentShape, FormatKind, FormatOptions, InputSource, TransformStrategy, TypeProfile,
    sniff::SNIFF_BYTES,
};
use crate::load::LoadPlan;

fn source(contents: &[u8]) -> (NamedTempFile, InputSource) {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(contents).unwrap();
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source)
}

fn source_with_suffix(contents: &[u8], suffix: &str) -> (NamedTempFile, InputSource) {
    let mut temp = TempFileBuilder::new().suffix(suffix).tempfile().unwrap();
    temp.write_all(contents).unwrap();
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source)
}

#[test]
fn resolves_plain_extension_to_passthrough_profile() {
    let (_temp, source) = source_with_suffix(b"plain\n", ".txt");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Plain);
    assert_eq!(profile.shape, ContentShape::LineIndexed);
    assert_eq!(profile.load, LoadPlan::EagerIndexedSource);
    assert_eq!(profile.transform, TransformStrategy::Passthrough);
}

#[test]
fn resolves_jinja_extension_to_template_profile() {
    let (_temp, source) = source_with_suffix(b"<h1>{{ title }}</h1>\n", ".html.j2");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Jinja);
    assert_eq!(profile.shape, ContentShape::LineIndexed);
    assert_eq!(profile.load, LoadPlan::EagerIndexedSource);
    assert_eq!(profile.transform, TransformStrategy::Passthrough);
}

#[test]
fn resolves_toml_extension_to_passthrough_profile() {
    let (_temp, source) = source_with_suffix(b"[package]\nname = \"fmtview\"\n", ".toml");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Toml);
    assert_eq!(profile.shape, ContentShape::LineIndexed);
    assert_eq!(profile.load, LoadPlan::EagerIndexedSource);
    assert_eq!(profile.transform, TransformStrategy::Passthrough);
}

#[test]
fn resolves_markdown_extension_to_passthrough_profile() {
    let (_temp, source) = source_with_suffix(b"# fmtview\n\n- fast viewer\n", ".md");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Markdown);
    assert_eq!(profile.shape, ContentShape::LineIndexed);
    assert_eq!(profile.load, LoadPlan::EagerIndexedSource);
    assert_eq!(profile.transform, TransformStrategy::Passthrough);
}

#[test]
fn unknown_textual_content_falls_back_to_plain_profile() {
    let (_temp, source) = source_with_suffix(b"hello world\nnot json\n", ".weird");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Plain);
    assert_eq!(profile.shape, ContentShape::LineIndexed);
    assert_eq!(profile.load, LoadPlan::EagerIndexedSource);
    assert_eq!(profile.transform, TransformStrategy::Passthrough);
}

#[test]
fn resolves_record_stream_to_lazy_jsonl_profile() {
    let (_temp, source) = source_with_suffix(b"{\"a\":1}\n{\"b\":2}\n", ".data");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Jsonl);
    assert_eq!(profile.shape, ContentShape::RecordStream);
    assert_eq!(profile.load, LoadPlan::LazyTransformedRecords);
    assert_eq!(profile.transform, TransformStrategy::RecordPrettyPrint);
}

#[test]
fn explicit_format_kinds_choose_profile_without_sniffing() {
    let (_temp, source) = source(b"{\"broken\":\n");

    let cases = [
        (
            FormatKind::Jsonl,
            ContentShape::RecordStream,
            LoadPlan::LazyTransformedRecords,
            TransformStrategy::RecordPrettyPrint,
        ),
        (
            FormatKind::Json,
            ContentShape::WholeDocument,
            LoadPlan::EagerTransformedDocument,
            TransformStrategy::PrettyPrint,
        ),
        (
            FormatKind::Xml,
            ContentShape::WholeDocument,
            LoadPlan::EagerTransformedDocument,
            TransformStrategy::PrettyPrint,
        ),
        (
            FormatKind::Toml,
            ContentShape::LineIndexed,
            LoadPlan::EagerIndexedSource,
            TransformStrategy::Passthrough,
        ),
        (
            FormatKind::Markdown,
            ContentShape::LineIndexed,
            LoadPlan::EagerIndexedSource,
            TransformStrategy::Passthrough,
        ),
        (
            FormatKind::Plain,
            ContentShape::LineIndexed,
            LoadPlan::EagerIndexedSource,
            TransformStrategy::Passthrough,
        ),
        (
            FormatKind::Jinja,
            ContentShape::LineIndexed,
            LoadPlan::EagerIndexedSource,
            TransformStrategy::Passthrough,
        ),
    ];

    for (kind, shape, load, transform) in cases {
        let profile = TypeProfile::resolve(&source, &FormatOptions { kind, indent: 2 }).unwrap();

        assert_eq!(profile.content, kind);
        assert_eq!(profile.shape, shape);
        assert_eq!(profile.load, load);
        assert_eq!(profile.transform, transform);
    }
}

#[test]
fn resolves_jsonl_extension_before_sampling() {
    let mut data = b"{\"message\":\"".to_vec();
    data.extend(std::iter::repeat_n(b'a', SNIFF_BYTES + 1024));
    data.extend_from_slice(b"\"}\n{\"ok\":true}\n");
    let (_temp, source) = source_with_suffix(&data, ".jsonl");

    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Jsonl);
    assert_eq!(profile.shape, ContentShape::RecordStream);
    assert_eq!(profile.load, LoadPlan::LazyTransformedRecords);
}

#[test]
fn keeps_truncated_record_prefix_as_eager_json_document_without_extension() {
    let mut data = b"{\"message\":\"".to_vec();
    data.extend(std::iter::repeat_n(b'a', SNIFF_BYTES + 1024));
    data.extend_from_slice(b"\"}\n{\"ok\":true}\n");
    let (_temp, source) = source(&data);

    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Json);
    assert_eq!(profile.shape, ContentShape::WholeDocument);
    assert_eq!(profile.load, LoadPlan::EagerTransformedDocument);
}

#[test]
fn keeps_multiline_documents_eager() {
    let (_json_temp, json_source) = source(b"{\n  \"items\": [\n    {\"a\": 1}\n  ]\n}\n");
    let (_xml_temp, xml_source) = source(b"<root>\n  <item>one</item>\n</root>\n");
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let json = TypeProfile::resolve(&json_source, &options).unwrap();
    let xml = TypeProfile::resolve(&xml_source, &options).unwrap();

    assert_eq!(json.content, FormatKind::Json);
    assert_eq!(json.shape, ContentShape::WholeDocument);
    assert_eq!(json.load, LoadPlan::EagerTransformedDocument);
    assert_eq!(xml.content, FormatKind::Xml);
    assert_eq!(xml.shape, ContentShape::WholeDocument);
    assert_eq!(xml.load, LoadPlan::EagerTransformedDocument);
}

#[test]
fn keeps_single_line_json_document_eager() {
    let (_temp, source) = source(b"{\"items\":[{\"a\":1},{\"b\":2}]}\n");

    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Json);
    assert_eq!(profile.shape, ContentShape::WholeDocument);
    assert_eq!(profile.load, LoadPlan::EagerTransformedDocument);
}

#[test]
fn resolves_html_extension_to_html_profile() {
    let (_temp, source) =
        source_with_suffix(b"<!doctype html><html><body><br></body></html>\n", ".html");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();

    assert_eq!(profile.content, FormatKind::Html);
    assert_eq!(profile.shape, ContentShape::WholeDocument);
    assert_eq!(profile.load, LoadPlan::EagerTransformedDocument);
    assert_eq!(profile.transform, TransformStrategy::PrettyPrint);
}

#[test]
fn sniffs_doctype_html_as_html_without_extension() {
    let (_temp, source) = source(b"<!doctype html>\n<html><body><br></body></html>\n");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();
    assert_eq!(profile.content, FormatKind::Html);
}

#[test]
fn sniffs_xml_declaration_as_xml_without_extension() {
    let (_temp, source) = source(b"<?xml version=\"1.0\"?><root><item/></root>\n");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        },
    )
    .unwrap();
    assert_eq!(profile.content, FormatKind::Xml);
}

#[test]
fn explicit_html_type_chooses_html_profile() {
    let (_temp, source) = source(b"<div><br></div>\n");
    let profile = TypeProfile::resolve(
        &source,
        &FormatOptions {
            kind: FormatKind::Html,
            indent: 2,
        },
    )
    .unwrap();
    assert_eq!(profile.content, FormatKind::Html);
    assert_eq!(profile.load, LoadPlan::EagerTransformedDocument);
}
