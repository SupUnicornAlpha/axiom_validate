mod cases;
mod coding_agent;
mod report;

use std::fs;
use std::path::PathBuf;

use cases::all_cases;
use report::{render_markdown_report, CaseResult};

fn main() {
    let results: Vec<CaseResult> = all_cases().into_iter().map(|case| case.run()).collect();
    let markdown = render_markdown_report(&results);
    let report_path = PathBuf::from("reports/latest-report.md");
    if let Some(parent) = report_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&report_path, markdown).expect("write latest report");

    for result in &results {
        println!(
            "[{}] {} - {}",
            if result.passed { "PASS" } else { "FAIL" },
            result.case_id,
            result.summary
        );
    }
    println!("report={}", report_path.display());

    if results.iter().any(|result| !result.passed) {
        std::process::exit(1);
    }
}
