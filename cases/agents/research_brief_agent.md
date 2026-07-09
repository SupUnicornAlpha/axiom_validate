# research_brief_agent

## Goal

用 Axiom 实现一个 research brief agent，验证：

- 多 step 研究流程是否能稳定 replay
- 外部检索 / summarization tool 是否具备统一审计
- LoopSpec 风格目标推进是否可追踪

## Planned Metrics

- `brief_completeness`
- `citation_coverage`
- `replay_determinism`
- `budget_adherence`

## Baseline

- native research script
- Axiom wrap mode
- 后续接入 GAIA 风格信息获取与综合任务
