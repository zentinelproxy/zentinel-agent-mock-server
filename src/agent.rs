//! Main Mock Server agent implementation.

use crate::config::{FaultConfig, MockServerConfig, ResponseBody, StubDefinition};
use crate::matcher::Matcher;
use crate::template::TemplateEngine;
use async_trait::async_trait;
use sentinel_agent_sdk::prelude::*;
use sentinel_agent_protocol::v2::{
    AgentCapabilities, AgentFeatures, AgentHandlerV2, CounterMetric, DrainReason,
    GaugeMetric, HealthStatus, MetricsReport, ShutdownReason,
};
use sentinel_agent_protocol::{AgentResponse, EventType, RequestHeadersEvent, ResponseHeadersEvent};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Mock Server Agent
///
/// Intercepts requests and returns configured stub responses
/// for testing and development purposes.
pub struct MockServerAgent {
    config: MockServerConfig,
    matcher: Matcher,
    template_engine: TemplateEngine,
    /// Match counts per stub ID
    match_counts: Arc<RwLock<HashMap<String, AtomicU32>>>,
    /// Total requests processed.
    requests_total: AtomicU64,
    /// Total requests matched to stubs.
    requests_matched: AtomicU64,
    /// Total requests unmatched.
    requests_unmatched: AtomicU64,
    /// Whether the agent is draining (not accepting new mock responses).
    draining: AtomicBool,
}

/// Flatten SDK headers (Vec<String>) to single-value HashMap
fn flatten_headers(headers: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(k, v)| v.first().map(|first| (k.clone(), first.clone())))
        .collect()
}

impl MockServerAgent {
    /// Create a new mock server agent with the given configuration.
    pub fn new(config: MockServerConfig) -> Self {
        let matcher = Matcher::new(&config.stubs);
        let template_engine = TemplateEngine::new();

        // Initialize match counts
        let mut match_counts = HashMap::new();
        for stub in &config.stubs {
            match_counts.insert(stub.id.clone(), AtomicU32::new(0));
        }

        info!(
            stubs = config.stubs.len(),
            passthrough = config.settings.passthrough_unmatched,
            "Mock server agent initialized"
        );

        Self {
            config,
            matcher,
            template_engine,
            match_counts: Arc::new(RwLock::new(match_counts)),
            requests_total: AtomicU64::new(0),
            requests_matched: AtomicU64::new(0),
            requests_unmatched: AtomicU64::new(0),
            draining: AtomicBool::new(false),
        }
    }

    /// Check if the agent is draining.
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Relaxed)
    }

    /// Get total requests processed.
    pub fn total_requests(&self) -> u64 {
        self.requests_total.load(Ordering::Relaxed)
    }

    /// Get total requests matched.
    pub fn total_matched(&self) -> u64 {
        self.requests_matched.load(Ordering::Relaxed)
    }

    /// Get total requests unmatched.
    pub fn total_unmatched(&self) -> u64 {
        self.requests_unmatched.load(Ordering::Relaxed)
    }

    /// Create from a YAML configuration string.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let config: MockServerConfig = serde_yaml::from_str(yaml)?;
        Ok(Self::new(config))
    }

    /// Check if a stub has exceeded its max matches.
    async fn is_stub_exhausted(&self, stub: &StubDefinition) -> bool {
        if stub.max_matches == 0 {
            return false; // Unlimited
        }

        let counts = self.match_counts.read().await;
        if let Some(count) = counts.get(&stub.id) {
            count.load(Ordering::Relaxed) >= stub.max_matches
        } else {
            false
        }
    }

    /// Increment the match count for a stub.
    async fn increment_match_count(&self, stub_id: &str) {
        let counts = self.match_counts.read().await;
        if let Some(count) = counts.get(stub_id) {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Build a response from a stub definition.
    async fn build_response(
        &self,
        stub: &StubDefinition,
        match_ctx: &crate::matcher::MatchContext,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Decision {
        // Check for fault injection
        if let Some(fault) = &stub.fault {
            return self.apply_fault(fault, stub).await;
        }

        // Apply delay if configured
        if let Some(delay) = &stub.delay {
            let delay_ms = delay.calculate();
            if delay_ms > 0 {
                debug!(stub_id = %stub.id, delay_ms, "Applying delay");
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }
        }

        // Build the response
        let response = &stub.response;

        // Get body content
        let body_content = if let Some(body_def) = &response.body {
            if response.template {
                // Render template
                self.render_template_body(body_def, match_ctx, method, path, headers, body)
            } else {
                // Static body
                body_def.to_bytes().ok()
            }
        } else {
            None
        };

        // Determine content type
        let content_type = response
            .headers
            .get("content-type")
            .or_else(|| response.headers.get("Content-Type"))
            .cloned()
            .unwrap_or_else(|| {
                response
                    .body
                    .as_ref()
                    .map(|b| b.content_type().to_string())
                    .unwrap_or_else(|| self.config.settings.default_content_type.clone())
            });

        // Build decision
        let mut decision = Decision::block(response.status)
            .with_block_header("Content-Type", &content_type)
            .with_tag("mocked")
            .with_metadata("stub_id", serde_json::json!(stub.id));

        // Add response headers
        for (name, value) in &response.headers {
            if name.to_lowercase() != "content-type" {
                decision = decision.with_block_header(name, value);
            }
        }

        // Add body
        if let Some(content) = body_content {
            decision = decision.with_body(String::from_utf8_lossy(&content).to_string());
        }

        decision
    }

    /// Render a template body.
    fn render_template_body(
        &self,
        body_def: &ResponseBody,
        match_ctx: &crate::matcher::MatchContext,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        match body_def {
            ResponseBody::Text { content } => {
                self.template_engine
                    .render(content, match_ctx, method, path, headers, body)
                    .ok()
                    .map(|s| s.into_bytes())
            }
            ResponseBody::Json { content } => {
                self.template_engine
                    .render_json(content, match_ctx, method, path, headers, body)
                    .ok()
                    .and_then(|v| serde_json::to_vec(&v).ok())
            }
            _ => body_def.to_bytes().ok(),
        }
    }

    /// Apply fault injection.
    async fn apply_fault(&self, fault: &FaultConfig, stub: &StubDefinition) -> Decision {
        match fault {
            FaultConfig::Error { status, message } => {
                let body = message.clone().unwrap_or_else(|| "Error".to_string());
                Decision::block(*status)
                    .with_body(body)
                    .with_block_header("Content-Type", "text/plain")
                    .with_tag("mocked")
                    .with_tag("fault_injected")
                    .with_metadata("stub_id", serde_json::json!(stub.id))
                    .with_metadata("fault_type", serde_json::json!("error"))
            }

            FaultConfig::Timeout { duration_ms } => {
                debug!(
                    stub_id = %stub.id,
                    duration_ms,
                    "Simulating timeout"
                );
                // Sleep for the timeout duration
                tokio::time::sleep(tokio::time::Duration::from_millis(*duration_ms)).await;

                // Return a gateway timeout
                Decision::block(504)
                    .with_body("Gateway Timeout (simulated)")
                    .with_block_header("Content-Type", "text/plain")
                    .with_tag("mocked")
                    .with_tag("fault_injected")
                    .with_metadata("stub_id", serde_json::json!(stub.id))
                    .with_metadata("fault_type", serde_json::json!("timeout"))
            }

            FaultConfig::Empty => {
                Decision::block(200)
                    .with_body("")
                    .with_tag("mocked")
                    .with_tag("fault_injected")
                    .with_metadata("stub_id", serde_json::json!(stub.id))
                    .with_metadata("fault_type", serde_json::json!("empty"))
            }

            FaultConfig::Corrupt { probability } => {
                use rand::Rng;
                let should_corrupt = {
                    let mut rng = rand::thread_rng();
                    rng.gen::<f64>() < *probability
                };

                if should_corrupt {
                    // Return corrupted response
                    Decision::block(200)
                        .with_body(generate_garbage())
                        .with_block_header("Content-Type", "application/octet-stream")
                        .with_tag("mocked")
                        .with_tag("fault_injected")
                        .with_metadata("stub_id", serde_json::json!(stub.id))
                        .with_metadata("fault_type", serde_json::json!("corrupt"))
                } else {
                    // Return normal response
                    self.build_normal_response(stub).await
                }
            }

            FaultConfig::SlowResponse { bytes_per_second } => {
                // For now, just simulate with a delay
                // A real implementation would drip-feed the response
                let body_size = stub
                    .response
                    .body
                    .as_ref()
                    .and_then(|b| b.to_bytes().ok())
                    .map(|b| b.len())
                    .unwrap_or(100);

                let delay_ms = (body_size as u64 * 1000) / (*bytes_per_second).max(1);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;

                self.build_normal_response(stub).await
            }
        }
    }

    /// Build a normal response (no fault injection).
    async fn build_normal_response(&self, stub: &StubDefinition) -> Decision {
        let response = &stub.response;

        let body_content = response
            .body
            .as_ref()
            .and_then(|b| b.to_bytes().ok());

        let content_type = response
            .headers
            .get("content-type")
            .or_else(|| response.headers.get("Content-Type"))
            .cloned()
            .unwrap_or_else(|| {
                response
                    .body
                    .as_ref()
                    .map(|b| b.content_type().to_string())
                    .unwrap_or_else(|| self.config.settings.default_content_type.clone())
            });

        let mut decision = Decision::block(response.status)
            .with_block_header("Content-Type", &content_type)
            .with_tag("mocked")
            .with_metadata("stub_id", serde_json::json!(stub.id));

        for (name, value) in &response.headers {
            if name.to_lowercase() != "content-type" {
                decision = decision.with_block_header(name, value);
            }
        }

        if let Some(content) = body_content {
            decision = decision.with_body(String::from_utf8_lossy(&content).to_string());
        }

        decision
    }

    /// Build a default response for unmatched requests.
    fn build_default_response(&self) -> Decision {
        if let Some(default) = &self.config.default_response {
            let body_content = default
                .body
                .as_ref()
                .and_then(|b| b.to_bytes().ok());

            let content_type = default
                .headers
                .get("content-type")
                .or_else(|| default.headers.get("Content-Type"))
                .cloned()
                .unwrap_or_else(|| self.config.settings.default_content_type.clone());

            let mut decision = Decision::block(default.status)
                .with_block_header("Content-Type", &content_type)
                .with_tag("mocked")
                .with_tag("default_response");

            for (name, value) in &default.headers {
                if name.to_lowercase() != "content-type" {
                    decision = decision.with_block_header(name, value);
                }
            }

            if let Some(content) = body_content {
                decision = decision.with_body(String::from_utf8_lossy(&content).to_string());
            }

            decision
        } else {
            // No default configured, return 404
            Decision::block(404)
                .with_body(r#"{"error": "not_found", "message": "No matching stub found"}"#)
                .with_block_header("Content-Type", "application/json")
                .with_tag("mocked")
                .with_tag("not_found")
        }
    }
}

/// Generate random garbage data for corruption simulation.
fn generate_garbage() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let len = rng.gen_range(50..200);
    (0..len).map(|_| rng.gen_range(0x20..0x7e) as u8 as char).collect()
}

// The agent needs to be Send + Sync for the SDK
unsafe impl Send for MockServerAgent {}
unsafe impl Sync for MockServerAgent {}

#[async_trait]
impl Agent for MockServerAgent {
    fn name(&self) -> &str {
        "mock-server"
    }

    async fn on_request(&self, request: &Request) -> Decision {
        // Increment request counter
        self.requests_total.fetch_add(1, Ordering::Relaxed);

        // Check if draining - don't mock, pass through
        if self.is_draining() {
            debug!("Agent is draining, passing through request");
            return Decision::allow();
        }

        let method = request.method();
        let path = request.path();
        let query_string = request.query_string();
        let headers = flatten_headers(request.headers());
        let body = request.body();

        // Find matching stub
        let match_result = self.matcher.find_match(
            &self.config.stubs,
            method,
            path,
            query_string,
            &headers,
            body,
        );

        match match_result {
            Some(result) => {
                // Check if stub is exhausted
                if self.is_stub_exhausted(result.stub).await {
                    self.requests_unmatched.fetch_add(1, Ordering::Relaxed);
                    if self.config.settings.log_unmatched {
                        info!(
                            stub_id = %result.stub.id,
                            path = %path,
                            "Stub exhausted (max_matches reached)"
                        );
                    }
                    return if self.config.settings.passthrough_unmatched {
                        Decision::allow()
                    } else {
                        self.build_default_response()
                    };
                }

                // Increment counters
                self.requests_matched.fetch_add(1, Ordering::Relaxed);
                self.increment_match_count(&result.stub.id).await;

                if self.config.settings.log_matches {
                    info!(
                        stub_id = %result.stub.id,
                        method = %method,
                        path = %path,
                        "Request matched stub"
                    );
                }

                // Build and return response
                self.build_response(
                    result.stub,
                    &result.context,
                    method,
                    path,
                    &headers,
                    body,
                )
                .await
            }
            None => {
                self.requests_unmatched.fetch_add(1, Ordering::Relaxed);
                if self.config.settings.log_unmatched {
                    warn!(
                        method = %method,
                        path = %path,
                        "No matching stub found"
                    );
                }

                if self.config.settings.passthrough_unmatched {
                    Decision::allow()
                } else {
                    self.build_default_response()
                }
            }
        }
    }

    async fn on_response(&self, _request: &Request, _response: &Response) -> Decision {
        // Response phase - nothing to do for mock server
        Decision::allow()
    }

    async fn on_configure(&self, config: serde_json::Value) -> Result<(), String> {
        // v2 configuration update support
        if config.is_null() {
            return Ok(());
        }

        info!(config = %config, "Received configuration update");
        // For now, we acknowledge the config - full hot-reload would require
        // more complex state management
        Ok(())
    }
}

/// v2 Protocol implementation for MockServerAgent.
#[async_trait]
impl AgentHandlerV2 for MockServerAgent {
    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::new("mock-server", "Mock Server Agent", env!("CARGO_PKG_VERSION"))
            .with_event(EventType::RequestHeaders)
            .with_features(AgentFeatures {
                config_push: true,
                health_reporting: true,
                metrics_export: true,
                concurrent_requests: 100,
                cancellation: true,
                max_processing_time_ms: 5000,
            })
    }

    fn health_status(&self) -> HealthStatus {
        // Report healthy unless we're draining
        if self.is_draining() {
            HealthStatus::degraded(
                "mock-server",
                vec!["stubbing".to_string()],
                1.0,
            )
        } else {
            HealthStatus::healthy("mock-server")
        }
    }

    fn metrics_report(&self) -> Option<MetricsReport> {
        let mut report = MetricsReport::new("mock-server", 10_000);

        // Add counter metrics
        report.counters.push(CounterMetric::new(
            "mock_server_requests_total",
            self.total_requests(),
        ));

        report.counters.push(CounterMetric::new(
            "mock_server_requests_matched_total",
            self.total_matched(),
        ));

        report.counters.push(CounterMetric::new(
            "mock_server_requests_unmatched_total",
            self.total_unmatched(),
        ));

        // Add gauge metrics
        report.gauges.push(GaugeMetric::new(
            "mock_server_stubs_configured",
            self.config.stubs.len() as f64,
        ));

        report.gauges.push(GaugeMetric::new(
            "mock_server_stubs_enabled",
            self.config.stubs.iter().filter(|s| s.enabled).count() as f64,
        ));

        report.gauges.push(GaugeMetric::new(
            "mock_server_agent_draining",
            if self.is_draining() { 1.0 } else { 0.0 },
        ));

        Some(report)
    }

    async fn on_shutdown(&self, reason: ShutdownReason, grace_period_ms: u64) {
        info!(
            reason = ?reason,
            grace_period_ms = grace_period_ms,
            "Mock server agent shutdown requested"
        );
        // Set draining flag to stop mocking new requests
        self.draining.store(true, Ordering::SeqCst);
    }

    async fn on_drain(&self, duration_ms: u64, reason: DrainReason) {
        warn!(
            reason = ?reason,
            duration_ms = duration_ms,
            "Mock server agent drain requested - stopping stub matching"
        );
        self.draining.store(true, Ordering::SeqCst);
    }

    fn on_stream_closed(&self) {
        debug!("gRPC stream closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MockServerConfig {
        let yaml = r#"
stubs:
  - id: hello
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

  - id: error-endpoint
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

  - id: delayed-endpoint
    request:
      path:
        type: exact
        value: /slow
    response:
      status: 200
      body:
        type: text
        content: "Delayed response"
    delay:
      fixed_ms: 100

settings:
  passthrough_unmatched: false
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn test_agent_creation() {
        let config = test_config();
        let agent = MockServerAgent::new(config);
        assert_eq!(agent.config.stubs.len(), 4);
    }

    #[tokio::test]
    async fn test_simple_match() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        // Create a mock request (we'll test the matcher directly)
        let headers = HashMap::new();
        let match_result = agent.matcher.find_match(
            &agent.config.stubs,
            "GET",
            "/hello",
            None,
            &headers,
            None,
        );

        assert!(match_result.is_some());
        assert_eq!(match_result.unwrap().stub.id, "hello");
    }

    #[tokio::test]
    async fn test_template_match() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        let headers = HashMap::new();
        let match_result = agent.matcher.find_match(
            &agent.config.stubs,
            "GET",
            "/users/123",
            None,
            &headers,
            None,
        );

        assert!(match_result.is_some());
        let result = match_result.unwrap();
        assert_eq!(result.stub.id, "user-by-id");
        assert_eq!(result.context.path_params.get("id"), Some(&"123".to_string()));
    }

    #[tokio::test]
    async fn test_no_match() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        let headers = HashMap::new();
        let match_result = agent.matcher.find_match(
            &agent.config.stubs,
            "GET",
            "/nonexistent",
            None,
            &headers,
            None,
        );

        assert!(match_result.is_none());
    }

    #[tokio::test]
    async fn test_max_matches() {
        let mut config = test_config();
        config.stubs[0].max_matches = 2;

        let agent = MockServerAgent::new(config);

        // First two matches should work
        for _ in 0..2 {
            let headers = HashMap::new();
            let match_result = agent.matcher.find_match(
                &agent.config.stubs,
                "GET",
                "/hello",
                None,
                &headers,
                None,
            );
            assert!(match_result.is_some());
            agent.increment_match_count("hello").await;
        }

        // Third match - stub should be exhausted
        assert!(agent.is_stub_exhausted(&agent.config.stubs[0]).await);
    }

    #[test]
    fn test_v2_capabilities() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        let caps = agent.capabilities();
        assert_eq!(caps.agent_id, "mock-server");
        assert_eq!(caps.name, "Mock Server Agent");
        assert!(caps.features.config_push);
        assert!(caps.features.health_reporting);
        assert!(caps.features.metrics_export);
        assert_eq!(caps.features.concurrent_requests, 100);
    }

    #[test]
    fn test_v2_health_status() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        // Should be healthy initially
        let health = agent.health_status();
        assert!(health.is_healthy());
        assert_eq!(health.agent_id, "mock-server");
    }

    #[tokio::test]
    async fn test_v2_health_status_draining() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        // Trigger drain
        agent.on_drain(5000, DrainReason::Maintenance).await;

        // Should be degraded now
        let health = agent.health_status();
        assert!(!health.is_healthy());
    }

    #[test]
    fn test_v2_metrics_report() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        let report = agent.metrics_report();
        assert!(report.is_some());

        let report = report.unwrap();
        assert_eq!(report.agent_id, "mock-server");
        assert!(!report.counters.is_empty());
        assert!(!report.gauges.is_empty());
    }

    #[tokio::test]
    async fn test_draining_flag() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        assert!(!agent.is_draining());

        agent.on_shutdown(ShutdownReason::Graceful, 30000).await;

        assert!(agent.is_draining());
    }

    #[test]
    fn test_request_counters() {
        let config = test_config();
        let agent = MockServerAgent::new(config);

        assert_eq!(agent.total_requests(), 0);
        assert_eq!(agent.total_matched(), 0);
        assert_eq!(agent.total_unmatched(), 0);

        // Simulate incrementing (in real usage this happens in on_request)
        agent.requests_total.fetch_add(1, Ordering::Relaxed);
        agent.requests_matched.fetch_add(1, Ordering::Relaxed);

        assert_eq!(agent.total_requests(), 1);
        assert_eq!(agent.total_matched(), 1);
    }
}
