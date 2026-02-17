# syntax=docker/dockerfile:1.4

# Zentinel Mock Server Agent Container Image
#
# Targets:
#   - prebuilt: For CI with pre-built binaries

################################################################################
# Pre-built binary stage (for CI builds)
################################################################################
FROM gcr.io/distroless/cc-debian12:nonroot AS prebuilt

COPY zentinel-agent-mock-server /zentinel-agent-mock-server

LABEL org.opencontainers.image.title="Zentinel Mock Server Agent" \
      org.opencontainers.image.description="Zentinel Mock Server Agent for Zentinel reverse proxy" \
      org.opencontainers.image.vendor="Raskell" \
      org.opencontainers.image.source="https://github.com/zentinelproxy/zentinel-agent-mock-server"

ENV RUST_LOG=info,zentinel_agent_mock_server=debug \
    SOCKET_PATH=/var/run/zentinel/mock-server.sock

USER nonroot:nonroot

ENTRYPOINT ["/zentinel-agent-mock-server"]
