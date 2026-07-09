# coding_patch_small

## Goal

用 Axiom 实现一个最小 coding agent，完成小型补丁任务，验证：

- ShellDecision 审批链
- patch tool 审计完整性
- child reviewer agent 合并语义
- 任务成功率与回放稳定性

## Planned Metrics

- `task_success`
- `patch_apply_success`
- `audit_coverage`
- `reviewer_merge_quality`
- `normalized_step_count`

## Baseline

- native direct-run coding loop
- wrap mode coding loop
- 后续接入 SWE-bench 风格小样本任务
