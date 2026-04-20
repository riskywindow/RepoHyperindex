# How To Run Phase 2

Phase 2 is the local daemon-and-CLI smoke path only.

It does not include parsing, indexing, search, semantic retrieval, impact analysis, a UI, or an
editor extension.

## Prerequisites

- Rust toolchain with `cargo`
- `git`
- `python3`

## One Command Smoke Run

From the repo root:

```bash
bash scripts/phase2-smoke.sh
```

The script will:

1. build `hyperctl` and `hyperd`
2. create a temporary local git repo
3. start the daemon
4. add the repo and inspect daemon/repo status
5. create a clean snapshot
6. set an unsaved buffer overlay for one file
7. create a second snapshot that includes the buffer
8. show that `snapshot read-file` returns the unsaved contents
9. show that `snapshot diff` reports the same path as a buffer-only change
10. clear the buffer, remove the repo, and stop the daemon

The smoke workspace is created under a temporary directory and is cleaned up automatically.

## Manual Quickstart

Build the binaries:

```bash
cargo build -p hyperindex-cli -p hyperindex-daemon
```

Create a working directory for the demo and run the commands from inside it so the default
relative `.hyperindex/` paths stay contained:

```bash
mkdir -p /tmp/repo-hyperindex-phase2-demo
cd /tmp/repo-hyperindex-phase2-demo
/absolute/path/to/RepoHyperindex/target/debug/hyperctl --config-path ./config.toml config init --force
/absolute/path/to/RepoHyperindex/target/debug/hyperctl --config-path ./config.toml daemon start
```

Core daemon-backed commands in the polished Phase 2 path:

- `hyperctl daemon start|status|stop [--json]`
- `hyperctl repos add|list|show|remove [--json]`
- `hyperctl repo status [--json]`
- `hyperctl snapshot create|show|diff|read-file [--json]`
- `hyperctl buffers set|clear|list [--json]`

Use `--json` for script consumption. Human output remains concise by default.

If `hyperctl daemon start` fails with a Unix-socket bind error, the environment is blocking local
socket creation. The checked-in Rust tests still validate the same request/response contract
through the stdio fallback, but the full daemon smoke path needs a normal local workstation
environment where Unix-domain sockets are permitted.
