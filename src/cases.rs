use std::fs;
use std::path::PathBuf;
use std::process::Command;

use axiom_core::{
    AuditShell, CapabilityRegistry, CompositeShell, JsonlEventLog, Kernel, LocalTransport,
    MinimalPolicyEngine, PolicyMiddleware, QueueScheduler, RemoteSubRunTransportMock,
    RemoteTransportMock, StaticCapability, TitlePolicyMiddleware,
};
use axiom_spec::{
    CapabilityLease, ChildRunSpec, Effect, MergeMode, Message, RunSpec, Step, StepAction,
};

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
            run: golden_eventlog_match,
        },
        ValidationCase {
            run: kernel_replay_basic,
        },
        ValidationCase {
            run: shell_decision_allow_rewrite_deny,
        },
        ValidationCase {
            run: shell_policy_engine_capability_deny,
        },
        ValidationCase {
            run: tool_syscall_audit,
        },
        ValidationCase {
            run: childrun_capability_lease_denied,
        },
        ValidationCase {
            run: childrun_sandbox_inheritance,
        },
        ValidationCase {
            run: childrun_merge_gate,
        },
        ValidationCase {
            run: subrun_transport_invariance,
        },
        ValidationCase {
            run: coding_patch_small,
        },
        ValidationCase {
            run: local_remote_invariance,
        },
        ValidationCase {
            run: ts_sdk_conformance_runspec,
        },
        ValidationCase {
            run: py_sdk_conformance_runspec,
        },
        ValidationCase {
            run: research_brief_agent,
        },
        ValidationCase {
            run: wrap_vs_native_audit,
        },
    ]
}

fn golden_eventlog_match() -> CaseResult {
    let event_path = temp_event_path("kernel_replay_basic.validator");
    let kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
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

    let _ = kernel
        .run(&spec)
        .expect("golden eventlog run should succeed");

    let golden_path = PathBuf::from("fixtures/eventlog/kernel_replay_basic.golden.jsonl");
    let actual = fs::read_to_string(&event_path).expect("actual eventlog readable");
    let golden = fs::read_to_string(&golden_path).expect("golden eventlog readable");

    let validator_script = PathBuf::from("runners/validate-eventlog.mjs");
    let validator = Command::new("node")
        .arg(&validator_script)
        .arg(&event_path)
        .output()
        .expect("eventlog validator should run");
    let validator_stdout = String::from_utf8(validator.stdout).expect("validator output utf8");
    let valid_lines = validator_stdout.contains("\"invalidLines\":0");
    let content_match = actual == golden;
    let passed = validator.status.success() && valid_lines && content_match;

    CaseResult {
        case_id: "golden_eventlog_match".to_string(),
        category: "eventlog".to_string(),
        passed,
        summary: "Generated EventLog matches golden fixture and passes validator".to_string(),
        metrics: vec![
            Metric {
                name: "content_match".to_string(),
                value: content_match.to_string(),
            },
            Metric {
                name: "validator_clean".to_string(),
                value: valid_lines.to_string(),
            },
            Metric {
                name: "actual_lines".to_string(),
                value: actual.lines().count().to_string(),
            },
        ],
        evidence: vec![
            format!("actual={}", event_path.display()),
            format!("golden={}", golden_path.display()),
            format!("validator={}", validator_script.display()),
        ],
    }
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
    registry.register(
        "tool/compose_brief",
        StaticCapability::new(|input, _ctx| {
            Ok(Effect {
                summary: "tool_compose_brief".to_string(),
                messages: vec![Message {
                    role: "tool".to_string(),
                    content: format!("brief:market={input};key_points=3;risks=2"),
                }],
                outputs: vec![format!("brief:{input}")],
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
        LocalTransport::new(base_registry()),
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
    let mut shell = CompositeShell::new();
    shell.push(TitlePolicyMiddleware);
    let kernel = Kernel::new(
        QueueScheduler,
        shell,
        LocalTransport::new(base_registry()),
        None,
    );
    let mut spec = RunSpec::new(
        "shell-decisions",
        "shell decisions",
        vec![
            tool_step("s1", "[rewrite] echo first", "tool/echo", "first"),
            tool_step("s2", "[deny] echo second", "tool/echo", "second"),
            tool_step("s3", "echo third", "tool/echo", "third"),
        ],
    );
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
        evidence: report
            .events
            .iter()
            .map(|event| format!("{:?}:{}", event.kind, event.detail))
            .collect(),
    }
}

fn shell_policy_engine_capability_deny() -> CaseResult {
    let mut shell = CompositeShell::new();
    shell.push(TitlePolicyMiddleware);
    shell.push(PolicyMiddleware::new(
        MinimalPolicyEngine::new().deny_capability("tool/write_patch"),
    ));
    let kernel = Kernel::new(
        QueueScheduler,
        shell,
        LocalTransport::new(base_registry()),
        None,
    );

    let mut spec = RunSpec::new(
        "shell-policy-capability-deny",
        "shell policy capability deny",
        vec![
            tool_step("s1", "safe echo", "tool/echo", "ok"),
            tool_step("s2", "blocked patch", "tool/write_patch", "forbidden"),
        ],
    );
    spec.capability_leases.push(lease("tool/echo"));
    spec.capability_leases.push(lease("tool/write_patch"));

    let report = kernel.run(&spec).expect("policy deny should not fail run");
    let denied = report.state.denied_actions.contains(&"s2".to_string());
    let allowed_output = report.state.outputs == vec!["ok"];
    let denial_event = report.events.iter().any(|event| {
        event
            .detail
            .contains("policy_denied_capability:tool/write_patch")
    });
    let passed = denied && allowed_output && denial_event;

    CaseResult {
        case_id: "shell_policy_engine_capability_deny".to_string(),
        category: "shell".to_string(),
        passed,
        summary: "Minimal PolicyEngine can deny capability invocation through the shell chain"
            .to_string(),
        metrics: vec![
            Metric {
                name: "denied".to_string(),
                value: denied.to_string(),
            },
            Metric {
                name: "allowed_output_only".to_string(),
                value: allowed_output.to_string(),
            },
            Metric {
                name: "denial_event".to_string(),
                value: denial_event.to_string(),
            },
        ],
        evidence: report
            .events
            .iter()
            .map(|event| format!("{:?}:{}", event.kind, event.detail))
            .collect(),
    }
}

fn tool_syscall_audit() -> CaseResult {
    let kernel = local_kernel();
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
    let kernel = local_kernel();
    let child = RunSpec::new(
        "child-denied",
        "child denied",
        vec![tool_step(
            "child-s1",
            "child patch",
            "tool/write_patch",
            "fix bug",
        )],
    );
    let mut parent = RunSpec::new(
        "parent-denied",
        "parent denied",
        vec![Step {
            id: "parent-s1".to_string(),
            title: "delegate child".to_string(),
            action: StepAction::Delegate {
                child: Box::new(ChildRunSpec::new("parent-denied", child)),
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

fn childrun_sandbox_inheritance() -> CaseResult {
    let kernel = local_kernel();

    let mut permission_child = RunSpec::new("child-permission", "child permission", Vec::new());
    permission_child.capability_leases.push(CapabilityLease {
        capability_id: "tool/echo".to_string(),
        permissions: vec!["admin".to_string()],
    });
    let permission_result = kernel.run(&parent_for_child(
        "parent-permission",
        permission_child,
        lease("tool/echo"),
    ));

    let mut namespace_child = RunSpec::new("child-namespace", "child namespace", Vec::new());
    namespace_child.namespace.workspace_root = "/outside".to_string();
    let namespace_result = kernel.run(&parent_for_child(
        "parent-namespace",
        namespace_child,
        lease("tool/echo"),
    ));

    let mut budget_child = RunSpec::new("child-budget", "child budget", Vec::new());
    budget_child.budget.max_steps = 129;
    let budget_result = kernel.run(&parent_for_child(
        "parent-budget",
        budget_child,
        lease("tool/echo"),
    ));

    let permission_denied =
        format!("{permission_result:?}").contains("child_permission_not_delegated");
    let namespace_denied =
        format!("{namespace_result:?}").contains("child_namespace_outside_parent");
    let budget_denied = format!("{budget_result:?}").contains("child_budget_exceeds_parent");

    CaseResult {
        case_id: "childrun_sandbox_inheritance".to_string(),
        category: "childrun".to_string(),
        passed: permission_denied && namespace_denied && budget_denied,
        summary: "ChildRun permissions, namespace, and budget can only narrow parent authority"
            .to_string(),
        metrics: vec![
            Metric {
                name: "permission_denied".to_string(),
                value: permission_denied.to_string(),
            },
            Metric {
                name: "namespace_denied".to_string(),
                value: namespace_denied.to_string(),
            },
            Metric {
                name: "budget_denied".to_string(),
                value: budget_denied.to_string(),
            },
        ],
        evidence: vec![
            format!("permission={permission_result:?}"),
            format!("namespace={namespace_result:?}"),
            format!("budget={budget_result:?}"),
        ],
    }
}

fn childrun_merge_gate() -> CaseResult {
    let kernel = local_kernel();
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
                child: Box::new(ChildRunSpec::new("parent-merge", child)),
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

fn subrun_transport_invariance() -> CaseResult {
    let spec = coding_patch_small_spec();
    let local_report = local_kernel().run(&spec).expect("local subrun should run");
    let remote_kernel = Kernel::with_subrun_transport(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        RemoteSubRunTransportMock,
        None,
    );
    let remote_report = remote_kernel
        .run(&spec)
        .expect("remote subrun mock should run");
    let state_match = local_report.state == remote_report.state;
    let event_kinds_match = local_report
        .events
        .iter()
        .map(|event| &event.kind)
        .eq(remote_report.events.iter().map(|event| &event.kind));

    CaseResult {
        case_id: "subrun_transport_invariance".to_string(),
        category: "childrun".to_string(),
        passed: state_match && event_kinds_match,
        summary: "Local and remote-mock ChildRun transports preserve kernel semantics".to_string(),
        metrics: vec![
            Metric {
                name: "state_match".to_string(),
                value: state_match.to_string(),
            },
            Metric {
                name: "event_kinds_match".to_string(),
                value: event_kinds_match.to_string(),
            },
        ],
        evidence: remote_report.state.outputs,
    }
}

fn coding_patch_small() -> CaseResult {
    let kernel = local_kernel();
    let spec = coding_patch_small_spec();
    let report = kernel.run(&spec).expect("coding_patch_small should run");

    let task_success = report.state.outputs.iter().any(|output| output == "hello")
        && report
            .state
            .outputs
            .iter()
            .any(|output| output == "patch:replace hi with hello");
    let review_merged = report
        .state
        .messages
        .iter()
        .any(|message| message.content == "approved");
    let passed = task_success && review_merged;

    CaseResult {
        case_id: "coding_patch_small".to_string(),
        category: "agents".to_string(),
        passed,
        summary: "Minimal coding agent can patch, delegate review, and produce audited output"
            .to_string(),
        metrics: vec![
            Metric {
                name: "task_success".to_string(),
                value: if task_success { "1.0" } else { "0.0" }.to_string(),
            },
            Metric {
                name: "review_merged".to_string(),
                value: if review_merged { "1.0" } else { "0.0" }.to_string(),
            },
            Metric {
                name: "event_count".to_string(),
                value: report.events.len().to_string(),
            },
        ],
        evidence: report
            .state
            .outputs
            .into_iter()
            .chain(
                report
                    .state
                    .messages
                    .into_iter()
                    .map(|message| message.content),
            )
            .collect(),
    }
}

fn local_remote_invariance() -> CaseResult {
    let spec = coding_patch_small_spec();
    let local = local_kernel()
        .run(&spec)
        .expect("local invariance run should succeed");
    let remote = remote_kernel()
        .run(&spec)
        .expect("remote invariance run should succeed");

    let same_outputs = local.state.outputs == remote.state.outputs;
    let same_messages = local.state.messages == remote.state.messages;
    let same_denials = local.state.denied_actions == remote.state.denied_actions;
    let same_event_details = local
        .events
        .iter()
        .map(|event| format!("{:?}:{}", event.kind, event.detail))
        .collect::<Vec<_>>()
        == remote
            .events
            .iter()
            .map(|event| format!("{:?}:{}", event.kind, event.detail))
            .collect::<Vec<_>>();
    let passed = same_outputs && same_messages && same_denials && same_event_details;

    CaseResult {
        case_id: "local_remote_invariance".to_string(),
        category: "transport".to_string(),
        passed,
        summary: "LocalTransport and RemoteTransportMock preserve the same observable semantics"
            .to_string(),
        metrics: vec![
            Metric {
                name: "same_outputs".to_string(),
                value: same_outputs.to_string(),
            },
            Metric {
                name: "same_messages".to_string(),
                value: same_messages.to_string(),
            },
            Metric {
                name: "same_event_details".to_string(),
                value: same_event_details.to_string(),
            },
            Metric {
                name: "local_event_count".to_string(),
                value: local.events.len().to_string(),
            },
            Metric {
                name: "remote_event_count".to_string(),
                value: remote.events.len().to_string(),
            },
        ],
        evidence: vec![
            format!("local_outputs={:?}", local.state.outputs),
            format!("remote_outputs={:?}", remote.state.outputs),
            format!("local_events={}", local.events.len()),
            format!("remote_events={}", remote.events.len()),
        ],
    }
}

fn ts_sdk_conformance_runspec() -> CaseResult {
    let script =
        PathBuf::from("../axiom_kernal/sdks/typescript/scripts/build-coding-patch-small.mjs");
    let compare_script = PathBuf::from("runners/compare-json.mjs");
    let fixture_path = PathBuf::from("fixtures/runspec/coding_patch_small.json");
    sdk_conformance_case(
        "ts_sdk_conformance_runspec",
        "TypeScript SDK generates the same RunSpec as the golden fixture",
        "node",
        vec![script.to_string_lossy().to_string()],
        compare_script,
        fixture_path,
        "coding_patch_small.generated.json",
    )
}

fn py_sdk_conformance_runspec() -> CaseResult {
    let script = PathBuf::from("../axiom_kernal/sdks/python/scripts/build_coding_patch_small.py");
    let compare_script = PathBuf::from("runners/compare-json.mjs");
    let fixture_path = PathBuf::from("fixtures/runspec/coding_patch_small.json");
    sdk_conformance_case(
        "py_sdk_conformance_runspec",
        "Python SDK generates the same RunSpec as the golden fixture",
        "python3",
        vec![script.to_string_lossy().to_string()],
        compare_script,
        fixture_path,
        "coding_patch_small.py.generated.json",
    )
}

fn sdk_conformance_case(
    case_id: &str,
    summary: &str,
    program: &str,
    args: Vec<String>,
    compare_script: PathBuf,
    fixture_path: PathBuf,
    generated_name: &str,
) -> CaseResult {
    let generated_path = temp_generated_path(generated_name);
    let output = Command::new(program)
        .args(args.clone())
        .output()
        .expect("sdk generator should run");
    let generated = String::from_utf8(output.stdout).expect("utf8 json output");
    fs::write(&generated_path, &generated).expect("generated json should be writable");

    let compare = Command::new("node")
        .arg(&compare_script)
        .arg(&generated_path)
        .arg(&fixture_path)
        .output()
        .expect("node compare script should run");
    let compare_stdout = String::from_utf8(compare.stdout).expect("compare result utf8");
    let equal = compare_stdout.contains("\"equal\":true");
    let passed = output.status.success() && compare.status.success() && equal;

    CaseResult {
        case_id: case_id.to_string(),
        category: "sdk".to_string(),
        passed,
        summary: summary.to_string(),
        metrics: vec![
            Metric {
                name: "generator_exit_success".to_string(),
                value: output.status.success().to_string(),
            },
            Metric {
                name: "json_match".to_string(),
                value: equal.to_string(),
            },
            Metric {
                name: "generated_bytes".to_string(),
                value: generated.len().to_string(),
            },
        ],
        evidence: vec![
            format!("program={program}"),
            format!("args={args:?}"),
            format!("compare_script={}", compare_script.display()),
            format!("generated={}", generated_path.display()),
            format!("fixture={}", fixture_path.display()),
        ],
    }
}

fn research_brief_agent() -> CaseResult {
    let kernel = local_kernel();
    let spec = research_brief_spec();
    let report = kernel.run(&spec).expect("research brief should run");

    let brief_output = report
        .state
        .outputs
        .iter()
        .any(|output| output == "brief:cloud database market");
    let publish_output = report
        .state
        .outputs
        .iter()
        .any(|output| output == "brief ready");
    let tool_message = report
        .state
        .messages
        .iter()
        .any(|message| message.content.contains("key_points=3"));
    let passed = brief_output && publish_output && tool_message;

    CaseResult {
        case_id: "research_brief_agent".to_string(),
        category: "agents".to_string(),
        passed,
        summary: "Research brief agent can compose and publish a brief with auditable outputs"
            .to_string(),
        metrics: vec![
            Metric {
                name: "brief_completeness".to_string(),
                value: passed.to_string(),
            },
            Metric {
                name: "event_count".to_string(),
                value: report.events.len().to_string(),
            },
            Metric {
                name: "output_count".to_string(),
                value: report.state.outputs.len().to_string(),
            },
        ],
        evidence: report
            .state
            .outputs
            .into_iter()
            .chain(
                report
                    .state
                    .messages
                    .into_iter()
                    .map(|message| message.content),
            )
            .collect(),
    }
}

fn wrap_vs_native_audit() -> CaseResult {
    let spec = coding_patch_small_spec();
    let wrapped = local_kernel()
        .run(&spec)
        .expect("wrapped kernel run should succeed");
    let native_outputs = vec![
        "patch:replace hi with hello".to_string(),
        "hello".to_string(),
    ];
    let native_audit_events = 0usize;

    let wrapped_event_count = wrapped.events.len();
    let wrapped_audit_coverage = wrapped
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                axiom_spec::EventKind::StepStarted
                    | axiom_spec::EventKind::StepCompleted
                    | axiom_spec::EventKind::EffectApplied
                    | axiom_spec::EventKind::ShellDecision
            )
        })
        .count();
    let outputs_equal = wrapped.state.outputs == native_outputs;
    let audit_gain = wrapped_audit_coverage > native_audit_events;
    let passed = outputs_equal && audit_gain;

    CaseResult {
        case_id: "wrap_vs_native_audit".to_string(),
        category: "wrap".to_string(),
        passed,
        summary:
            "Wrap mode preserves task result while adding audit visibility over native execution"
                .to_string(),
        metrics: vec![
            Metric {
                name: "outputs_equal".to_string(),
                value: outputs_equal.to_string(),
            },
            Metric {
                name: "wrapped_event_count".to_string(),
                value: wrapped_event_count.to_string(),
            },
            Metric {
                name: "wrapped_audit_coverage".to_string(),
                value: wrapped_audit_coverage.to_string(),
            },
            Metric {
                name: "native_audit_coverage".to_string(),
                value: native_audit_events.to_string(),
            },
        ],
        evidence: vec![
            format!("wrapped_outputs={:?}", wrapped.state.outputs),
            format!("native_outputs={:?}", native_outputs),
            "native_mode_assumption=no kernel event stream".to_string(),
        ],
    }
}

fn local_kernel() -> Kernel<QueueScheduler, AuditShell, LocalTransport> {
    Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        None,
    )
}

fn parent_for_child(parent_run_id: &str, child: RunSpec, parent_lease: CapabilityLease) -> RunSpec {
    let mut parent = RunSpec::new(
        parent_run_id,
        "sandbox inheritance parent",
        vec![Step {
            id: "delegate".to_string(),
            title: "delegate child".to_string(),
            action: StepAction::Delegate {
                child: Box::new(ChildRunSpec::new(parent_run_id, child)),
                merge_mode: MergeMode::SummaryOnly,
            },
        }],
    );
    parent.capability_leases.push(parent_lease);
    parent
}

fn remote_kernel() -> Kernel<QueueScheduler, AuditShell, RemoteTransportMock> {
    Kernel::new(
        QueueScheduler,
        AuditShell,
        RemoteTransportMock::new(base_registry()),
        None,
    )
}

fn coding_patch_small_spec() -> RunSpec {
    let reviewer = RunSpec::new(
        "reviewer-child",
        "reviewer child",
        vec![
            msg_step(
                "review-1",
                "review findings",
                "assistant",
                "patch looks safe",
            ),
            msg_step("review-2", "review verdict", "assistant", "approved"),
        ],
    );

    let mut spec = RunSpec::new(
        "coding-patch-small",
        "coding patch small",
        vec![
            msg_step("s1", "understand task", "user", "fix greeting output"),
            tool_step(
                "s2",
                "draft patch",
                "tool/write_patch",
                "replace hi with hello",
            ),
            Step {
                id: "s3".to_string(),
                title: "delegate reviewer".to_string(),
                action: StepAction::Delegate {
                    child: Box::new(ChildRunSpec::new("coding-patch-small", reviewer)),
                    merge_mode: MergeMode::AppendMessages,
                },
            },
            tool_step("s4", "echo final result", "tool/echo", "hello"),
        ],
    );
    spec.capability_leases.push(lease("tool/write_patch"));
    spec.capability_leases.push(lease("tool/echo"));
    spec
}

fn research_brief_spec() -> RunSpec {
    let mut spec = RunSpec::new(
        "research-brief-agent",
        "research brief agent",
        vec![
            msg_step(
                "r1",
                "collect ask",
                "user",
                "summarize cloud database market",
            ),
            tool_step(
                "r2",
                "compose brief",
                "tool/compose_brief",
                "cloud database market",
            ),
            tool_step("r3", "publish brief", "tool/echo", "brief ready"),
        ],
    );
    spec.capability_leases.push(lease("tool/compose_brief"));
    spec.capability_leases.push(lease("tool/echo"));
    spec
}

fn temp_event_path(case_id: &str) -> PathBuf {
    let root = PathBuf::from("reports/eventlogs");
    let _ = fs::create_dir_all(&root);
    let path = root.join(format!("{case_id}.jsonl"));
    let _ = fs::remove_file(&path);
    path
}

fn temp_generated_path(name: &str) -> PathBuf {
    let root = PathBuf::from("reports/generated");
    let _ = fs::create_dir_all(&root);
    let path = root.join(name);
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
