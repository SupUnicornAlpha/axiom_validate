# Axiom Coding Agent (Go)

Go owns the coding agent: prompts, planner/decide, and workspace tools.
Rust (`axiom_validate/src/coding_agent.rs`) is only the Axiom runtime protocol bridge
(`ReActScheduler` + capability leases + EventLog).

## Commands

| Command | Purpose |
|---------|---------|
| `prompt` | Print the system prompt |
| `plan [task]` | Offline deterministic decision script (CI / `coding_agent_opencode_parity`) |
| `decide [obs.json\|-]` | Live per-turn LLM decision via DeepSeek (stdin or file) |
| `tool <name> <root> <perm> <input>` | Execute a workspace-scoped tool |

## DeepSeek live mode

Set environment variables:

```bash
export DEEPSEEK_API_KEY="sk-..."          # required
export DEEPSEEK_BASE_URL="https://api.deepseek.com"  # optional
export DEEPSEEK_MODEL="deepseek-chat"                # optional
```

From `axiom_validate` (recommended — goes through Kernel + Shell + EventLog):

```bash
# Uses a built-in buggy calculator fixture under reports/generated/
cargo run -- try-coding

# Or point at your own workspace
cargo run -- try-coding --workspace /path/to/project --task "Fix the failing tests"
```

Direct Go smoke test (decide only):

```bash
cd agents/coding-go
echo '{"task":"Say hello by finishing","messages":[],"outputs":[],"denied_actions":[],"next_step_index":0,"visible_capabilities":["coding/list"]}' \
  | go run . decide -
```

## Capability table

| Capability | Status | Notes |
|---|---|---|
| Read/list/grep | Implemented | Workspace-scoped |
| Exact edit/write | Implemented | Single-match edit |
| Bash | Implemented | Argv allowlist (`cargo test/check/build`, `go test/build`, pytest) |
| ReAct tool loop | Implemented | Offline `plan` or live `decide` + DeepSeek |
| Permissions/audit | Via Axiom | Capability leases, Shell, EventLog |
| Model quality parity | Not claimed | Live runs depend on model + task |
