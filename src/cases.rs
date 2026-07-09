use std::fs;
use std::path::PathBuf;

use axiom_core::{
    AuditShell, CapabilityRegistry, JsonlEventLog, Kernel, QueueScheduler, StaticCapability,
};
use axiom_spec::{CapabilityLease, Effect, MergeMode, Message, RunSpec, Step, StepAction};

use crate::report::{CaseResult, Metric};

pub struct ValidationCase {
    pub run: fn() -> CaseResult,
}

impl ValidationCase {
    pub fn run(&self) -> CaseResult {
        (self.run)()
    }
}

pub fn all_cases() -> Vec<ValidationCase> {
    vec![
        ValidationCase {
            run: kernel_replay_basic,
        },
        ValidationCase {
            run: shell_decision_allow_rewrite_deny,
        },
        ValidationCase {
            run: tool_syscall_audit,
        },
        ValidationCase {
            run: childrun_capability_lease_denied,
        },
        ValidationCase {
            run: childrun_merge_gate,
        },
    ]
}

fn base_registry() -> CapabilityRegistry {
    let mut registry = CapabilityRegistry::new();
    registry.register(
        "tool/echo",
        StaticCapability::new(|input, _ctx| {
            Ok(Effect {
                summary: "tool_echo".to_string(),
                messages: vec![Message {
                    role: "tool".to_string(),
                    content: format!("echo:{input}"),
                }],
                outputs: vec![input.to_string()],
            })
        }),
    );
    registry.register(
        "tool/write_patch",
        StaticCapability::new(|input, _ctx| {
            Ok(Effect {
                summary: "tool_write_patch".to_string(),
                messages: vec![],
                outputs: vec![format!("patch:{input}")],
            })
        }),
    );
    registry
}

fn kernel_replay_basic() -> CaseResult {
    let event_path = temp_event_path("kernel_replay_basic");
    let kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        base_registry(),
        Some(JsonlEventLog::new(&event_path)),
    );
    let mut spec = RunSpec::new(
        "kernel-replay-basic",
        "kernel replay basic",
        vec![
            msg_step("s1", "user says hi", "user", "hi"),
            tool_step("s2", "echo hi", "tool/echo", "hi"),
        ],
    );
    spec.capability_leases.push(lease("tool/echo"));

    let report = kernel.run(&spec).expect("kernel_replay_basic should run");
    let replay = JsonlEventLog::new(&event_path)
        .replay_summary()
        .expect("replay summary");
    let passed = replay.completed_runs == 1 && replay.total_events == report.events.len();

    CaseResult {
        case_id: "kernel_replay_basic".to_string(),
        category: "kernel".to_string(),
        passed,
        summary: "EventLog replay summary aligns with emitted events".to_string(),
        metrics: vec![
            Metric {
                name: "event_count".to_string(),
                value: report.events.len().to_string(),
            },
            Metric {
                name: "replay_completed_runs".to_string(),
                value: replay.completed_runs.to_string(),
            },
        ],
        evidence: vec![format!("event_log={}", event_path.display())],
    }
}

fn shell_decision_allow_rewrite_deny() -> CaseResult {
    let kernel = Kernel::new(QueueScheduler, AuditShell, base_registry(), None);
    let spec = RunSpec::new(
        "shell-decisions",
        "shell decisions",
        vec![
            tool_step("s1", "[rewrite] echo first", "tool/echo", "first"),
            tool_step("s2", "[deny] echo second", "tool/echo", "second"),
            tool_step("s3", "echo third", "tool/echo", "third"),
        ],
    );
    let mut spec = spec;
    spec.capability_leases.push(lease("tool/echo"));

    let report = kernel.run(&spec).expect("shell decisions should run");
    let denied = report.state.denied_actions.len();
    let rewritten = report
        .events
        .iter()
        .any(|event| event.detail.starts_with("rewrite:"));
    let passed = denied == 1 && rewritten && report.state.outputs == vec!["first", "third"];

    CaseResult {
        case_id: "shell_decision_allow_rewrite_deny".to_string(),
        category: "shell".to_string(),
        passed,
        summary: "Shell can allow, rewrite, or deny without directly producing effects".to_string(),
        metrics: vec![
            Metric {
                name: "denied_actions".to_string(),
                value: denied.to_string(),
            },
            Metric {
                name: "output_count".to_string(),
                value: report.state.outputs.len().to_string(),
            },
        ],
        evidence: report.events.iter().map(|event| format!("{:?}:{}", event.kind, event.detail)).collect(),
    }
}

fn tool_syscall_audit() -> CaseResult {
    let kernel = Kernel::new(QueueScheduler, AuditShell, base_registry(), None);
    let mut spec = RunSpec::new(
        "tool-syscall-audit",
        "tool syscall audit",
        vec![
            tool_step("s1", "echo one", "tool/echo", "one"),
            tool_step("s2", "echo two", "tool/echo", "two"),
        ],
    );
    spec.capability_leases.push(lease("tool/echo"));

    let report = kernel.run(&spec).expect("tool audit should run");
    let started = report
        .events
        .iter()
        .filter(|event| matches!(event.kind, axiom_spec::EventKind::StepStarted))
        .count();
    let completed = report
        .events
        .iter()
        .filter(|event| matches!(event.kind, axiom_spec::EventKind::StepCompleted))
        .count();
    let passed = started == 2 && completed == 2;

    CaseResult {
        case_id: "tool_syscall_audit".to_string(),
        category: "audit".to_string(),
        passed,
        summary: "Every tool invocation is visible in the event stream".to_string(),
        metrics: vec![
            Metric {
                name: "step_started".to_string(),
                value: started.to_string(),
            },
            Metric {
                name: "step_completed".to_string(),
                value: completed.to_string(),
            },
        ],
        evidence: report.state.outputs,
    }
}

fn childrun_capability_lease_denied() -> CaseResult {
    let kernel = Kernel::new(QueueScheduler, AuditShell, base_registry(), None);

    let child = RunSpec::new(
        "child-denied",
        "child denied",
        vec![tool_step("child-s1", "child patch", "tool/write_patch", "fix bug")],
    );
    let mut parent = RunSpec::new(
        "parent-denied",
        "parent denied",
        vec![Step {
            id: "parent-s1".to_string(),
            title: "delegate child".to_string(),
            action: StepAction::Delegate {
                child: Box::new(child),
                merge_mode: MergeMode::SummaryOnly,
            },
        }],
    );
    parent.capability_leases.push(lease("tool/echo"));

    let result = kernel.run(&parent);
    let passed = result.is_err();

    CaseResult {
        case_id: "childrun_capability_lease_denied".to_string(),
        category: "childrun".to_string(),
        passed,
        summary: "Child run without delegated capability is denied".to_string(),
        metrics: vec![Metric {
            name: "permission_denial_rate".to_string(),
            value: if passed { "1.0" } else { "0.0" }.to_string(),
        }],
        evidence: vec![format!("result={result:?}")],
    }
}

fn childrun_merge_gate() -> CaseResult {
    let kernel = Kernel::new(QueueScheduler, AuditShell, base_registry(), None);

    let mut child = RunSpec::new(
        "child-merge",
        "child merge",
        vec![
            msg_step("child-s1", "child analyst", "assistant", "analysis ready"),
            tool_step("child-s2", "child patch", "tool/write_patch", "apply diff"),
        ],
    );
    child.capability_leases.push(lease("tool/write_patch"));

    let mut parent = RunSpec::new(
        "parent-merge",
        "parent merge",
        vec![Step {
            id: "parent-s1".to_string(),
            title: "delegate merge".to_string(),
            action: StepAction::Delegate {
                child: Box::new(child),
                merge_mode: MergeMode::AppendMessages,
            },
        }],
    );
    parent.capability_leases.push(lease("tool/echo"));
    parent.capability_leases.push(lease("tool/write_patch"));

    let report = kernel.run(&parent).expect("child merge should run");
    let passed = report
        .state
        .outputs
        .iter()
        .any(|output| output == "patch:apply diff")
        && report
            .state
            .messages
            .iter()
            .any(|message| message.content == "analysis ready");

    CaseResult {
        case_id: "childrun_merge_gate".to_string(),
        category: "childrun".to_string(),
        passed,
        summary: "Child run results merge through explicit parent merge mode".to_string(),
        metrics: vec![
            Metric {
                name: "merged_messages".to_string(),
                value: report.state.messages.len().to_string(),
            },
            Metric {
                name: "merged_outputs".to_string(),
                value: report.state.outputs.len().to_string(),
            },
        ],
        evidence: report.state.outputs,
    }
}

fn temp_event_path(case_id: &str) -> PathBuf {
    let root = PathBuf::from("reports/eventlogs");
    let _ = fs::create_dir_all(&root);
    let path = root.join(format!("{case_id}.jsonl"));
    let _ = fs::remove_file(&path);
    path
}

fn msg_step(id: &str, title: &str, role: &str, content: &str) -> Step {
    Step {
        id: id.to_string(),
        title: title.to_string(),
        action: StepAction::Message {
            role: role.to_string(),
            content: content.to_string(),
        },
    }
}

fn tool_step(id: &str, title: &str, capability_id: &str, input: &str) -> Step {
    Step {
        id: id.to_string(),
        title: title.to_string(),
        action: StepAction::CapabilityInvoke {
            capability_id: capability_id.to_string(),
            input: input.to_string(),
        },
    }
}

fn lease(capability_id: &str) -> CapabilityLease {
    CapabilityLease {
        capability_id: capability_id.to_string(),
        permissions: vec!["invoke".to_string()],
    }
}
