# syntax=docker/dockerfile:1.4

# Sentinel Mock Server Agent Container Image
#
# Targets:
#   - prebuilt: For CI with pre-built binaries

################################################################################
# Pre-built binary stage (for CI builds)
################################################################################
FROM gcr.io/distroless/cc-debian12:nonroot AS prebuilt

COPY sentinel-agent-mock-server /sentinel-agent-mock-server

LABEL org.opencontainers.image.title="Sentinel Mock Server Agent" \
      org.opencontainers.image.description="Sentinel Mock Server Agent for Sentinel reverse proxy" \
      org.opencontainers.image.vendor="Raskell" \
      org.opencontainers.image.source="https://github.com/raskell-io/sentinel-agent-mock-server"

ENV RUST_LOG=info,sentinel_agent_mock_server=debug \
    SOCKET_PATH=/var/run/sentinel/mock-server.sock

USER nonroot:nonroot

ENTRYPOINT ["/sentinel-agent-mock-server"]
