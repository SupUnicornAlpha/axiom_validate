# Axiom Coding Agent Validation

The coding agent, prompts, planner/`decide`, and tools are implemented in Go under
`agents/coding-go`. Rust is only the Axiom runtime protocol bridge.

See [`agents/coding-go/README.md`](../coding-go/README.md) for DeepSeek live mode.

| Capability | Status | Axiom implementation |
|---|---|---|
| Read/list/grep | Implemented in Go | Workspace-scoped tools |
| Exact edit/write | Implemented in Go | Single-match edit and bounded write |
| Bash | Implemented in Go | Explicit argv allowlist, no shell interpolation |
| ReAct tool loop | Implemented | `ReActScheduler` + scripted `plan` or live `decide` |
| DeepSeek live model | Implemented in Go | `DEEPSEEK_API_KEY` + `cargo run -- try-coding` |
| Permissions/audit | Implemented | Capability leases, Shell policy, EventLog |
| Subagents | Kernel-supported | `ChildRun` sandbox and gate merge |
| MCP/custom tools | Kernel-supported boundary | Capability transport/registry adapters |
| Todo/session UI/LSP | Not yet parity | Future validation agents and SDK surfaces |
| Model quality parity | Not claimed | Offline plan is deterministic; live quality depends on model |

## Modes

1. **Offline regression** (`coding_agent_opencode_parity`): Go `plan` returns a fixed script;
   Kernel executes tools via Go CLI. No API key needed.
2. **Live try** (`cargo run -- try-coding`): each ReAct turn calls Go `decide`, which queries
   DeepSeek and returns one decision. Requires `DEEPSEEK_API_KEY`.

The default live fixture is a Rust workspace with a failing `add` test. The agent should
list/read/search, apply an exact edit, run tests, and emit an audited final response.
