use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use axiom_core::{
    AuditShell, CapabilityRegistry, Kernel, LocalTransport, MockModelDriver, ModelDecision,
    ModelDriver, ReActScheduler, StaticCapability,
};
use axiom_spec::{CapabilityLease, EffectProposal, RunSpec, RunState};

pub struct CodingAgentResult {
    pub report: axiom_core::RunReport,
    pub workspace: PathBuf,
}

/// Offline regression path: Go emits a full scripted plan once.
pub fn run_go_agent(
    workspace: impl Into<PathBuf>,
    task: &str,
) -> Result<CodingAgentResult, String> {
    let workspace = workspace.into();
    let binary = build_go_agent()?;
    let decisions = load_go_plan(&binary, task)?;
    run_with_model(
        &binary,
        &workspace,
        task,
        "coding-agent-go-opencode-parity",
        MockModelDriver::scripted(decisions),
    )
}

/// Live ReAct path: each turn shells out to Go `decide` (DeepSeek).
pub fn run_go_agent_live(
    workspace: impl Into<PathBuf>,
    task: &str,
) -> Result<CodingAgentResult, String> {
    let workspace = workspace.into();
    let binary = build_go_agent()?;
    let driver = GoDecideModelDriver::new(binary.clone(), workspace.clone());
    run_with_model(
        &binary,
        &workspace,
        task,
        "coding-agent-go-deepseek-live",
        driver,
    )
}

fn run_with_model<M: ModelDriver>(
    binary: &Path,
    workspace: &Path,
    task: &str,
    run_id: &str,
    model: M,
) -> Result<CodingAgentResult, String> {
    let registry = go_tool_registry(binary, workspace);
    let kernel = Kernel::new(
        ReActScheduler::new(model),
        AuditShell,
        LocalTransport::new(registry),
        None,
    );
    let mut spec = RunSpec::new(run_id, task, Vec::new());
    spec.namespace.workspace_root = workspace.to_string_lossy().to_string();
    spec.budget.max_steps = 48;
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
    Ok(CodingAgentResult {
        report,
        workspace: workspace.to_path_buf(),
    })
}

pub fn build_go_agent() -> Result<PathBuf, String> {
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
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let values: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    values
        .into_iter()
        .map(parse_decision_value)
        .collect::<Result<Vec<_>, _>>()
}

fn parse_decision_value(value: serde_json::Value) -> Result<ModelDecision, String> {
    match value["kind"].as_str() {
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
        _ => Err("unknown Go model decision".into()),
    }
}

struct GoDecideModelDriver {
    binary: PathBuf,
    workspace: PathBuf,
}

impl GoDecideModelDriver {
    fn new(binary: PathBuf, workspace: PathBuf) -> Self {
        Self { binary, workspace }
    }
}

impl ModelDriver for GoDecideModelDriver {
    fn decide(&self, spec: &RunSpec, state: &RunState) -> Result<ModelDecision, String> {
        let observation = serde_json::json!({
            "task": spec.name,
            "messages": state.messages.iter().map(|m| serde_json::json!({
                "role": m.role,
                "content": m.content,
            })).collect::<Vec<_>>(),
            "outputs": state.outputs,
            "denied_actions": state.denied_actions,
            "next_step_index": state.next_step_index,
            "visible_capabilities": spec.namespace.visible_capabilities,
            "workspace_root": self.workspace.to_string_lossy(),
        });
        let payload = serde_json::to_vec(&observation).map_err(|e| e.to_string())?;
        let mut child = Command::new(&self.binary)
            .args(["decide", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "decide stdin missing".to_string())?;
            stdin
                .write_all(&payload)
                .map_err(|error| error.to_string())?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "go decide failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "invalid decide json: {error}; stdout={}",
                String::from_utf8_lossy(&output.stdout)
            )
        })?;
        parse_decision_value(value)
    }
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
                // Tool-level failures become observations so ReAct can recover
                // instead of aborting the Kernel run (Capability Err).
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    return Ok(EffectProposal {
                        summary: format!("go_tool:{tool_name}:error"),
                        messages: Vec::new(),
                        outputs: vec![format!("tool_error: {err}")],
                    });
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
