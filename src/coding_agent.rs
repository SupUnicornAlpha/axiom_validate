use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use axiom_core::{
    AuditShell, CapabilityRegistry, Kernel, LocalTransport, MockModelDriver, ModelDecision,
    ReActScheduler, StaticCapability,
};
use axiom_spec::{CapabilityLease, EffectProposal, RunSpec};

pub struct CodingAgentResult {
    pub report: axiom_core::RunReport,
    pub workspace: PathBuf,
}

pub fn run_scripted(
    workspace: impl Into<PathBuf>,
    task: &str,
    decisions: impl IntoIterator<Item = ModelDecision>,
) -> Result<CodingAgentResult, axiom_core::KernelError> {
    let workspace = workspace.into();
    let registry = workspace_registry(&workspace);
    let scheduler = ReActScheduler::new(MockModelDriver::scripted(decisions));
    let kernel = Kernel::new(scheduler, AuditShell, LocalTransport::new(registry), None);
    let mut spec = RunSpec::new("coding-agent-opencode-parity", task, Vec::new());
    spec.namespace.workspace_root = workspace.to_string_lossy().to_string();
    for capability_id in [
        "coding/list",
        "coding/read",
        "coding/grep",
        "coding/edit",
        "coding/write",
        "coding/bash",
    ] {
        spec.namespace
            .visible_capabilities
            .push(capability_id.to_string());
        spec.capability_leases.push(CapabilityLease {
            capability_id: capability_id.to_string(),
            permissions: vec!["invoke".to_string()],
        });
    }
    let report = kernel.run(&spec)?;
    Ok(CodingAgentResult { report, workspace })
}

fn workspace_registry(root: &Path) -> CapabilityRegistry {
    let root = root.canonicalize().expect("coding workspace must exist");
    let mut registry = CapabilityRegistry::new();
    let list_root = root.to_path_buf();
    registry.register(
        "coding/list",
        StaticCapability::new(move |input, _| {
            let path = resolve(&list_root, input)?;
            let mut entries = fs::read_dir(path)
                .map_err(|error| error.to_string())?
                .map(|entry| {
                    entry
                        .map(|value| value.file_name().to_string_lossy().to_string())
                        .map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort();
            output("list", entries.join("\n"))
        }),
    );
    let read_root = root.to_path_buf();
    registry.register(
        "coding/read",
        StaticCapability::new(move |input, _| {
            output(
                "read",
                fs::read_to_string(resolve(&read_root, input)?)
                    .map_err(|error| error.to_string())?,
            )
        }),
    );
    let grep_root = root.to_path_buf();
    registry.register(
        "coding/grep",
        StaticCapability::new(move |input, _| {
            let request: serde_json::Value =
                serde_json::from_str(input).map_err(|error| error.to_string())?;
            let needle = request["pattern"].as_str().ok_or("grep_pattern_missing")?;
            let content = fs::read_to_string(resolve(
                &grep_root,
                request["path"].as_str().ok_or("grep_path_missing")?,
            )?)
            .map_err(|error| error.to_string())?;
            let matches = content
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains(needle))
                .map(|(line, text)| format!("{}:{text}", line + 1))
                .collect::<Vec<_>>()
                .join("\n");
            output("grep", matches)
        }),
    );
    let edit_root = root.to_path_buf();
    registry.register(
        "coding/edit",
        StaticCapability::new(move |input, _| {
            let request: serde_json::Value =
                serde_json::from_str(input).map_err(|error| error.to_string())?;
            let path = resolve(
                &edit_root,
                request["path"].as_str().ok_or("edit_path_missing")?,
            )?;
            let old = request["old"].as_str().ok_or("edit_old_missing")?;
            let new = request["new"].as_str().ok_or("edit_new_missing")?;
            let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            if content.matches(old).count() != 1 {
                return Err("edit_requires_exactly_one_match".to_string());
            }
            fs::write(&path, content.replacen(old, new, 1)).map_err(|error| error.to_string())?;
            output("edit", path.to_string_lossy())
        }),
    );
    let write_root = root.to_path_buf();
    registry.register(
        "coding/write",
        StaticCapability::new(move |input, _| {
            let request: serde_json::Value =
                serde_json::from_str(input).map_err(|error| error.to_string())?;
            let path = resolve(
                &write_root,
                request["path"].as_str().ok_or("write_path_missing")?,
            )?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(
                &path,
                request["content"].as_str().ok_or("write_content_missing")?,
            )
            .map_err(|error| error.to_string())?;
            output("write", path.to_string_lossy())
        }),
    );
    let bash_root = root.to_path_buf();
    registry.register(
        "coding/bash",
        StaticCapability::new(move |input, _| {
            if input != "cargo test --offline" {
                return Err(format!("bash_command_denied:{input}"));
            }
            let result = Command::new("cargo")
                .args(["test", "--offline"])
                .current_dir(&bash_root)
                .output()
                .map_err(|error| error.to_string())?;
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            if !result.status.success() {
                return Err(format!("bash_failed:{combined}"));
            }
            output("bash", combined)
        }),
    );
    registry
}

fn resolve(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("workspace_escape_denied".to_string());
    }
    let candidate = root.join(relative);
    let boundary = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|error| error.to_string())?
    } else {
        let parent = candidate.parent().ok_or("workspace_parent_missing")?;
        let parent = parent.canonicalize().map_err(|error| error.to_string())?;
        parent.join(candidate.file_name().ok_or("workspace_filename_missing")?)
    };
    if !boundary.starts_with(root) {
        return Err("workspace_escape_denied".to_string());
    }
    Ok(boundary)
}

fn output(summary: &str, value: impl ToString) -> Result<EffectProposal, String> {
    Ok(EffectProposal {
        summary: summary.to_string(),
        messages: Vec::new(),
        outputs: vec![value.to_string()],
    })
}
