# sdk_conformance_runspec

## Goal

验证 TypeScript / Python / Go / Java SDK 对同一高层 agent 定义，是否能产出等价 `RunSpec`。

## Planned Checks

- `run_name`
- `step topology`
- `capability lease set`
- `namespace`
- `budget group`

## Status

在 Rust core 稳定后，按 `TypeScript -> Python -> Go -> Java` 顺序推进。
