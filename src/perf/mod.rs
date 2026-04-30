mod fixtures;
mod format;
mod load;
mod runner;

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_load_suite() {
    runner::run_suite("load", load::CASES);
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_format_suite() {
    runner::run_suite("transform", format::CASES);
}
