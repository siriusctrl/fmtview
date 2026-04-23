use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

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
        .write_stdin("{\"a\":1}\n{\"b\":2}\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"a\": 1"))
        .stdout(predicate::str::contains("\"b\": 2"));
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
