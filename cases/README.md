# Cases

这一轮先用最小可执行 case 验证三件事：

1. `Run/Step/Event` replay 是否稳定。
2. `ShellDecision` 是否始终经由内核审计链。
3. `ChildRun + CapabilityLease` 是否具备最小越权拦截与合并语义。

后续会继续扩展：

- `agents/coding_patch_small`
- `agents/research_brief_agent`
- `wrap/wrap_vs_native_audit`
- `transport/local_remote_invariance`
- `sdk/sdk_conformance_runspec`
- 对接开源评测基线（SWE-bench 风格、GAIA 风格、trace judge 风格）
