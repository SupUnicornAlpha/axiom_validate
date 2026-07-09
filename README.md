# axiom_validate

`axiom_validate` 是 Axiom / Agent Atom 的验证项目。它不是普通 examples 目录，而是用来持续验证框架正确性、Agent 效果和企业治理收益的回归系统。

## 职责

- 用 `axiom_kernal` 实现多个 Agent。
- 验证 Kernel replay、checkpoint、ShellDecision、ChildRun、安全隔离。
- 做原框架直跑 vs Axiom wrap 的对照实验。
- 做 LocalTransport vs RemoteTransport mock 的不变性测试。
- 为 TypeScript、Python、Go、Java SDK 提供 conformance golden cases。

## MVP cases

| Case | 目标 |
|------|------|
| `kernel_replay_basic` | EventLog replay 到同一 State |
| `shell_decision_allow_rewrite_deny` | Shell 不能直接产出 Effect |
| `tool_syscall_audit` | 每次工具调用都有 Event |
| `childrun_capability_lease_denied` | 子 Agent 未授权 capability 被拒绝 |
| `childrun_merge_gate` | child result 由父 Gate 合并 |
| `coding_patch_small` | 验证真实 coding agent 任务 |
| `wrap_vs_native_audit` | 验证 wrap 模式治理收益 |
| `local_remote_invariance` | 验证 Local/Remote 语义一致 |
| `sdk_conformance_runspec` | 验证不同 SDK 生成同一 RunSpec |

## 建议目录

```text
axiom_validate/
  cases/
    kernel/
    shell/
    childrun/
    agents/
    wrap/
    transport/
    sdk/
  fixtures/
    runspec/
    eventlog/
    workspaces/
  reports/
  runners/
  metrics/
```

详细规划见 `../docs/09-dual-track-development-validation.md`。
