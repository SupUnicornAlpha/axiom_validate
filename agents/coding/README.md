# Axiom Coding Agent Validation

This agent validates the OpenCode-style coding runtime loop rather than copying OpenCode's TUI or provider ecosystem.

| Capability | Status | Axiom implementation |
|---|---|---|
| Read/list/grep | Implemented | Workspace-scoped capability drivers |
| Exact edit/write | Implemented | Single-match edit and bounded write |
| Bash | Implemented | Explicit command allowlist, no shell interpolation |
| ReAct tool loop | Implemented | `ReActScheduler` plus durable proposals |
| Permissions/audit | Implemented | Capability leases, Shell policy, EventLog |
| Subagents | Kernel-supported | `ChildRun` sandbox and gate merge |
| MCP/custom tools | Kernel-supported boundary | Capability transport/registry adapters |
| Todo/session UI/LSP | Not yet parity | Future validation agents and SDK surfaces |
| Model quality parity | Not claimed | Requires identical model, prompt, task set, and repeated trials |

The initial benchmark creates a real Rust workspace containing a failing test. The agent lists and reads files, searches for the defect, applies an exact edit, runs the test suite, and emits an audited final response.
