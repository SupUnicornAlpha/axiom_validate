# local_remote_invariance

## Goal

验证同一 `RunSpec` 在：

- `LocalTransport`
- `RemoteTransport mock`

下是否得到一致的：

- 终态
- Event 数量
- denied / merge / output 语义

## Status

当前 RemoteTransport 仍未实现，本 case 作为下一阶段门槛。
