# Sentinel Mock Server Agent

A mock server agent for [Sentinel](https://sentinel.raskell.io) that intercepts requests and returns configurable stub responses. Perfect for testing, development, and API demos.

## Features

- **Request Matching**: Match by path (exact, prefix, regex, glob, template), method, headers, query params, body
- **Static Responses**: Return fixed responses for matched requests
- **Dynamic Templates**: Use Handlebars templates for dynamic responses
- **Latency Simulation**: Add fixed or random delays
- **Failure Injection**: Simulate errors, timeouts, empty responses, corruption
- **Match Limits**: Limit how many times a stub can be matched
- **Priority Matching**: Control which stub matches when multiple could match

## Installation

### Using Cargo

```bash
cargo install sentinel-agent-mock-server
```

### From Source

```bash
git clone https://github.com/raskell-io/sentinel-agent-mock-server
cd sentinel-agent-mock-server
cargo build --release
```

## Quick Start

1. Create a configuration file `mock-server.yaml`:

```yaml
stubs:
  - id: hello-world
    request:
      method: [GET]
      path:
        type: exact
        value: /hello
    response:
      status: 200
      body:
        type: json
        content:
          message: "Hello, World!"
```

2. Add to your Sentinel configuration:

```kdl
agents {
    mock-server socket="/tmp/sentinel-mock-server.sock"
}
```

3. Start the agent:

```bash
sentinel-agent-mock-server -c mock-server.yaml
```

## Configuration

### Path Matching

```yaml
# Exact match
path:
  type: exact
  value: /api/users

# Prefix match
path:
  type: prefix
  value: /api/

# Regex match
path:
  type: regex
  pattern: "^/api/v[0-9]+/.*"

# Glob match
path:
  type: glob
  pattern: "/api/*/users"

# Template with parameters
path:
  type: template
  template: /users/{id}
```

### Query Parameter Matching

```yaml
request:
  query:
    # Exact value
    page:
      type: exact
      value: "1"
    # Regex pattern
    limit:
      type: regex
      pattern: "^\\d+$"
    # Must be present
    search:
      type: present
    # Must be absent
    debug:
      type: absent
```

### Header Matching

```yaml
request:
  headers:
    authorization:
      type: present
    content-type:
      type: exact
      value: application/json
    user-agent:
      type: contains
      value: "Mozilla"
```

### Body Matching

```yaml
request:
  body:
    # Must be valid JSON
    type: json

    # Exact match
    type: exact
    value: '{"key": "value"}'

    # Contains substring
    type: contains
    value: "search-term"

    # JSON path expressions
    type: json_path
    expressions:
      $.email: null  # Just check exists
      $.role: "admin"
```

### Response Configuration

```yaml
response:
  status: 200
  headers:
    X-Custom-Header: "value"
  body:
    # Text body
    type: text
    content: "Hello, World!"

    # JSON body
    type: json
    content:
      message: "success"

    # Base64 binary
    type: base64
    content: "SGVsbG8sIFdvcmxkIQ=="

    # File contents
    type: file
    path: ./fixtures/response.json
```

### Dynamic Templates

Use Handlebars templates for dynamic responses:

```yaml
response:
  template: true
  body:
    type: json
    content:
      id: "{{path.id}}"
      query: "{{query.q}}"
      user_agent: "{{headers.user-agent}}"
      timestamp: "{{now}}"
      request_id: "{{uuid}}"
```

Available template helpers:
- `{{path.name}}` - Path parameters from template matching
- `{{query.name}}` - Query parameters
- `{{headers.name}}` - Request headers
- `{{json.field}}` - Fields from JSON request body
- `{{body}}` - Raw request body
- `{{method}}` - Request method
- `{{request_path}}` - Request path
- `{{uuid}}` - Generate a random UUID
- `{{now}}` / `{{now "%Y-%m-%d"}}` - Current timestamp
- `{{random 1 100}}` - Random number in range
- `{{default value "fallback"}}` - Default value
- `{{upper value}}` / `{{lower value}}` - Case conversion

### Latency Simulation

```yaml
# Fixed delay
delay:
  fixed_ms: 1000

# Random range
delay:
  min_ms: 100
  max_ms: 500
```

### Failure Injection

```yaml
# Return error
fault:
  type: error
  status: 500
  message: "Internal Server Error"

# Simulate timeout
fault:
  type: timeout
  duration_ms: 30000

# Return empty response
fault:
  type: empty

# Return corrupted data
fault:
  type: corrupt
  probability: 0.5

# Slow drip response
fault:
  type: slow_response
  bytes_per_second: 100
```

### Match Limits

```yaml
# Only match first 5 times
max_matches: 5
```

### Priority

```yaml
# Higher priority matches first (default: 0)
priority: 10
```

## Global Settings

```yaml
settings:
  # Log matched requests
  log_matches: true

  # Log unmatched requests
  log_unmatched: true

  # Pass unmatched to upstream (false = return 404)
  passthrough_unmatched: false

  # Default content type
  default_content_type: application/json

  # Case-insensitive header matching
  case_insensitive_headers: true

# Default response for unmatched requests
default_response:
  status: 404
  body:
    type: json
    content:
      error: "not_found"
```

## CLI Options

```
sentinel-agent-mock-server [OPTIONS]

Options:
  -c, --config <PATH>        Configuration file [default: mock-server.yaml]
  -s, --socket <PATH>        Unix socket path [default: /tmp/sentinel-mock-server.sock]
  -L, --log-level <LEVEL>    Log level [default: info]
      --print-config         Print example configuration
      --validate             Validate configuration and exit
  -h, --help                 Print help
  -V, --version              Print version
```

## Use Cases

### API Development

Mock backend services while developing frontend:

```yaml
stubs:
  - id: users-list
    request:
      method: [GET]
      path:
        type: exact
        value: /api/users
    response:
      status: 200
      body:
        type: json
        content:
          users:
            - id: 1
              name: Alice
            - id: 2
              name: Bob
```

### Integration Testing

Simulate specific API responses for tests:

```yaml
stubs:
  - id: payment-success
    request:
      method: [POST]
      path:
        type: exact
        value: /api/payments
      body:
        type: json_path
        expressions:
          $.amount: null
    response:
      status: 200
      body:
        type: json
        content:
          status: "success"
          transaction_id: "{{uuid}}"
```

### Chaos Testing

Inject failures to test resilience:

```yaml
stubs:
  - id: random-failures
    request:
      path:
        type: prefix
        value: /api/
    response:
      status: 200
    fault:
      type: error
      status: 503
    # 20% chance of triggering fault
    priority: -1  # Lower priority than normal stubs
```

### Demo Environments

Create realistic demo data:

```yaml
stubs:
  - id: user-profile
    request:
      method: [GET]
      path:
        type: template
        template: /users/{id}
    response:
      status: 200
      template: true
      body:
        type: json
        content:
          id: "{{path.id}}"
          name: "Demo User {{path.id}}"
          email: "user{{path.id}}@example.com"
          created_at: "{{now}}"
```

## License

Apache-2.0
