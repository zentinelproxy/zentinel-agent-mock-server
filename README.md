<p align="center">
  <a href="https://zentinelproxy.io">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/zentinelproxy/design/main/marks/lockup/zentinel-lockup-dark.svg">
      <img alt="Zentinel" src="https://raw.githubusercontent.com/zentinelproxy/design/main/marks/lockup/zentinel-lockup-light.svg" height="52">
    </picture>
  </a>
</p>

<h3 align="center">Mock server agent</h3>

<p align="center">Answers matching requests with stub responses from a YAML file, at the request-headers phase, for development, tests and demos.</p>

<p align="center">
  <a href="https://github.com/zentinelproxy/zentinel-agent-mock-server/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/zentinelproxy/zentinel-agent-mock-server/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-9b4d8a?style=flat&labelColor=1f161e">
  <img alt="Agent protocol: v2" src="https://img.shields.io/badge/agent%20protocol-v2-f68d31?style=flat&labelColor=1f161e">
</p>

<p align="center">
  <a href="https://registry.zentinelproxy.io/mock-server/">Registry</a> &middot;
  <a href="https://docs.zentinelproxy.io">Docs</a> &middot;
  <a href="https://github.com/zentinelproxy">All repositories</a>
</p>

The agent matches each request against a list of stubs (method, path, query
parameters, headers) and, on a match, returns the stub's status, headers and
body to the client instead of forwarding the request. Responses can be static,
rendered from Handlebars templates, delayed, or replaced by an injected fault.
Unmatched requests get a default 404, or go to the upstream with
`passthrough_unmatched`. Because it answers instead of the upstream, set
`failure-mode "closed"` on the filter if a missing agent should be noticed
rather than silently fall through to the real backend.

## Quick start

```bash
zentinel bundle install mock-server
```

`mock-server.yaml`:

```yaml
stubs:
  - id: hello-world
    request:
      method: [GET]
      path:
        type: exact
        value: /api/hello
    response:
      status: 200
      body:
        type: json
        content:
          message: "Hello, World!"
```

```bash
zentinel-mock-server-agent --validate -c mock-server.yaml
zentinel-mock-server-agent -c mock-server.yaml --socket /var/run/zentinel/mock-server.sock
```

```kdl
system {
    worker-threads 0
}

listeners {
    listener "http" {
        address "127.0.0.1:8080"
        protocol "http"
    }
}

agents {
    agent "mock-server" type="custom" {
        unix-socket "/var/run/zentinel/mock-server.sock"
        events "request_headers"
        timeout-ms 1000
        failure-mode "closed"
    }
}

filters {
    filter "mock-server" {
        type "agent"
        agent "mock-server"
        failure-mode "closed"
    }
}

routes {
    route "api" {
        matches {
            path-prefix "/api/"
        }
        upstream "backend"
        filters "mock-server"
    }
}

upstreams {
    upstream "backend" {
        target "127.0.0.1:3000"
    }
}
```

`curl http://127.0.0.1:8080/api/hello` now returns the stub. Raise
`timeout-ms` above the longest `delay` or `timeout` fault you configure, or the
proxy gives up on the agent first.

## Install

**Bundle (recommended).** Downloads the release binary for your platform:

```bash
zentinel bundle install mock-server
```

See [the bundle documentation](https://docs.zentinelproxy.io/deployment/bundle/).

**Release binaries.** Each [release](https://github.com/zentinelproxy/zentinel-agent-mock-server/releases)
ships `linux-x86_64`, `linux-aarch64` and `darwin-aarch64` archives:

```bash
VERSION=0.4.0
PLATFORM=linux-x86_64   # or linux-aarch64, darwin-aarch64
curl -fsSL -o zentinel-mock-server-agent.tar.gz \
  "https://github.com/zentinelproxy/zentinel-agent-mock-server/releases/download/v${VERSION}/zentinel-mock-server-agent-${VERSION}-${PLATFORM}.tar.gz"
tar -xzf zentinel-mock-server-agent.tar.gz
sudo install -m 0755 zentinel-mock-server-agent /usr/local/bin/
```

**From source.** The crate takes `zentinel-agent-sdk` as a path dependency on a
sibling `zentinel-agent-rust-sdk` checkout, so `cargo install --git` does not
work, and the crate is not on crates.io:

```bash
git clone https://github.com/zentinelproxy/zentinel-agent-rust-sdk
git clone https://github.com/zentinelproxy/zentinel-agent-mock-server
cd zentinel-agent-mock-server
cargo build --release
```

## Configuration

Command-line flags:

| Flag | Default | Meaning |
|---|---|---|
| `-c, --config <PATH>` | `mock-server.yaml` | Stub file; if it does not exist the agent starts with no stubs |
| `-s, --socket <PATH>` | `/tmp/zentinel-mock-server.sock` | Unix socket to listen on |
| `--grpc-address <ADDR>` | none | Also serve gRPC on this address, e.g. `0.0.0.0:50051` |
| `-L, --log-level <LEVEL>` | `info` | `trace`, `debug`, `info`, `warn`, `error` |
| `--print-config` | | Print [`examples/default-config.yaml`](examples/default-config.yaml) and exit |
| `--validate` | | Parse and validate the file, then exit |

The YAML file is the only way to configure the agent: it rejects unknown keys at
the top level, in stubs, requests and responses, and an agent block's
`config { }` in the proxy's KDL is not forwarded to the agent. Changes need a
restart.

### Stubs

| Field | Default | Meaning |
|---|---|---|
| `id` | required | Stub name, used in logs and match counts |
| `name` | none | Free-text label |
| `request` | required | What to match (below); every given condition must hold |
| `response` | required | What to return (below) |
| `priority` | `0` | Higher priorities are tried first |
| `enabled` | `true` | Skip the stub when `false` |
| `max_matches` | `0` (unlimited) | After this many matches the stub counts as unmatched |
| `delay` | none | `fixed_ms`, or a random `min_ms`..`max_ms` range |
| `fault` | none | Replace the response with a fault (below) |

**Request matching.**

- `method`: a list, e.g. `[GET, POST]`; empty matches any method.
- `path` (`type` plus one value field): `exact` / `prefix` with `value`,
  `regex` / `glob` with `pattern`, `template` with `template` (e.g.
  `/users/{id}`; the parameters are available to templates).
- `query`: a map of parameter to `exact` (`value`), `regex` (`pattern`),
  `present` or `absent`.
- `headers`: a map of header to `exact` (`value`), `regex` (`pattern`),
  `contains` (`value`), `present` or `absent`. Names match case-insensitively.
- `body`: `exact`, `regex`, `contains`, `json`, `json_path` and `empty` are
  parsed, but the agent only receives request headers, so it sees no body:
  stubs with a body matcher other than `empty` never match through the proxy
  today.

**Response.**

| Field | Default | Meaning |
|---|---|---|
| `status` | `200` | Status code, 100 to 599 |
| `headers` | `{}` | Response headers |
| `body` | none | `text` / `json` / `base64` with `content`, or `file` with `path` |
| `template` | `false` | Render the body's strings with Handlebars |

Without a `Content-Type` header, the body type decides it (`text/plain`,
`application/json`, `application/octet-stream`), then
`settings.default_content_type`.

**Templates.** With `template: true`, body strings can use `{{path.<name>}}`,
`{{query.<name>}}`, `{{headers.<name>}}`, `{{captures.<group>}}` (regex path
groups, by name or number), `{{method}}`,
`{{request_path}}`, and the helpers `{{uuid}}`, `{{now}}` /
`{{now "%Y-%m-%d"}}`, `{{random 1 100}}`, `{{default value "fallback"}}`,
`{{upper value}}`, `{{lower value}}` and `{{json value}}`. `{{body}}` and
`{{json.<field>}}` exist but are empty for the reason above.

**Faults.** A configured fault applies to every match:

| `type` | Fields | Effect |
|---|---|---|
| `error` | `status`, `message` | Returns that status with `message` (default `Error`) as `text/plain` |
| `timeout` | `duration_ms` | Waits, then returns 504; the proxy's agent `timeout-ms` usually fires first |
| `empty` | | Returns 200 with an empty body |
| `corrupt` | `probability` (default `1.0`) | With that probability returns 200 with random bytes, otherwise the normal response |
| `slow_response` | `bytes_per_second` | Delays the whole response by body size / rate; the body is not streamed |

### Global settings

```yaml
settings:
  log_matches: true
  log_unmatched: true
  passthrough_unmatched: false
  default_content_type: application/json

default_response:
  status: 404
  body:
    type: json
    content:
      error: "not_found"
```

| Option | Default | Meaning |
|---|---|---|
| `settings.log_matches` | `true` | Log each matched request |
| `settings.log_unmatched` | `true` | Log each unmatched request |
| `settings.passthrough_unmatched` | `false` | Forward unmatched requests to the upstream instead of answering |
| `settings.default_content_type` | `application/json` | Content type of a response that has neither a `Content-Type` header nor a body |
| `settings.case_insensitive_headers` | `true` | Accepted but not read; header names always match case-insensitively |
| `default_response` | 404 JSON `{"error": "not_found", ...}` | Response for unmatched requests |

Proxy-side options (`events`, `timeout-ms`, `failure-mode`):
<https://docs.zentinelproxy.io/configuration/agents/>. Agent reference:
<https://registry.zentinelproxy.io/mock-server/>.

## How it works

On each `request_headers` event the agent sorts enabled stubs by priority and
takes the first whose conditions all hold. A match is answered from the agent
as a block decision carrying the stub's status, headers and body, so the
upstream never sees the request. While the proxy drains or shuts the agent
down, it stops answering and lets requests through. It reports request,
match and stub counts as v2 metrics (`mock_server_requests_total`,
`mock_server_requests_matched_total`, `mock_server_requests_unmatched_total`,
`mock_server_stubs_configured`, `mock_server_stubs_enabled`).

## Development

The build needs `zentinel-agent-rust-sdk` checked out next to this one.

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Related

- [Registry page](https://registry.zentinelproxy.io/mock-server/)
- [Agents configuration](https://docs.zentinelproxy.io/configuration/agents/)
- [Agent protocol v2](https://docs.zentinelproxy.io/agents/v2/)
- [Zentinel](https://github.com/zentinelproxy/zentinel)
- [Chaos agent](https://github.com/zentinelproxy/zentinel-agent-chaos), for fault injection in front of a real upstream
- [Rust agent SDK](https://github.com/zentinelproxy/zentinel-agent-rust-sdk), which this agent is built on

## License

Apache-2.0, see [`LICENSE`](LICENSE).

---

<p align="center">
  <sub>Part of <a href="https://zentinelproxy.io">Zentinel</a>, the security-first reverse proxy. The shisa guards the gate.</sub>
</p>
