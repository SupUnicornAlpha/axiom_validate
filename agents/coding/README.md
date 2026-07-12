# Axiom Coding Agent Validation

The coding agent, prompts, planner, and tools are implemented in Go under `agents/coding-go`. Rust is only the Axiom runtime protocol bridge.

| Capability | Status | Axiom implementation |
|---|---|---|
| Read/list/grep | Implemented in Go | Workspace-scoped tools |
| Exact edit/write | Implemented in Go | Single-match edit and bounded write |
| Bash | Implemented in Go | Explicit argv allowlist, no shell interpolation |
| ReAct tool loop | Implemented | `ReActScheduler` plus durable proposals |
| Permissions/audit | Implemented | Capability leases, Shell policy, EventLog |
| Subagents | Kernel-supported | `ChildRun` sandbox and gate merge |
| MCP/custom tools | Kernel-supported boundary | Capability transport/registry adapters |
| Todo/session UI/LSP | Not yet parity | Future validation agents and SDK surfaces |
| Model quality parity | Not claimed | Current Go planner is deterministic; online comparison requires identical models and repeated trials |

The initial benchmark creates a real Rust workspace containing a failing test. The agent lists and reads files, searches for the defect, applies an exact edit, runs the test suite, and emits an audited final response.
