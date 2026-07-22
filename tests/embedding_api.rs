use std::{fs, process::Command};

#[test]
fn downstream_consumer_needs_only_the_fmtview_dependency() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let fmtview_path = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    fs::write(
        project.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "fmtview-downstream-check"
version = "0.0.0"
edition = "2024"

[workspace]

[dependencies]
fmtview = {{ path = "{fmtview_path}" }}
"#,
        ),
    )
    .unwrap();
    fs::write(
        project.path().join("src/main.rs"),
        r#"use fmtview::view::{
    self, RecordLoadLimit, RecordTimeline, Result, TimelineRead, TimelineRefresh,
    TimelineSnapshot, ViewOptions,
};

struct Source;

impl RecordTimeline for Source {
    fn label(&self) -> &str { "downstream" }
    fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot { epoch: 1, committed_end: 0, observed_end: 0, pending_bytes: 0 }
    }
    fn probe_prefix(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
        Ok(TimelineRead::End)
    }
    fn load_older(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
        Ok(TimelineRead::End)
    }
    fn load_newer(&mut self, _: RecordLoadLimit) -> Result<TimelineRead> {
        Ok(TimelineRead::End)
    }
    fn refresh(&mut self) -> Result<TimelineRefresh> {
        Ok(TimelineRefresh::End(self.snapshot()))
    }
}

fn main() {
    let source: Box<dyn RecordTimeline> = Box::new(Source);
    let options = ViewOptions::default();
    let _call: fn(Box<dyn RecordTimeline>, ViewOptions) -> Result<()> = view::run;
    drop((source, options));
}
"#,
    )
    .unwrap();
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"),
        project.path().join("Cargo.lock"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO"))
        .args(["check", "--offline", "--quiet"])
        .current_dir(project.path())
        .env("CARGO_TARGET_DIR", project.path().join("target"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "downstream cargo check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
