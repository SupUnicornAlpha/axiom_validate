#[derive(Clone, Debug)]
pub struct Metric {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct CaseResult {
    pub case_id: String,
    pub category: String,
    pub passed: bool,
    pub summary: String,
    pub metrics: Vec<Metric>,
    pub evidence: Vec<String>,
}

pub fn render_markdown_report(results: &[CaseResult]) -> String {
    let mut out = String::new();
    out.push_str("# Axiom Validate Report\n\n");
    out.push_str("| Case | Category | Status | Summary |\n");
    out.push_str("|------|----------|--------|---------|\n");
    for result in results {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            result.case_id,
            result.category,
            if result.passed { "PASS" } else { "FAIL" },
            result.summary.replace('|', "/"),
        ));
    }
    out.push_str("\n## Details\n\n");
    for result in results {
        out.push_str(&format!("### {}\n\n", result.case_id));
        out.push_str(&format!("- Category: `{}`\n", result.category));
        out.push_str(&format!(
            "- Status: `{}`\n",
            if result.passed { "PASS" } else { "FAIL" }
        ));
        out.push_str(&format!("- Summary: {}\n", result.summary));
        if !result.metrics.is_empty() {
            out.push_str("- Metrics:\n");
            for metric in &result.metrics {
                out.push_str(&format!("  - `{}` = `{}`\n", metric.name, metric.value));
            }
        }
        if !result.evidence.is_empty() {
            out.push_str("- Evidence:\n");
            for evidence in &result.evidence {
                out.push_str(&format!("  - {}\n", evidence));
            }
        }
        out.push('\n');
    }
    out.push_str("## Evaluation Methods\n\n");
    out.push_str("- `replay_determinism`: 对齐开源 trace replay / event sourcing 验证思路。\n");
    out.push_str(
        "- `effect_commit_boundary`: 验证 driver proposal 与 Kernel commit 的唯一状态提交边界。\n",
    );
    out.push_str("- `checkpoint_resume`: 验证失败后从最后成功 step 恢复且不重复已提交 effect。\n");
    out.push_str("- `journal_checkpoint_crash_recovery`: 验证 committed event 已落盘但 checkpoint 未落盘时，重启后幂等恢复。\n");
    out.push_str("- `writer_lease_epoch_fencing`: 验证活跃 writer 排他、过期接管递增 epoch、旧 writer 永久失效。\n");
    out.push_str("- `schema_migration_compatibility_matrix`: 验证 v0 迁移、v1 保持、未来版本与未知类型拒绝策略。\n");
    out.push_str("- `journal_maintenance_and_snapshot_retention`: 验证完整性扫描、旁路修复、checkpoint-aware compaction 与 snapshot retention。\n");
    out.push_str("- `toy_tool_agent`: 验证 ReAct 的 model decision、tool invocation、observation 与 response 闭环。\n");
    out.push_str("- `native_driver_contracts`: 验证 CLI 无 shell interpolation 且 filesystem driver 阻止目录逃逸。\n");
    out.push_str("- `durable_scheduler_proposal_recovery`: 验证动态提案写 journal 后即使 checkpoint 失败，恢复也复用原提案。\n");
    out.push_str("- `audit_coverage`: 对齐工具调用审计覆盖率与治理可见性验证。\n");
    out.push_str("- `permission_denial_rate`: 对齐 sandbox / lease 越权拦截验证。\n");
    out.push_str("- `merge_semantics`: 对齐 multi-agent / subagent 合并语义验证。\n");
    out.push_str("- `local_remote_invariance`: 对齐本地/远程执行语义一致性验证。\n");
    out.push_str(
        "- `subrun_transport_invariance`: 对齐 ChildRun 本地/远程部署边界的语义一致性验证。\n",
    );
    out.push_str("- `sdk_conformance_runspec`: 对齐多语言 SDK 生成同一 RunSpec 的一致性验证。\n");
    out.push_str("- `sdk_spec_digest_conformance`: 验证 Rust/TypeScript/Python 对 canonical RunSpec 生成相同 SHA-256。\n");
    out.push_str("- `brief_completeness`: 对齐 research agent 输出完整性验证。\n");
    out.push_str("- `wrap_audit_gain`: 对齐 wrap 模式相对 native 执行的审计覆盖收益验证。\n");
    out.push_str("- `coding_agent_opencode_parity`: 验证 workspace-scoped read/search/edit/bash 与 ReAct 修复闭环，不宣称模型质量等价。\n");
    out.push_str("- `golden_eventlog_match`: 对齐 golden EventLog 与 validator 的一致性验证。\n");
    out.push_str(
        "- `task_success`: 作为后续接入开源基准（如 SWE-bench 风格、GAIA 风格）的占位接口。\n",
    );
    out
}
