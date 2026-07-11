use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{
    migrate_checkpoint_file, migrate_checkpoint_json, migrate_event_json, migrate_event_log,
    validate_run_spec_version, AuditShell, CapabilityRegistry, CheckpointMigrationContext,
    CliDriver, CompositeShell, EventMigrationContext, FileRunLeaseStore, FileRunStore,
    FileSnapshotArchive, FilesystemDriver, JsonlEventLog, Kernel, LocalSubRunTransport,
    LocalTransport, MemoryRunStore, MigrationStatus, MinimalPolicyEngine, MockModelDriver,
    ModelDecision, ModelDriver, PolicyMiddleware, QueueScheduler, ReActScheduler,
    RemoteSubRunTransportMock, RemoteTransportMock, RunLeaseStore, RunStore, RunStoreRecord,
    SnapshotArchive, StaticCapability, TitlePolicyMiddleware,
};
use axiom_spec::{
    CapabilityLease, ChildRunSpec, EffectProposal, Event, EventKind, MergeMode, Message, RunSpec,
    RunState, Step, StepAction,
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
            run: effect_commit_boundary,
        },
        ValidationCase {
            run: eventlog_failure_is_fatal,
        },
        ValidationCase {
            run: runstore_checkpoint_resume,
        },
        ValidationCase {
            run: journal_checkpoint_crash_recovery,
        },
        ValidationCase {
            run: writer_lease_epoch_fencing,
        },
        ValidationCase {
            run: schema_migration_compatibility_matrix,
        },
        ValidationCase {
            run: journal_maintenance_and_snapshot_retention,
        },
        ValidationCase {
            run: toy_tool_agent,
        },
        ValidationCase {
            run: native_driver_contracts,
        },
        ValidationCase {
            run: durable_scheduler_proposal_recovery,
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
            run: sdk_spec_digest_conformance,
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

    let validator_script = PathBuf::from("runners/validate-eventlog.mjs");
    let validator = Command::new("node")
        .arg(&validator_script)
        .arg(&event_path)
        .output()
        .expect("eventlog validator should run");
    let validator_stdout = String::from_utf8(validator.stdout).expect("validator output utf8");
    let valid_lines = validator_stdout.contains("\"invalidLines\":0");
    let comparator = Command::new("node")
        .arg("runners/compare-eventlog.mjs")
        .arg(&event_path)
        .arg(&golden_path)
        .output()
        .expect("eventlog comparator should run");
    let content_match = comparator.status.success();
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
            Ok(EffectProposal {
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
            Ok(EffectProposal {
                summary: "tool_write_patch".to_string(),
                messages: vec![],
                outputs: vec![format!("patch:{input}")],
            })
        }),
    );
    registry.register(
        "tool/compose_brief",
        StaticCapability::new(|input, _ctx| {
            Ok(EffectProposal {
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

fn effect_commit_boundary() -> CaseResult {
    let report = local_kernel()
        .run(&RunSpec::new(
            "effect-commit-boundary",
            "effect commit boundary",
            vec![msg_step("s1", "commit message", "assistant", "committed")],
        ))
        .expect("effect commit case should run");
    let proposed_index = report
        .events
        .iter()
        .position(|event| matches!(event.kind, axiom_spec::EventKind::EffectProposed));
    let committed_index = report
        .events
        .iter()
        .position(|event| matches!(event.kind, axiom_spec::EventKind::EffectCommitted));
    let ordered = matches!((proposed_index, committed_index), (Some(proposed), Some(committed)) if proposed < committed);
    let applied_once =
        report.state.messages.len() == 1 && report.state.messages[0].content == "committed";

    CaseResult {
        case_id: "effect_commit_boundary".to_string(),
        category: "kernel".to_string(),
        passed: ordered && applied_once,
        summary: "Drivers propose effects and only the kernel commits state changes".to_string(),
        metrics: vec![
            Metric {
                name: "proposal_before_commit".to_string(),
                value: ordered.to_string(),
            },
            Metric {
                name: "state_applied_once".to_string(),
                value: applied_once.to_string(),
            },
        ],
        evidence: report
            .events
            .iter()
            .map(|event| format!("{:?}:{}", event.kind, event.detail))
            .collect(),
    }
}

fn eventlog_failure_is_fatal() -> CaseResult {
    let blocker = temp_event_path("eventlog-parent-file");
    fs::write(&blocker, "not a directory").expect("write eventlog blocker");
    let event_path = blocker.join("events.jsonl");
    let kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        Some(JsonlEventLog::new(event_path)),
    );
    let result = kernel.run(&RunSpec::new(
        "eventlog-failure",
        "eventlog failure",
        Vec::new(),
    ));
    let fatal = matches!(&result, Err(axiom_core::KernelError::EventLog(_)));

    CaseResult {
        case_id: "eventlog_failure_is_fatal".to_string(),
        category: "eventlog".to_string(),
        passed: fatal,
        summary: "Kernel aborts instead of executing without an audit log".to_string(),
        metrics: vec![Metric {
            name: "write_failure_aborted".to_string(),
            value: fatal.to_string(),
        }],
        evidence: vec![format!("result={result:?}")],
    }
}

fn runstore_checkpoint_resume() -> CaseResult {
    let store = MemoryRunStore::new();
    let mut failing_registry = CapabilityRegistry::new();
    failing_registry.register(
        "tool/unstable",
        StaticCapability::new(|_, _| Err("transient_driver_failure".to_string())),
    );
    let mut spec = RunSpec::new(
        "checkpoint-resume",
        "checkpoint resume",
        vec![
            msg_step("s1", "persist first step", "assistant", "first-step"),
            tool_step("s2", "retry unstable tool", "tool/unstable", "resume-ok"),
        ],
    );
    spec.capability_leases.push(lease("tool/unstable"));

    let failing_kernel = Kernel::with_services(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(failing_registry),
        LocalSubRunTransport,
        None,
        Arc::new(store.clone()),
    );
    let first_result = failing_kernel.run(&spec);
    let checkpoint = store
        .get(&spec.run_id)
        .expect("checkpoint store readable")
        .expect("checkpoint exists");

    let mut recovered_registry = CapabilityRegistry::new();
    recovered_registry.register(
        "tool/unstable",
        StaticCapability::new(|input, _| {
            Ok(EffectProposal {
                summary: "unstable_recovered".to_string(),
                messages: Vec::new(),
                outputs: vec![input.to_string()],
            })
        }),
    );
    let recovered_kernel = Kernel::with_services(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(recovered_registry),
        LocalSubRunTransport,
        None,
        Arc::new(store),
    );
    let resumed = recovered_kernel
        .resume(&spec)
        .expect("checkpoint should resume");
    let resumed_at_second_step = checkpoint.state.next_step_index == 1
        && !resumed.events.iter().any(|event| {
            event.step_id.as_deref() == Some("s1")
                && matches!(event.kind, axiom_spec::EventKind::StepStarted)
        });
    let completed = resumed.state.status == axiom_spec::RunStatus::Completed
        && resumed.state.messages.len() == 1
        && resumed.state.outputs == vec!["resume-ok"];

    CaseResult {
        case_id: "runstore_checkpoint_resume".to_string(),
        category: "recovery".to_string(),
        passed: first_result.is_err() && resumed_at_second_step && completed,
        summary: "A failed run resumes from its last committed checkpoint".to_string(),
        metrics: vec![
            Metric {
                name: "checkpoint_step".to_string(),
                value: checkpoint.state.next_step_index.to_string(),
            },
            Metric {
                name: "first_step_replayed".to_string(),
                value: (!resumed_at_second_step).to_string(),
            },
            Metric {
                name: "resume_completed".to_string(),
                value: completed.to_string(),
            },
        ],
        evidence: vec![
            format!("first_result={first_result:?}"),
            format!("resumed_outputs={:?}", resumed.state.outputs),
        ],
    }
}

#[derive(Clone)]
struct FailCommitCheckpointOnce {
    inner: Arc<dyn RunStore>,
    failed: Arc<AtomicBool>,
}

impl FailCommitCheckpointOnce {
    fn new(inner: Arc<dyn RunStore>) -> Self {
        Self {
            inner,
            failed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl RunStore for FailCommitCheckpointOnce {
    fn put(&self, record: RunStoreRecord) -> Result<(), String> {
        if record.state.next_step_index == 1 && !self.failed.swap(true, Ordering::SeqCst) {
            return Err("simulated_crash_after_effect_commit".to_string());
        }
        self.inner.put(record)
    }

    fn get(&self, run_id: &str) -> Result<Option<RunStoreRecord>, String> {
        self.inner.get(run_id)
    }

    fn list_run_ids(&self) -> Result<Vec<String>, String> {
        self.inner.list_run_ids()
    }
}

fn journal_checkpoint_crash_recovery() -> CaseResult {
    let checkpoint_root = PathBuf::from("reports/checkpoints/journal-crash-window");
    let _ = fs::remove_dir_all(&checkpoint_root);
    let journal_path = temp_event_path("journal-checkpoint-crash-recovery");
    let durable_store = Arc::new(FileRunStore::new(&checkpoint_root));
    let fail_once_store = Arc::new(FailCommitCheckpointOnce::new(durable_store));
    let spec = RunSpec::new(
        "journal-crash-window",
        "journal crash window",
        vec![msg_step("s1", "commit once", "assistant", "exactly-once")],
    );
    let crashing_kernel = Kernel::with_services(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        LocalSubRunTransport,
        Some(Arc::new(JsonlEventLog::new(&journal_path))),
        fail_once_store,
    );
    let crash_result = crashing_kernel.run(&spec);
    drop(crashing_kernel);

    let restarted_store = Arc::new(FileRunStore::new(&checkpoint_root));
    let restarted_kernel = Kernel::with_services(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        LocalSubRunTransport,
        Some(Arc::new(JsonlEventLog::new(&journal_path))),
        restarted_store.clone(),
    );
    let recovered = restarted_kernel
        .resume(&spec)
        .expect("journal should close checkpoint crash window");
    let recovered_again = restarted_kernel
        .resume(&spec)
        .expect("second resume must be idempotent");
    let durable_checkpoint = restarted_store
        .get(&spec.run_id)
        .expect("durable checkpoint readable")
        .expect("durable checkpoint exists");
    let exactly_once = recovered.state.messages.len() == 1
        && recovered.state.messages[0].content == "exactly-once"
        && recovered_again.state.messages.len() == 1
        && durable_checkpoint.applied_commit_ids.len() == 1;
    let sequence_continued = durable_checkpoint.last_sequence >= 10;
    let persisted = FileRunStore::new(&checkpoint_root)
        .list_run_ids()
        .expect("file store list readable")
        == vec![spec.run_id.clone()];

    CaseResult {
        case_id: "journal_checkpoint_crash_recovery".to_string(),
        category: "recovery".to_string(),
        passed: crash_result.is_err() && exactly_once && sequence_continued && persisted,
        summary: "Journal replay closes the committed-event/checkpoint crash window exactly once"
            .to_string(),
        metrics: vec![
            Metric {
                name: "exactly_once".to_string(),
                value: exactly_once.to_string(),
            },
            Metric {
                name: "last_sequence".to_string(),
                value: durable_checkpoint.last_sequence.to_string(),
            },
            Metric {
                name: "durable_restart".to_string(),
                value: persisted.to_string(),
            },
        ],
        evidence: vec![
            format!("crash_result={crash_result:?}"),
            format!("checkpoint_root={}", checkpoint_root.display()),
            format!("commit_ids={:?}", durable_checkpoint.applied_commit_ids),
        ],
    }
}

fn writer_lease_epoch_fencing() -> CaseResult {
    let lease_root = PathBuf::from("reports/checkpoints/writer-lease-fencing");
    let _ = fs::remove_dir_all(&lease_root);
    let lease_store = FileRunLeaseStore::new(&lease_root);
    let writer_a = lease_store
        .acquire("fenced-run", "writer-a", 100, 10)
        .expect("writer A acquires epoch 1");
    let contention = lease_store.acquire("fenced-run", "writer-b", 105, 10);
    let writer_b = lease_store
        .acquire("fenced-run", "writer-b", 111, 10)
        .expect("writer B takes over expired lease");
    let stale_writer = lease_store.validate(&writer_a, 111);
    let current_writer = lease_store.validate(&writer_b, 111);
    let renewed = lease_store
        .renew(&writer_b, 112, 10)
        .expect("current writer renews same epoch");
    let blocked_while_active = contention.is_err();
    let epoch_advanced = writer_a.epoch == 1 && writer_b.epoch == 2 && renewed.epoch == 2;
    let stale_fenced = format!("{stale_writer:?}").contains("writer_fenced");
    let current_valid = current_writer.is_ok() && renewed.expires_at_ms == 122;
    lease_store
        .release(&renewed)
        .expect("synthetic lease releases");
    let writer_c = lease_store
        .acquire("fenced-run", "writer-c", 113, 10)
        .expect("released lease retains epoch tombstone");
    let release_preserved_epoch = writer_c.epoch == 3;
    lease_store
        .release(&writer_c)
        .expect("writer C releases tombstone lease");

    let live_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis() as u64;
    let blocker = lease_store
        .acquire("kernel-fenced-run", "external-writer", live_now, 60_000)
        .expect("external writer acquires live lease");
    let coordinated_kernel = Kernel::with_coordination(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        LocalSubRunTransport,
        None,
        Arc::new(MemoryRunStore::new()),
        Arc::new(lease_store.clone()),
        "kernel-writer",
        30_000,
    );
    let blocked_kernel = coordinated_kernel.run(&RunSpec::new(
        "kernel-fenced-run",
        "kernel fenced run",
        Vec::new(),
    ));
    lease_store
        .release(&blocker)
        .expect("external writer releases live lease");
    let admitted_kernel = coordinated_kernel.run(&RunSpec::new(
        "kernel-fenced-run",
        "kernel fenced run",
        Vec::new(),
    ));
    let kernel_fenced =
        matches!(blocked_kernel, Err(axiom_core::KernelError::Lease(_))) && admitted_kernel.is_ok();

    CaseResult {
        case_id: "writer_lease_epoch_fencing".to_string(),
        category: "coordination".to_string(),
        passed: blocked_while_active
            && epoch_advanced
            && stale_fenced
            && current_valid
            && release_preserved_epoch
            && kernel_fenced,
        summary: "Expired writer leases advance epoch and fence every stale writer".to_string(),
        metrics: vec![
            Metric {
                name: "active_contention_blocked".to_string(),
                value: blocked_while_active.to_string(),
            },
            Metric {
                name: "epoch_advanced".to_string(),
                value: epoch_advanced.to_string(),
            },
            Metric {
                name: "stale_writer_fenced".to_string(),
                value: stale_fenced.to_string(),
            },
            Metric {
                name: "kernel_fenced_until_release".to_string(),
                value: kernel_fenced.to_string(),
            },
            Metric {
                name: "release_preserved_epoch".to_string(),
                value: release_preserved_epoch.to_string(),
            },
        ],
        evidence: vec![
            format!("writer_a={writer_a:?}"),
            format!("writer_b={writer_b:?}"),
            format!("stale_validation={stale_writer:?}"),
        ],
    }
}

fn schema_migration_compatibility_matrix() -> CaseResult {
    let event_context = EventMigrationContext {
        spec_digest: "a".repeat(64),
        writer_epoch: 7,
    };
    let legacy_event_json =
        fs::read_to_string("fixtures/migration/event-v0.json").expect("legacy event fixture");
    let legacy_event = migrate_event_json(&legacy_event_json, &event_context, 3)
        .expect("legacy event should migrate");
    let legacy_event_safe = matches!(
        legacy_event.status,
        MigrationStatus::Migrated { from_version: 0 }
    ) && legacy_event.value.schema_version == 1
        && legacy_event.value.sequence == 3
        && legacy_event.value.writer_epoch == 7
        && matches!(legacy_event.value.kind, EventKind::EffectCommitted)
        && legacy_event.value.effect.is_none()
        && !legacy_event.warnings.is_empty();

    let mut current_event = Event::new("migration-run", None, EventKind::RunStarted, "current");
    current_event.sequence = 1;
    current_event.timestamp_ms = 1;
    current_event.spec_digest = event_context.spec_digest.clone();
    current_event.writer_epoch = 7;
    let current_event_json =
        serde_json::to_string(&current_event).expect("serialize current event");
    let current_event_result = migrate_event_json(&current_event_json, &event_context, 99)
        .expect("current event should remain current");
    let current_event_unchanged = matches!(current_event_result.status, MigrationStatus::Current)
        && current_event_result.value == current_event;
    let future_event_rejected = migrate_event_json(
        r#"{"schema_version":2,"run_id":"future"}"#,
        &event_context,
        1,
    )
    .is_err();
    let unknown_legacy_rejected = migrate_event_json(
        r#"{"run_id":"legacy","step_id":null,"kind":"UnknownKind","detail":"x"}"#,
        &event_context,
        1,
    )
    .is_err();

    let checkpoint_context = CheckpointMigrationContext {
        spec_digest: "b".repeat(64),
        last_sequence: 8,
        writer_epoch: 4,
    };
    let legacy_checkpoint_json = fs::read_to_string("fixtures/migration/checkpoint-v0.json")
        .expect("legacy checkpoint fixture");
    let legacy_checkpoint = migrate_checkpoint_json(&legacy_checkpoint_json, &checkpoint_context)
        .expect("legacy checkpoint should migrate");
    let legacy_checkpoint_safe = matches!(
        legacy_checkpoint.status,
        MigrationStatus::Migrated { from_version: 0 }
    ) && legacy_checkpoint.value.checkpoint_version == 1
        && legacy_checkpoint.value.spec_digest == checkpoint_context.spec_digest
        && legacy_checkpoint.value.last_sequence == 8
        && !legacy_checkpoint.warnings.is_empty();
    let current_checkpoint = RunStoreRecord {
        checkpoint_version: 1,
        run_id: "migration-run".to_string(),
        spec_digest: checkpoint_context.spec_digest.clone(),
        last_sequence: 8,
        writer_epoch: 4,
        applied_commit_ids: BTreeSet::new(),
        state: RunState::from_spec(&RunSpec::new("migration-run", "migration", Vec::new())),
    };
    let current_checkpoint_json =
        serde_json::to_string(&current_checkpoint).expect("serialize current checkpoint");
    let current_checkpoint_unchanged = matches!(
        migrate_checkpoint_json(&current_checkpoint_json, &checkpoint_context)
            .expect("current checkpoint should remain current")
            .status,
        MigrationStatus::Current
    );
    let future_checkpoint_rejected = migrate_checkpoint_json(
        r#"{"checkpoint_version":2,"run_id":"future"}"#,
        &checkpoint_context,
    )
    .is_err();
    let migrated_checkpoint_path = temp_generated_path("migrated-checkpoint-v1.json");
    let checkpoint_file_migrated = matches!(
        migrate_checkpoint_file(
            "fixtures/migration/checkpoint-v0.json",
            &migrated_checkpoint_path,
            &checkpoint_context,
        )
        .expect("checkpoint file migration should run")
        .status,
        MigrationStatus::Migrated { from_version: 0 }
    ) && migrated_checkpoint_path.exists();

    let legacy_log_path = temp_generated_path("legacy-eventlog-v0.jsonl");
    let migrated_log_path = temp_generated_path("migrated-eventlog-v1.jsonl");
    fs::write(&legacy_log_path, &legacy_event_json).expect("write legacy event log");
    let migration_report = migrate_event_log(&legacy_log_path, &migrated_log_path, &event_context)
        .expect("event log migration should run");
    let batch_migration_valid = migration_report.total_records == 1
        && migration_report.migrated_records == 1
        && migration_report.warnings.len() == 1;
    let runspec_matrix = validate_run_spec_version(1).is_ok()
        && validate_run_spec_version(0).is_err()
        && validate_run_spec_version(2).is_err();

    let passed = legacy_event_safe
        && current_event_unchanged
        && future_event_rejected
        && unknown_legacy_rejected
        && legacy_checkpoint_safe
        && current_checkpoint_unchanged
        && future_checkpoint_rejected
        && checkpoint_file_migrated
        && batch_migration_valid
        && runspec_matrix;

    CaseResult {
        case_id: "schema_migration_compatibility_matrix".to_string(),
        category: "migration".to_string(),
        passed,
        summary: "Schema migration upgrades v0, preserves v1, and rejects unknown future versions"
            .to_string(),
        metrics: vec![
            Metric {
                name: "legacy_event_safe".to_string(),
                value: legacy_event_safe.to_string(),
            },
            Metric {
                name: "legacy_checkpoint_safe".to_string(),
                value: legacy_checkpoint_safe.to_string(),
            },
            Metric {
                name: "future_versions_rejected".to_string(),
                value: (future_event_rejected && future_checkpoint_rejected).to_string(),
            },
            Metric {
                name: "batch_migration_valid".to_string(),
                value: (batch_migration_valid && checkpoint_file_migrated).to_string(),
            },
        ],
        evidence: legacy_event
            .warnings
            .into_iter()
            .chain(legacy_checkpoint.warnings)
            .collect(),
    }
}

fn journal_maintenance_and_snapshot_retention() -> CaseResult {
    let source_path = temp_event_path("journal-maintenance-source");
    let repaired_path = temp_generated_path("journal-maintenance-repaired.jsonl");
    let quarantine_path = temp_generated_path("journal-maintenance-quarantine.txt");
    let compacted_path = temp_generated_path("journal-maintenance-compacted.jsonl");
    let spec = RunSpec::new(
        "journal-maintenance",
        "journal maintenance",
        vec![
            msg_step("s1", "first committed message", "assistant", "first"),
            msg_step("s2", "second committed message", "assistant", "second"),
        ],
    );
    let kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        Some(JsonlEventLog::new(&source_path)),
    );
    let report = kernel.run(&spec).expect("maintenance source run");
    let clean_before_corruption = JsonlEventLog::new(&source_path)
        .scan_integrity()
        .expect("source integrity scan")
        .is_clean();
    let mut source = OpenOptions::new()
        .append(true)
        .open(&source_path)
        .expect("open source journal for corruption fixture");
    source
        .write_all(b"{corrupt-journal-line\n")
        .expect("append corrupt fixture line");
    source.flush().expect("flush corrupt fixture line");

    let corrupted_log = JsonlEventLog::new(&source_path);
    let corrupted_scan = corrupted_log
        .scan_integrity()
        .expect("corrupted journal scan");
    let compaction_blocked = corrupted_log
        .compact_to(&compacted_path, BTreeMap::new())
        .is_err();
    let repair = corrupted_log
        .repair_to(&repaired_path, &quarantine_path)
        .expect("repair should quarantine malformed line");
    let repaired_log = JsonlEventLog::new(&repaired_path);
    let repaired_clean = repaired_log
        .scan_integrity()
        .expect("repaired journal scan")
        .is_clean();
    let boundaries = BTreeMap::from([(spec.run_id.clone(), 8_u64)]);
    let compaction = repaired_log
        .compact_to(&compacted_path, boundaries)
        .expect("clean journal compaction");
    let compacted_tail = JsonlEventLog::new(&compacted_path)
        .load_after(&spec.run_id, 8)
        .expect("compacted journal tail readable");
    let compaction_valid = compaction.source_events == report.events.len()
        && compaction.dropped_events == 8
        && compacted_tail.first().map(|event| event.sequence) == Some(9)
        && compacted_tail.len() == compaction.retained_events;

    let snapshot_root = PathBuf::from("reports/checkpoints/snapshot-retention");
    let _ = fs::remove_dir_all(&snapshot_root);
    let archive = FileSnapshotArchive::new(&snapshot_root);
    for sequence in [8_u64, 15, 20] {
        archive
            .archive(&RunStoreRecord {
                checkpoint_version: 1,
                run_id: spec.run_id.clone(),
                spec_digest: spec.digest(),
                last_sequence: sequence,
                writer_epoch: 1,
                applied_commit_ids: BTreeSet::new(),
                state: report.state.clone(),
            })
            .expect("archive checkpoint snapshot");
    }
    let retention = archive
        .retain_latest(&spec.run_id, 2)
        .expect("snapshot retention");
    let retention_valid = retention.deleted_sequences == vec![8]
        && retention.retained_sequences == vec![15, 20]
        && archive
            .load(&spec.run_id, 8)
            .expect("deleted snapshot lookup")
            .is_none()
        && archive
            .load(&spec.run_id, 20)
            .expect("retained snapshot lookup")
            .is_some();

    let passed = clean_before_corruption
        && corrupted_scan.corrupt_lines.len() == 1
        && compaction_blocked
        && repair.quarantined_lines.len() == 1
        && repaired_clean
        && compaction_valid
        && retention_valid;
    CaseResult {
        case_id: "journal_maintenance_and_snapshot_retention".to_string(),
        category: "maintenance".to_string(),
        passed,
        summary: "Journal repair preserves evidence, compaction follows snapshots, and retention prunes safely"
            .to_string(),
        metrics: vec![
            Metric {
                name: "corrupt_lines_quarantined".to_string(),
                value: repair.quarantined_lines.len().to_string(),
            },
            Metric {
                name: "events_compacted".to_string(),
                value: compaction.dropped_events.to_string(),
            },
            Metric {
                name: "snapshots_retained".to_string(),
                value: retention.retained_sequences.len().to_string(),
            },
        ],
        evidence: vec![
            format!("quarantine={}", quarantine_path.display()),
            format!("compacted={}", compacted_path.display()),
            format!("retention={retention:?}"),
        ],
    }
}

fn toy_tool_agent() -> CaseResult {
    let scheduler = ReActScheduler::new(MockModelDriver::scripted([
        ModelDecision::Invoke {
            capability_id: "tool/echo".to_string(),
            input: "hello from react".to_string(),
        },
        ModelDecision::Respond {
            content: "The tool returned hello from react.".to_string(),
        },
        ModelDecision::Finish,
    ]));
    let kernel = Kernel::new(
        scheduler,
        AuditShell,
        LocalTransport::new(base_registry()),
        None,
    );
    let mut spec = RunSpec::new(
        "toy-tool-agent",
        "toy ReAct tool agent",
        vec![msg_step(
            "user-prompt",
            "user prompt",
            "user",
            "Echo hello from react and explain the result.",
        )],
    );
    spec.capability_leases.push(lease("tool/echo"));

    let report = kernel.run(&spec).expect("toy ReAct agent should run");
    let tool_committed = report.events.iter().any(|event| {
        event.kind == EventKind::EffectCommitted && event.step_id.as_deref() == Some("react-2-tool")
    });
    let response_committed = report.events.iter().any(|event| {
        event.kind == EventKind::EffectCommitted
            && event.step_id.as_deref() == Some("react-3-response")
    });
    let passed = tool_committed
        && response_committed
        && report.state.outputs == vec!["hello from react"]
        && report.state.messages.last().is_some_and(|message| {
            message.role == "assistant" && message.content.contains("tool returned")
        });

    CaseResult {
        case_id: "toy_tool_agent".to_string(),
        category: "agent".to_string(),
        passed,
        summary: "ReAct scheduler alternates model decisions, tool effects, and final response"
            .to_string(),
        metrics: vec![Metric {
            name: "react_turns".to_string(),
            value: (report.state.next_step_index - spec.steps.len()).to_string(),
        }],
        evidence: report
            .events
            .iter()
            .filter(|event| event.kind == EventKind::StepProposed)
            .map(|event| {
                format!(
                    "{}:{}",
                    event.step_id.as_deref().unwrap_or("run"),
                    event.detail
                )
            })
            .collect(),
    }
}

fn native_driver_contracts() -> CaseResult {
    let root = temp_generated_path("native-driver-sandbox");
    let _ = fs::remove_dir_all(&root);
    let mut registry = CapabilityRegistry::new();
    registry.register("driver/cli", CliDriver::new("/bin/echo", Vec::new()));
    registry.register(
        "driver/filesystem",
        FilesystemDriver::new(&root).expect("filesystem driver root"),
    );
    let kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(registry),
        None,
    );
    let mut spec = RunSpec::new(
        "native-driver-contracts",
        "native driver contracts",
        vec![
            tool_step("cli", "CLI without shell", "driver/cli", "safe;not-shell"),
            tool_step(
                "write",
                "sandboxed write",
                "driver/filesystem",
                r#"{"operation":"write","path":"nested/result.txt","content":"sandboxed"}"#,
            ),
            tool_step(
                "read",
                "sandboxed read",
                "driver/filesystem",
                r#"{"operation":"read","path":"nested/result.txt"}"#,
            ),
        ],
    );
    spec.capability_leases.push(lease("driver/cli"));
    spec.capability_leases.push(lease("driver/filesystem"));
    let report = kernel.run(&spec).expect("native drivers should run");

    let mut denied_registry = CapabilityRegistry::new();
    denied_registry.register(
        "driver/filesystem",
        FilesystemDriver::new(&root).expect("filesystem driver root"),
    );
    let denied_kernel = Kernel::new(
        QueueScheduler,
        AuditShell,
        LocalTransport::new(denied_registry),
        None,
    );
    let mut denied_spec = RunSpec::new(
        "native-driver-traversal",
        "native driver traversal",
        vec![tool_step(
            "escape",
            "deny traversal",
            "driver/filesystem",
            r#"{"operation":"read","path":"../secret"}"#,
        )],
    );
    denied_spec
        .capability_leases
        .push(lease("driver/filesystem"));
    let traversal_denied = matches!(
        denied_kernel.run(&denied_spec),
        Err(axiom_core::KernelError::Capability(detail)) if detail.contains("filesystem_path_denied")
    );
    let passed = report
        .state
        .outputs
        .first()
        .is_some_and(|value| value == "safe;not-shell")
        && report
            .state
            .outputs
            .last()
            .is_some_and(|value| value == "sandboxed")
        && traversal_denied;

    CaseResult {
        case_id: "native_driver_contracts".to_string(),
        category: "driver".to_string(),
        passed,
        summary: "CLI avoids shell interpolation and filesystem access stays inside its root"
            .to_string(),
        metrics: vec![Metric {
            name: "traversal_denied".to_string(),
            value: traversal_denied.to_string(),
        }],
        evidence: report.state.outputs,
    }
}

#[derive(Clone)]
struct RecoveryModel {
    calls: Arc<AtomicUsize>,
}

impl ModelDriver for RecoveryModel {
    fn decide(&self, _spec: &RunSpec, _state: &RunState) -> Result<ModelDecision, String> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ModelDecision::Invoke {
                capability_id: "tool/echo".to_string(),
                input: "durable proposal".to_string(),
            }),
            1 => Ok(ModelDecision::Finish),
            _ => Err("model_called_more_than_expected".to_string()),
        }
    }
}

#[derive(Clone)]
struct FailPendingCheckpointOnce {
    inner: MemoryRunStore,
    failed: Arc<AtomicBool>,
}

impl RunStore for FailPendingCheckpointOnce {
    fn put(&self, record: RunStoreRecord) -> Result<(), String> {
        if record.state.pending_step.is_some() && !self.failed.swap(true, Ordering::SeqCst) {
            return Err("injected_pending_checkpoint_failure".to_string());
        }
        self.inner.put(record)
    }

    fn get(&self, run_id: &str) -> Result<Option<RunStoreRecord>, String> {
        self.inner.get(run_id)
    }

    fn list_run_ids(&self) -> Result<Vec<String>, String> {
        self.inner.list_run_ids()
    }
}

fn durable_scheduler_proposal_recovery() -> CaseResult {
    let event_path = temp_event_path("durable-scheduler-proposal");
    let journal = Arc::new(JsonlEventLog::new(&event_path));
    let store = Arc::new(FailPendingCheckpointOnce {
        inner: MemoryRunStore::new(),
        failed: Arc::new(AtomicBool::new(false)),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let spec = {
        let mut spec = RunSpec::new(
            "durable-scheduler-proposal",
            "durable scheduler proposal",
            Vec::new(),
        );
        spec.capability_leases.push(lease("tool/echo"));
        spec
    };

    let first = Kernel::with_services(
        ReActScheduler::new(RecoveryModel {
            calls: Arc::clone(&calls),
        }),
        AuditShell,
        LocalTransport::new(base_registry()),
        LocalSubRunTransport,
        Some(journal.clone()),
        store.clone(),
    );
    let failed_at_checkpoint = matches!(
        first.run(&spec),
        Err(axiom_core::KernelError::RunStore(detail))
            if detail == "injected_pending_checkpoint_failure"
    );

    let resumed = Kernel::with_services(
        ReActScheduler::new(RecoveryModel {
            calls: Arc::clone(&calls),
        }),
        AuditShell,
        LocalTransport::new(base_registry()),
        LocalSubRunTransport,
        Some(journal),
        store,
    )
    .resume(&spec)
    .expect("pending scheduler proposal should recover");
    let model_calls = calls.load(Ordering::SeqCst);
    let passed = failed_at_checkpoint
        && resumed.state.outputs == vec!["durable proposal"]
        && resumed.state.pending_step.is_none()
        && model_calls == 2;

    CaseResult {
        case_id: "durable_scheduler_proposal_recovery".to_string(),
        category: "recovery".to_string(),
        passed,
        summary: "Journaled scheduler proposals survive checkpoint failure without skipping tool execution"
            .to_string(),
        metrics: vec![Metric {
            name: "model_calls".to_string(),
            value: model_calls.to_string(),
        }],
        evidence: vec![
            format!("failed_at_checkpoint={failed_at_checkpoint}"),
            format!("outputs={:?}", resumed.state.outputs),
            format!("journal={}", event_path.display()),
        ],
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

fn sdk_spec_digest_conformance() -> CaseResult {
    let rust_digest = coding_patch_small_spec().digest();
    let ts_path = temp_generated_path("digest-ts-coding-patch-small.json");
    let py_path = temp_generated_path("digest-py-coding-patch-small.json");
    let ts_output = Command::new("node")
        .arg("../axiom_kernal/sdks/typescript/scripts/build-coding-patch-small.mjs")
        .output()
        .expect("TypeScript digest fixture generator should run");
    let py_output = Command::new("python3")
        .arg("../axiom_kernal/sdks/python/scripts/build_coding_patch_small.py")
        .output()
        .expect("Python digest fixture generator should run");
    fs::write(&ts_path, ts_output.stdout).expect("write TypeScript digest fixture");
    fs::write(&py_path, py_output.stdout).expect("write Python digest fixture");
    let ts_digest = runspec_digest(&ts_path);
    let py_digest = runspec_digest(&py_path);
    let passed = ts_digest == rust_digest && py_digest == rust_digest;

    CaseResult {
        case_id: "sdk_spec_digest_conformance".to_string(),
        category: "sdk".to_string(),
        passed,
        summary: "Rust, TypeScript, and Python produce the same canonical RunSpec digest"
            .to_string(),
        metrics: vec![
            Metric {
                name: "typescript_match".to_string(),
                value: (ts_digest == rust_digest).to_string(),
            },
            Metric {
                name: "python_match".to_string(),
                value: (py_digest == rust_digest).to_string(),
            },
        ],
        evidence: vec![
            format!("rust={rust_digest}"),
            format!("typescript={ts_digest}"),
            format!("python={py_digest}"),
        ],
    }
}

fn runspec_digest(path: &PathBuf) -> String {
    let output = Command::new("node")
        .arg("runners/digest-runspec.mjs")
        .arg(path)
        .output()
        .expect("RunSpec digest runner should execute");
    String::from_utf8(output.stdout)
        .expect("RunSpec digest should be utf8")
        .trim()
        .to_string()
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
                    | axiom_spec::EventKind::EffectProposed
                    | axiom_spec::EventKind::EffectCommitted
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
