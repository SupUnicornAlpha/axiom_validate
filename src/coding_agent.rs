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

pub fn run_go_agent(
    workspace: impl Into<PathBuf>,
    task: &str,
) -> Result<CodingAgentResult, String> {
    let workspace = workspace.into();
    let binary = build_go_agent()?;
    let decisions = load_go_plan(&binary, task)?;
    let registry = go_tool_registry(&binary, &workspace);
    let kernel = Kernel::new(
        ReActScheduler::new(MockModelDriver::scripted(decisions)),
        AuditShell,
        LocalTransport::new(registry),
        None,
    );
    let mut spec = RunSpec::new("coding-agent-go-opencode-parity", task, Vec::new());
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
            .push(capability_id.into());
        spec.capability_leases.push(CapabilityLease {
            capability_id: capability_id.into(),
            permissions: vec!["invoke".into()],
        });
    }
    let report = kernel.run(&spec).map_err(|error| format!("{error:?}"))?;
    Ok(CodingAgentResult { report, workspace })
}

fn build_go_agent() -> Result<PathBuf, String> {
    let binary = PathBuf::from("reports/generated/axiom-coding-agent-go");
    std::fs::create_dir_all(binary.parent().expect("binary parent"))
        .map_err(|error| error.to_string())?;
    let binary = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(binary);
    let output = Command::new("go")
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .current_dir("agents/coding-go")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    binary.canonicalize().map_err(|error| error.to_string())
}

fn load_go_plan(binary: &Path, task: &str) -> Result<Vec<ModelDecision>, String> {
    let output = Command::new(binary)
        .args(["plan", task])
        .output()
        .map_err(|error| error.to_string())?;
    let values: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    values
        .into_iter()
        .map(|value| match value["kind"].as_str() {
            Some("invoke") => Ok(ModelDecision::Invoke {
                capability_id: value["capability_id"]
                    .as_str()
                    .ok_or("capability missing")?
                    .into(),
                input: value["input"].as_str().ok_or("input missing")?.into(),
            }),
            Some("respond") => Ok(ModelDecision::Respond {
                content: value["content"].as_str().ok_or("content missing")?.into(),
            }),
            Some("finish") => Ok(ModelDecision::Finish),
            _ => Err("unknown Go model decision"),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(str::to_string)
}

fn go_tool_registry(binary: &Path, workspace: &Path) -> CapabilityRegistry {
    let mut registry = CapabilityRegistry::new();
    for (capability_id, tool_name) in [
        ("coding/list", "list"),
        ("coding/read", "read"),
        ("coding/grep", "grep"),
        ("coding/edit", "edit"),
        ("coding/write", "write"),
        ("coding/bash", "bash"),
    ] {
        let binary = binary.to_path_buf();
        let workspace = workspace.to_path_buf();
        registry.register(
            capability_id,
            StaticCapability::new(move |input, _| {
                let output = Command::new(&binary)
                    .arg("tool")
                    .arg(tool_name)
                    .arg(&workspace)
                    .arg("invoke")
                    .arg(input)
                    .output()
                    .map_err(|error| error.to_string())?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
                }
                Ok(EffectProposal {
                    summary: format!("go_tool:{tool_name}"),
                    messages: Vec::new(),
                    outputs: vec![String::from_utf8_lossy(&output.stdout).to_string()],
                })
            }),
        );
    }
    registry
}
