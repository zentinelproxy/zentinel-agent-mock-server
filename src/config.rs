//! Configuration for the Mock Server agent.
//!
//! Defines request matchers, response stubs, and simulation settings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Main configuration for the Mock Server agent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MockServerConfig {
    /// List of stub definitions
    #[serde(default)]
    pub stubs: Vec<StubDefinition>,

    /// Global settings
    #[serde(default)]
    pub settings: GlobalSettings,

    /// Default response when no stub matches
    #[serde(default)]
    pub default_response: Option<ResponseDefinition>,
}

impl MockServerConfig {
    /// Load configuration from a YAML file.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (i, stub) in self.stubs.iter().enumerate() {
            stub.validate()
                .map_err(|e| anyhow::anyhow!("Stub {}: {}", i, e))?;
        }
        Ok(())
    }
}

/// A single stub definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StubDefinition {
    /// Unique identifier for this stub
    pub id: String,

    /// Optional name/description
    #[serde(default)]
    pub name: Option<String>,

    /// Request matcher
    pub request: RequestMatcher,

    /// Response to return
    pub response: ResponseDefinition,

    /// Priority (higher = matched first)
    #[serde(default)]
    pub priority: i32,

    /// Whether this stub is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum number of times this stub can be matched (0 = unlimited)
    #[serde(default)]
    pub max_matches: u32,

    /// Latency simulation
    #[serde(default)]
    pub delay: Option<DelayConfig>,

    /// Failure simulation
    #[serde(default)]
    pub fault: Option<FaultConfig>,
}

fn default_true() -> bool {
    true
}

impl StubDefinition {
    /// Validate the stub definition.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.is_empty() {
            anyhow::bail!("Stub id cannot be empty");
        }
        self.request.validate()?;
        self.response.validate()?;
        Ok(())
    }
}

/// Request matching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestMatcher {
    /// HTTP method(s) to match (empty = any)
    #[serde(default)]
    pub method: Vec<String>,

    /// Path matching
    #[serde(default)]
    pub path: Option<PathMatcher>,

    /// Query parameter matching
    #[serde(default)]
    pub query: HashMap<String, QueryMatcher>,

    /// Header matching
    #[serde(default)]
    pub headers: HashMap<String, HeaderMatcher>,

    /// Body matching
    #[serde(default)]
    pub body: Option<BodyMatcher>,
}

impl RequestMatcher {
    /// Validate the request matcher.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            path.validate()?;
        }
        Ok(())
    }
}

/// Path matching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathMatcher {
    /// Exact path match
    Exact { value: String },
    /// Path prefix match
    Prefix { value: String },
    /// Regex pattern match
    Regex { pattern: String },
    /// Glob pattern match
    Glob { pattern: String },
    /// Path with parameters (e.g., /users/{id})
    Template { template: String },
}

impl PathMatcher {
    /// Validate the path matcher.
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            PathMatcher::Regex { pattern } => {
                regex::Regex::new(pattern).map_err(|e| anyhow::anyhow!("Invalid regex: {}", e))?;
            }
            PathMatcher::Glob { pattern } => {
                globset::Glob::new(pattern).map_err(|e| anyhow::anyhow!("Invalid glob: {}", e))?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Query parameter matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryMatcher {
    /// Exact value match
    Exact { value: String },
    /// Regex pattern match
    Regex { pattern: String },
    /// Parameter must be present (any value)
    Present,
    /// Parameter must be absent
    Absent,
}

/// Header matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeaderMatcher {
    /// Exact value match
    Exact { value: String },
    /// Regex pattern match
    Regex { pattern: String },
    /// Header must be present (any value)
    Present,
    /// Header must be absent
    Absent,
    /// Value must contain substring
    Contains { value: String },
}

/// Body matching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BodyMatcher {
    /// Exact body match
    Exact { value: String },
    /// Regex pattern match
    Regex { pattern: String },
    /// JSON path matching
    JsonPath {
        /// JSON path expressions and expected values
        expressions: HashMap<String, serde_json::Value>,
    },
    /// Body must contain substring
    Contains { value: String },
    /// Body must be valid JSON (any structure)
    Json,
    /// Body must be empty
    Empty,
}

/// Response definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseDefinition {
    /// HTTP status code
    #[serde(default = "default_status")]
    pub status: u16,

    /// Response headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Response body
    #[serde(default)]
    pub body: Option<ResponseBody>,

    /// Whether this is a template response
    #[serde(default)]
    pub template: bool,
}

fn default_status() -> u16 {
    200
}

impl ResponseDefinition {
    /// Validate the response definition.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.status < 100 || self.status > 599 {
            anyhow::bail!("Invalid status code: {}", self.status);
        }
        Ok(())
    }
}

/// Response body configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseBody {
    /// Plain text body
    Text { content: String },
    /// JSON body
    Json { content: serde_json::Value },
    /// Base64 encoded binary
    Base64 { content: String },
    /// Load from file
    File { path: String },
}

impl ResponseBody {
    /// Get the body content as bytes.
    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match self {
            ResponseBody::Text { content } => Ok(content.as_bytes().to_vec()),
            ResponseBody::Json { content } => Ok(serde_json::to_string(content)?.into_bytes()),
            ResponseBody::Base64 { content } => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(content)
                    .map_err(|e| anyhow::anyhow!("Invalid base64: {}", e))
            }
            ResponseBody::File { path } => std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", path, e)),
        }
    }

    /// Get content type for this body.
    pub fn content_type(&self) -> &'static str {
        match self {
            ResponseBody::Text { .. } => "text/plain",
            ResponseBody::Json { .. } => "application/json",
            ResponseBody::Base64 { .. } => "application/octet-stream",
            ResponseBody::File { .. } => "application/octet-stream",
        }
    }
}

/// Delay/latency simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayConfig {
    /// Fixed delay in milliseconds
    #[serde(default)]
    pub fixed_ms: u64,

    /// Minimum delay for random range (ms)
    #[serde(default)]
    pub min_ms: u64,

    /// Maximum delay for random range (ms)
    #[serde(default)]
    pub max_ms: u64,
}

impl DelayConfig {
    /// Calculate the actual delay to apply.
    pub fn calculate(&self) -> u64 {
        if self.fixed_ms > 0 {
            return self.fixed_ms;
        }
        if self.max_ms > self.min_ms {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            return rng.gen_range(self.min_ms..=self.max_ms);
        }
        self.min_ms
    }
}

/// Fault injection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FaultConfig {
    /// Return an error response
    Error {
        /// HTTP status code
        status: u16,
        /// Error message
        #[serde(default)]
        message: Option<String>,
    },
    /// Simulate connection timeout (no response)
    Timeout {
        /// Timeout duration in milliseconds
        duration_ms: u64,
    },
    /// Return empty response
    Empty,
    /// Corrupt the response
    Corrupt {
        /// Corruption probability (0.0 - 1.0)
        #[serde(default = "default_probability")]
        probability: f64,
    },
    /// Slow response (drip feed bytes)
    SlowResponse {
        /// Bytes per second
        bytes_per_second: u64,
    },
}

fn default_probability() -> f64 {
    1.0
}

/// Global settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalSettings {
    /// Log all matched stubs
    #[serde(default = "default_true")]
    pub log_matches: bool,

    /// Log unmatched requests
    #[serde(default = "default_true")]
    pub log_unmatched: bool,

    /// Pass through unmatched requests to upstream
    #[serde(default)]
    pub passthrough_unmatched: bool,

    /// Default content type for responses
    #[serde(default = "default_content_type")]
    pub default_content_type: String,

    /// Case-insensitive header matching
    #[serde(default = "default_true")]
    pub case_insensitive_headers: bool,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            log_matches: true,
            log_unmatched: true,
            passthrough_unmatched: false,
            default_content_type: default_content_type(),
            case_insensitive_headers: true,
        }
    }
}

fn default_content_type() -> String {
    "application/json".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_stub() {
        let yaml = r#"
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
        type: text
        content: "Hello, World!"
"#;
        let config: MockServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.stubs.len(), 1);
        assert_eq!(config.stubs[0].id, "hello-world");
    }

    #[test]
    fn test_parse_json_response() {
        let yaml = r#"
stubs:
  - id: json-response
    request:
      path:
        type: prefix
        value: /api
    response:
      status: 200
      headers:
        Content-Type: application/json
      body:
        type: json
        content:
          message: "success"
          code: 0
"#;
        let config: MockServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.stubs.len(), 1);

        if let Some(ResponseBody::Json { content }) = &config.stubs[0].response.body {
            assert_eq!(content["message"], "success");
        } else {
            panic!("Expected JSON body");
        }
    }

    #[test]
    fn test_parse_delay_config() {
        let yaml = r#"
stubs:
  - id: slow-response
    request:
      path:
        type: exact
        value: /slow
    response:
      status: 200
    delay:
      fixed_ms: 1000
"#;
        let config: MockServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.stubs[0].delay.as_ref().unwrap().fixed_ms, 1000);
    }

    #[test]
    fn test_parse_fault_config() {
        let yaml = r#"
stubs:
  - id: error-response
    request:
      path:
        type: exact
        value: /error
    response:
      status: 200
    fault:
      type: error
      status: 500
      message: "Internal Server Error"
"#;
        let config: MockServerConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.stubs[0].fault {
            Some(FaultConfig::Error { status, message }) => {
                assert_eq!(*status, 500);
                assert_eq!(message.as_deref(), Some("Internal Server Error"));
            }
            _ => panic!("Expected Error fault"),
        }
    }

    #[test]
    fn test_parse_path_template() {
        let yaml = r#"
stubs:
  - id: user-by-id
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
          name: "User {{path.id}}"
"#;
        let config: MockServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.stubs[0].response.template);
    }

    #[test]
    fn test_delay_calculation() {
        let fixed = DelayConfig {
            fixed_ms: 100,
            min_ms: 0,
            max_ms: 0,
        };
        assert_eq!(fixed.calculate(), 100);

        let range = DelayConfig {
            fixed_ms: 0,
            min_ms: 50,
            max_ms: 150,
        };
        let delay = range.calculate();
        assert!((50..=150).contains(&delay));
    }

    #[test]
    fn test_response_body_to_bytes() {
        let text = ResponseBody::Text {
            content: "hello".to_string(),
        };
        assert_eq!(text.to_bytes().unwrap(), b"hello");

        let json = ResponseBody::Json {
            content: serde_json::json!({"key": "value"}),
        };
        let bytes = json.to_bytes().unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains("key"));
    }
}
