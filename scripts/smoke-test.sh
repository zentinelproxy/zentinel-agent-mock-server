#!/usr/bin/env bash
# Smoke test: run the agent binary behind a real Zentinel (fail-closed) with
# the shared harness from zentinelproxy/.github (agent-smoke/smoke.py).
#
#   AGENT_SMOKE=<path to smoke.py> scripts/smoke-test.sh [<agent-binary> <zentinel-binary>]
#
# The binaries default to $AGENT_BIN and $ZENTINEL_BIN, which the shared
# release workflow sets. Run by that workflow on every platform before any
# asset is published, and by CI on every push and pull request.
#
# With scripts/smoke/mock-server.yaml (passthrough for unmatched requests),
# over the Unix socket and over gRPC:
#   * the v2 handshake works and a request without a stub is forwarded (200);
#   * a stubbed GET /mocked is answered 200 by the agent and never reaches the
#     upstream;
#   * a stub with status 418 is answered 418;
#   * an exact-path stub with a query matcher matches /search?q=... (201) and
#     /search without q falls through to the upstream (200);
#   * with the agent stopped the clean request is refused (agent on the path).

set -euo pipefail

AGENT_BIN=${1:-${AGENT_BIN:?usage: smoke-test.sh <agent-binary> <zentinel-binary>}}
ZENTINEL_BIN=${2:-${ZENTINEL_BIN:?usage: smoke-test.sh <agent-binary> <zentinel-binary>}}
AGENT_SMOKE=${AGENT_SMOKE:?set AGENT_SMOKE to agent-smoke/smoke.py from zentinelproxy/.github}

CONFIG="$(cd "$(dirname "$0")" && pwd)/smoke/mock-server.yaml"

cases=(
    --clean '{"name": "no stub: passthrough", "path": "/smoke"}'
    --case '{"name": "stubbed endpoint", "path": "/mocked", "expect": 200, "reaches_upstream": false}'
    --case '{"name": "stubbed error status", "path": "/teapot/pot", "expect": 418}'
    --case '{"name": "exact path + query matcher", "path": "/search?q=zentinel", "expect": 201, "reaches_upstream": false}'
    --case '{"name": "query matcher not met: passthrough", "path": "/search", "expect": 200}'
)

for transport in uds grpc; do
    echo "=== mock-server agent over $transport"
    python3 "$AGENT_SMOKE" --agent "$AGENT_BIN" --zentinel "$ZENTINEL_BIN" \
        --name mock-server --transport "$transport" \
        --agent-arg=--config --agent-arg "$CONFIG" \
        "${cases[@]}"
done
