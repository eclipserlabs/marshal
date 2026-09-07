# Marshall

[![CI](https://github.com/rapture-fx/Marshall/actions/workflows/ci.yml/badge.svg)](https://github.com/rapture-fx/Marshall/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.2.0-blue.svg)](Cargo.toml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition_2021-orange.svg)](Cargo.toml)

A Rust library and HTTP service (`marshalld`) for running agent tools behind explicit policies.

Every tool denies by default. Filesystem paths must resolve inside a configured root, HTTP hosts must be allowlisted and resolve to public addresses, shell binaries must be listed with an argument policy, and code runs only in allowlisted languages. A tool with no configuration refuses every call.

## Contents

- [When to use this](#when-to-use-this)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Tools](#tools)
- [Security notes](#security-notes)
- [HTTP API](#http-api)
- [SDKs](#sdks)
- [Development](#development)
- [Limitations](#limitations)
- [License](#license)

## When to use this

Use Marshall when an agent needs to read files, run commands, fetch URLs, or execute code snippets, and you want those actions checked against a policy first. Register only the tools the agent needs, scope them to a workspace directory, and run `marshalld` when callers live in another process or language.

What you get:

- Filesystem, shell, HTTP, code, and system tools behind allowlists
- Session-scoped helpers for working memory: think, memory, todo, plan, reflect
- HTTP client that checks the allowlist before DNS, disables redirects, pins the connection to the validated address, and caps body size
- Shell execution without an intermediate shell: absolute program paths, argument policies, piped stdin, output caps, timeout with child cleanup
- Capped output everywhere: file reads, HTTP bodies, and process output report truncation instead of growing without bound
- Summaries safe to log: `ToolOutcome.summary` carries hashes and byte counts, payload bytes stay in `content`
- Batch and sequence execution with ordering, concurrency bounds, and `{{steps[N].stdout}}` templating
- `marshalld`: sessions, per-request policy checks, Prometheus metrics, JSONL audit log, YAML hot-reload

## Quick start

Add the library:

```sh
cargo add marshall
```

Register tools explicitly. Anything not registered, or not covered by the policy, returns an error:

```rust
use std::sync::Arc;
use marshall::{FileSystemTool, HttpTool, Sandbox, ToolRegistry};

let sandbox = Sandbox::new(["/srv/agent/workspace"])?;

let mut tools = ToolRegistry::new();
tools.register(Arc::new(FileSystemTool::new(sandbox)));
tools.register(Arc::new(HttpTool::new(["api.github.com"])));
```

A fuller example with shell and code tools:

```rust
use std::sync::Arc;
use std::time::Duration;
use marshall::{
    ArgumentPolicy, CodeTool, FileSystemTool, HttpTool,
    Sandbox, ShellTool, ToolRegistry,
    shell::AllowedCommand,
};

let sandbox = Sandbox::new(["/srv/agent/workspace"])?;

let mut tools = ToolRegistry::new();
tools.register(Arc::new(FileSystemTool::new(sandbox.clone())));
tools.register(Arc::new(HttpTool::new(["api.github.com"])));
tools.register(Arc::new(
    ShellTool::new(vec![
        AllowedCommand::new("/usr/bin/git")
            .with_arguments(ArgumentPolicy::Exact(vec![vec!["status".into()]])),
    ])
    .with_working_dirs(sandbox.clone())
    .with_timeout(Duration::from_secs(5)),
));
tools.register(Arc::new(
    CodeTool::new().with_sandbox(sandbox).allow_all(),
));
```

Run the runnable example:

```sh
cargo run --example agent_tools
```

It wires a temp workspace, shows one allowed read and one allowed shell call, then shows four refused calls (path escape, flag injection, metadata endpoint, unlisted host).

To run the service instead:

```sh
git clone https://github.com/rapture-fx/Marshall.git
cd Marshall
cargo build
cargo run --bin marshalld -- --config marshall.yaml --port 3000
```

## Configuration

`marshalld` reads `marshall.yaml`. The file is watched and reloaded without a restart.

```yaml
workspace: /tmp/marshalld
concurrency: 32
audit_log: ./audit.jsonl

filesystem:
  writable: true
  read_limit: 8388608

shell:
  timeout_ms: 10000
  output_limit: 1048576
  commands:
    - program: /bin/echo
      args: NoFlags
    - program: /usr/bin/git
      args:
        Exact: [["status"], ["log", "--oneline"]]

http:
  allowed_hosts: [api.github.com, registry.npmjs.org]
  request_body_limit: 1048576
  response_body_limit: 4194304
  timeout_ms: 30000

code:
  allowed_languages: [python, bash, javascript]
  timeout_ms: 10000
  output_limit: 1048576

system:
  allowed_env: [PATH, TZ]
  allow_process_list: false
  allow_kill: false
  max_sleep_ms: 5000
```

Check the file without starting the server:

```sh
cargo run --bin marshalld -- --validate-config ./marshall.yaml
```

Requirements:

- Rust 1.88 or later
- Linux or macOS for development; Linux for `openat2`-backed path checks and container isolation
- Optional: `python3` / `node` on `PATH` if you enable those code languages
- Optional: `/dev/kvm` on Linux for the `container` backend

## Tools

| Name | Operations | Policy |
|---|---|---|
| `filesystem` | `read/write/list/mkdir/delete/stat/copy/move/append/search/glob/patch` | Paths must resolve under a sandbox root; `writable` gates mutating ops; reads capped at `read_limit` |
| `shell` | Run listed binaries | Absolute path, `ArgumentPolicy` (`None`, `Exact`, `NoFlags`, `Unrestricted`), timeout, output cap, cleared environment |
| `http` | Outbound requests | Host allowlist, public-address check, no redirects, port allowlist, body caps |
| `code` | `python` / `javascript` / `bash` via temp file | `allowed_languages`, 64 KiB source cap, timeout, output cap, sandbox working directory |
| `system` | `now/sleep/env_get/env_list/hash/info/process_list/process_kill` | `allowed_env`, `allow_process_list`, `allow_kill`, `max_sleep_ms` |
| `think` | Record a reasoning step | Length-bounded, always succeeds |
| `memory` | Session-scoped key/value store | Key/value size bounds, TTL, LRU cap |
| `todo` | Session-scoped task list | Item count cap |
| `plan` | Session-scoped multi-step plans | Plan/step count caps |
| `reflect` | Classify a prior outcome | Expects an outcome object |

`memory`, `todo`, and `plan` are scoped by `session_id` (`session_id|global`). `marshalld` fills this from the top-level `session_id` in batch and sequence requests.

## Security notes

Filesystem: roots are compared by path component, not string prefix, so `/tmp/safe` does not admit `/tmp/safe_evil`. Symlinks are resolved before the check. On Linux, resolution uses `openat2` with `RESOLVE_BENEATH` (`src/sandbox.rs`); elsewhere it falls back to `canonicalize`. The `openat2` file descriptor is not retained for I/O, and the check-then-use interval remains on non-Linux platforms.

HTTP: the metadata address `169.254.169.254` is refused in its common spellings (literal, IPv4-mapped IPv6, 6to4, embedded credentials), and `tests/escapes.rs` covers each case. Host matching is case-insensitive; percent-encoded or numeric-IP forms are rejected as malformed. Redirects are disabled and the TCP connection uses the addresses returned during validation instead of resolving again. `marshalld` repeats the destination and egress checks server-side. An allowlisted host that proxies or redirects still extends trust to wherever it sends clients.

Shell: the allowlist selects the binary only. Most binaries accept options that reach the filesystem or network (`find -exec`, `git --exec-path`, `tar --to-command`), so the argument policy is the effective control. `NoFlags` blocks `-`/`--` options but remains a heuristic: a program that treats a bare positional as a script is still unsafe. No shell is spawned, so `;`, `|`, and `$(…)` in arguments have no special meaning.

Output: `summary` is intended for logs and transcripts. Header values are allowlisted (`set-cookie`, `authorization`, and similar are dropped), long values are truncated, and environment values are returned in `content` with only a hash in `summary`.

Isolation: by default this crate enforces policy in-process. It does not apply seccomp, namespaces, or chroot. For stronger containment, run `marshalld` under the `WasmBackend` (`--features wasm`, wasmtime fuel/memory and WASI preopen) or `ContainerBackend` (`--features container`, Firecracker via the [`watchdog`](https://github.com/wiramahendra/watchdog) crate, Linux KVM required, falls back to local execution with a warning elsewhere), or place the service itself in a container or VM. CORS defaults to `AllowOrigin::any()`; restrict it when the port is reachable beyond localhost.

## HTTP API

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Liveness, version, tool and session counts |
| `GET` | `/metrics` | Prometheus exposition |
| `GET` | `/v1/tools` | Tool definitions |
| `GET` | `/v1/policy` | Active policy file |
| `POST` | `/v1/sessions` | Create a session workspace |
| `DELETE` | `/v1/sessions/:id` | Remove a session |
| `POST` | `/v1/execute` | Run one tool |
| `POST` | `/v1/execute/batch` | Run requests concurrently, results in input order |
| `POST` | `/v1/execute/sequence` | Run steps in order with optional `continue_on_error` and templating |
| `POST` | `/v1/execute/stream` | SSE stream (`summary`, `chunk`, `done` events) |

```sh
curl http://localhost:3000/health
curl http://localhost:3000/v1/tools | jq '.[].name'

curl -X POST http://localhost:3000/v1/execute \
  -d '{"tool":"code","args":{"language":"python","code":"print(42)"}}'

SID=$(curl -s -X POST http://localhost:3000/v1/sessions | jq -r .session_id)
curl -X POST http://localhost:3000/v1/execute/batch \
  -d "{\"session_id\":\"$SID\",\"requests\":[{\"tool\":\"memory\",\"args\":{\"operation\":\"store\",\"key\":\"k\",\"value\":1}}]}"
```

Batch and sequence accept a top-level `session_id` and per-entry `session_id` values. Per-entry values take precedence for the session-scoped tools.

## SDKs

- JavaScript: `sdk/js/` (`marshall-sdk`)
- Python: `sdk/python/` (`marshall_sdk.py`)

```js
import { ExecutionClient } from './index.js';
const c = new ExecutionClient('http://localhost:3000');
await c.execute('shell', { program: '/bin/echo', args: ['hi'] });
await c.batch([['shell', { program: '/bin/echo', args: ['hi'] }]], { sessionId: SID });
for await (const evt of c.stream('shell', { program: '/bin/echo', args: ['hi'] })) console.log(evt);
```

```python
from marshall_sdk import ExecutionClient
c = ExecutionClient("http://localhost:3000")
c.execute("shell", {"program": "/bin/echo", "args": ["hi"]})
c.batch([("shell", {"program": "/bin/echo", "args": ["hi"]})], session_id=SID)
for event, data in c.stream("shell", {"program": "/bin/echo", "args": ["hi"]}):
    print(event, data)
```

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test --doc
cargo test --test escapes
cargo test --test validation
cargo run --bin fuzz_destination
cargo run --bin fuzz_destination -- 'https://example.com/'
```

`tests/escapes.rs` records each sandbox and SSRF bypass as an attack case rather than a plain assertion. `tests/stress.rs` runs 10k idempotent executions to bound the `execute_once` cache.

## Limitations

- Session state (`memory`, `todo`, `plan`, `sessions`) is in-memory and lost on restart; there is no expiry pass for old sessions.
- `process_list` reads Linux `/proc` and reports `not_supported` elsewhere. `process_kill` is not scoped to session children.
- No per-token authentication or rate limiting beyond the global concurrency semaphore.
- The `watchdog` container dependency tracks git `main`.

## License

MIT. See [LICENSE](LICENSE).
