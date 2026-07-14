mod cases;
mod coding_agent;
mod report;

use std::fs;
use std::path::PathBuf;
use std::process;

use cases::all_cases;
use coding_agent::run_go_agent_live;
use report::{render_markdown_report, CaseResult};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("try-coding") {
        if let Err(error) = try_coding(&args[1..]) {
            eprintln!("try-coding failed: {error}");
            process::exit(1);
        }
        return;
    }

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
        process::exit(1);
    }
}

fn try_coding(args: &[String]) -> Result<(), String> {
    if std::env::var_os("DEEPSEEK_API_KEY").is_none() {
        return Err(
            "set DEEPSEEK_API_KEY first (optional: DEEPSEEK_BASE_URL, DEEPSEEK_MODEL)".into(),
        );
    }

    let mut workspace: Option<PathBuf> = None;
    let mut task: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--workspace" => {
                i += 1;
                workspace = Some(PathBuf::from(
                    args.get(i).ok_or("--workspace needs a path")?,
                ));
            }
            "--task" => {
                i += 1;
                task = Some(args.get(i).ok_or("--task needs a string")?.clone());
            }
            "-h" | "--help" => {
                println!(
                    "Usage: cargo run -- try-coding [--workspace PATH] [--task TEXT]\n\n\
Requires DEEPSEEK_API_KEY. Optional DEEPSEEK_BASE_URL / DEEPSEEK_MODEL.\n\
Default workspace is a buggy calculator fixture under reports/generated/."
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    let workspace = match workspace {
        Some(path) => path,
        None => prepare_calculator_fixture()?,
    };
    let task = task.unwrap_or_else(|| {
        "Fix the failing calculator test (add should return left + right) and verify with cargo test --offline."
            .to_string()
    });

    println!("workspace={}", workspace.display());
    println!("task={task}");
    println!("calling DeepSeek via Go decide through Axiom ReActScheduler...");

    let result = run_go_agent_live(&workspace, &task)?;
    let source = fs::read_to_string(workspace.join("src/lib.rs")).unwrap_or_default();
    let last_message = result
        .report
        .state
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "(no assistant message)".into());

    println!("\n=== Run complete (via Axiom Kernel) ===");
    println!("run_status={:?}", result.report.state.status);
    println!("steps={}", result.report.state.next_step_index);
    println!("outputs={}", result.report.state.outputs.len());
    println!("messages={}", result.report.state.messages.len());
    println!("events={}", result.report.events.len());
    println!("\n--- event trail (framework audit) ---");
    for event in &result.report.events {
        println!(
            "  [{:?}] seq={} detail={}",
            event.kind, event.sequence, event.detail
        );
    }
    println!("\n--- tool outputs (chronological) ---");
    for (i, out) in result.report.state.outputs.iter().enumerate() {
        let preview = if out.len() > 500 {
            format!("{}...", &out[..500])
        } else {
            out.clone()
        };
        println!("  [{i}] {preview}");
    }
    println!("\n--- src/lib.rs ---\n{source}");
    println!("--- last assistant message ---\n{last_message}");
    if !result.report.state.denied_actions.is_empty() {
        println!("denied={:?}", result.report.state.denied_actions);
    }
    Ok(())
}

fn prepare_calculator_fixture() -> Result<PathBuf, String> {
    let workspace = PathBuf::from("reports/generated/coding-agent-deepseek-try");
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(workspace.join("src")).map_err(|e| e.to_string())?;
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='calculator-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn add(left: i32, right: i32) -> i32 { left - right }\n\n\
#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn adds() {\n        assert_eq!(add(2, 3), 5);\n    }\n}\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(workspace)
}
