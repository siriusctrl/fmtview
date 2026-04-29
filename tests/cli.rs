use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::{Builder as TempFileBuilder, NamedTempFile};

#[test]
fn formats_json_from_stdin() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "json"])
        .write_stdin(r#"{"a":{"b":1}}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"b\": 1"));
}

#[test]
fn formats_jsonl_from_stdin() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "jsonl"])
        .write_stdin("{\"a\":{\"nested\":1}}\n{\"b\":[2,3]}\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("  \"nested\": 1"))
        .stdout(predicate::str::contains("  \"b\": ["));
}

#[test]
fn pretty_prints_each_jsonl_record() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    let assert = cmd
        .args(["--type", "jsonl"])
        .write_stdin("{\"a\":{\"b\":1}}\n[1,{\"c\":2}]\n")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert_eq!(
        stdout,
        "{\n  \"a\": {\n    \"b\": 1\n  }\n}\n[\n  1,\n  {\n    \"c\": 2\n  }\n]\n"
    );
}

#[test]
fn auto_detects_jsonl_file_and_pretty_prints_record() {
    let mut input = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    write!(
        input,
        r#"{{"event":{{"payload":{{"items":[{{"id":1,"ok":true}}]}}}}}}"#
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(input.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("  \"event\": {"))
        .stdout(predicate::str::contains("      \"items\": ["))
        .stdout(predicate::str::contains("          \"ok\": true"));
}

#[test]
fn formats_jsonl_showcase_deep_record() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/events.jsonl");

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(example)
        .assert()
        .success()
        .stdout(predicate::str::contains("  \"event\": \"deep_record\""))
        .stdout(predicate::str::contains("\n    \"request\": {\n"))
        .stdout(predicate::str::contains("\n      \"route\": [\n"));
}

#[test]
fn plain_type_passthrough_keeps_stdout_scriptable() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "plain", "--literal", "alpha {{ untouched }}\n"])
        .assert()
        .success()
        .stdout(predicate::eq("alpha {{ untouched }}\n"));
}

#[test]
fn auto_detects_txt_as_plain_passthrough() {
    let mut input = TempFileBuilder::new().suffix(".txt").tempfile().unwrap();
    input.write_all(b"not json\n{{ still plain }}\n").unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(input.path())
        .assert()
        .success()
        .stdout(predicate::eq("not json\n{{ still plain }}\n"));
}

#[test]
fn auto_detects_jinja_template_as_passthrough() {
    let mut input = TempFileBuilder::new()
        .suffix(".html.j2")
        .tempfile()
        .unwrap();
    input
        .write_all(b"<h1>{{ title }}</h1>\n{% if ok %}<p>yes</p>{% endif %}\n")
        .unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(input.path())
        .assert()
        .success()
        .stdout(predicate::eq(
            "<h1>{{ title }}</h1>\n{% if ok %}<p>yes</p>{% endif %}\n",
        ));
}

#[test]
fn formats_jinja_showcase_as_passthrough() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/template.html.j2");
    let expected = std::fs::read_to_string(&example).unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    let assert = cmd.arg(example).assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert_eq!(stdout, expected);
}

#[test]
fn diffs_plain_inputs_without_formatting() {
    let mut left = NamedTempFile::new().unwrap();
    let mut right = NamedTempFile::new().unwrap();
    write!(left, "same\nleft\n").unwrap();
    write!(right, "same\nright\n").unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args([
        "diff",
        "--type",
        "plain",
        left.path().to_str().unwrap(),
        right.path().to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("-left"))
    .stdout(predicate::str::contains("+right"));
}

#[test]
fn preserves_large_json_numbers() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "json"])
        .write_stdin(r#"{"n":123456789012345678901234567890}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""n": 123456789012345678901234567890"#,
        ))
        .stdout(predicate::str::contains("1.2345678901234568e+29").not());
}

#[test]
fn preserves_large_jsonl_numbers() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "jsonl"])
        .write_stdin("{\"n\":123456789012345678901234567890}\n")
        .assert()
        .success()
        .stdout(predicate::eq(
            "{\n  \"n\": 123456789012345678901234567890\n}\n",
        ));
}

#[test]
fn formats_xml_from_stdin() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "xml"])
        .write_stdin("<root><child>1</child></root>")
        .assert()
        .success()
        .stdout(predicate::str::contains("<child>1</child>"));
}

#[test]
fn auto_detects_well_formed_html_as_markup() {
    let mut input = TempFileBuilder::new().suffix(".html").tempfile().unwrap();
    write!(
        input,
        r#"<!doctype html><html><body><main><h1>Hello</h1><p>World</p></main></body></html>"#
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(input.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("<!DOCTYPE html>"))
        .stdout(predicate::str::contains("    <main>"))
        .stdout(predicate::str::contains("      <h1>Hello</h1>"));
}

#[test]
fn formats_html_showcase() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/page.html");

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.arg(example)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<title>fmtview markup sample</title>",
        ))
        .stdout(predicate::str::contains("<section data-kind=\"nested\">"))
        .stdout(predicate::str::contains(
            "<span>XML-compatible markup</span>",
        ));
}

#[test]
fn preserves_embedded_xml_string() {
    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args(["--type", "json"])
        .write_stdin(r#"{"xml":"<root><child>1</child></root>"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""xml": "<root><child>1</child></root>""#,
        ));
}

#[test]
fn diffs_formatted_json() {
    let mut left = NamedTempFile::new().unwrap();
    let mut right = NamedTempFile::new().unwrap();
    write!(left, r#"{{"a":1}}"#).unwrap();
    write!(right, r#"{{"a":2}}"#).unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args([
        "diff",
        "--type",
        "json",
        left.path().to_str().unwrap(),
        right.path().to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("-  \"a\": 1"))
    .stdout(predicate::str::contains("+  \"a\": 2"));
}

#[test]
fn equal_diff_stdout_is_empty() {
    let mut left = NamedTempFile::new().unwrap();
    let mut right = NamedTempFile::new().unwrap();
    write!(left, r#"{{"a":1}}"#).unwrap();
    write!(right, r#"{{"a":1}}"#).unwrap();

    let mut cmd = Command::cargo_bin("fmtview").unwrap();
    cmd.args([
        "diff",
        "--type",
        "json",
        left.path().to_str().unwrap(),
        right.path().to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::eq(""));
}
