use std::{env, time::Duration};

const DEFAULT_SAMPLES: usize = 7;

#[derive(Clone, Copy)]
pub(super) struct BenchCase {
    pub(super) label: &'static str,
    pub(super) shape: &'static str,
    pub(super) layer: &'static str,
    pub(super) run: fn() -> BenchSample,
}

pub(super) struct BenchSample {
    pub(super) elapsed: Duration,
    pub(super) records: usize,
    pub(super) items: usize,
    pub(super) string_bytes: usize,
    pub(super) lines: usize,
    pub(super) indexed_lines: usize,
    pub(super) window_lines: usize,
    pub(super) input_bytes: usize,
    pub(super) output_bytes: usize,
}

impl BenchSample {
    fn elapsed_ms(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1000.0
    }
}

pub(super) fn run_suite(name: &str, cases: &[BenchCase]) {
    let samples = sample_count();
    let case_filter = env::var("FMTVIEW_PERF_CASE").ok();

    println!("fmtview {name} performance smoke");
    println!("samples: {samples}");

    for case in cases.iter().filter(|case| {
        case_filter
            .as_deref()
            .is_none_or(|filter| case.matches(filter))
    }) {
        run_case(*case, samples);
    }
}

impl BenchCase {
    fn matches(self, filter: &str) -> bool {
        self.label.contains(filter) || self.shape.contains(filter) || self.layer.contains(filter)
    }
}

fn sample_count() -> usize {
    env::var("FMTVIEW_PERF_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}

fn run_case(case: BenchCase, samples: usize) {
    println!();
    println!("== {} ==", case.label);
    println!("shape={} layer={}", case.shape, case.layer);

    let mut timings = Vec::with_capacity(samples);
    for sample_index in 1..=samples {
        let sample = (case.run)();
        let ms = sample.elapsed_ms();
        timings.push(ms);
        println!(
            "sample {sample_index:02}: {ms:8.3}ms  records={}  items={}  string_bytes={}  lines={}  indexed_lines={}  window_lines={}  input_bytes={}  output_bytes={}",
            sample.records,
            sample.items,
            sample.string_bytes,
            sample.lines,
            sample.indexed_lines,
            sample.window_lines,
            sample.input_bytes,
            sample.output_bytes,
        );
    }

    let summary = TimingSummary::from_samples(&mut timings);
    println!(
        "time: median={:.3}ms min={:.3}ms max={:.3}ms avg={:.3}ms",
        summary.median, summary.min, summary.max, summary.avg
    );
}

struct TimingSummary {
    median: f64,
    min: f64,
    max: f64,
    avg: f64,
}

impl TimingSummary {
    fn from_samples(values: &mut [f64]) -> Self {
        values.sort_by(f64::total_cmp);
        let mid = values.len() / 2;
        let median = if values.len() % 2 == 0 {
            (values[mid - 1] + values[mid]) / 2.0
        } else {
            values[mid]
        };
        let min = values[0];
        let max = values[values.len() - 1];
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        Self {
            median,
            min,
            max,
            avg,
        }
    }
}
