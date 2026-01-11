//! Sentinel Mock Server Agent
//!
//! A mock server agent for Sentinel that intercepts requests and returns
//! configurable stub responses. Perfect for testing, development, and demos.
//!
//! # Features
//!
//! - **Request Matching**: Match by path, method, headers, query params, body
//! - **Static Responses**: Return fixed responses for matched requests
//! - **Dynamic Templates**: Use Handlebars templates for dynamic responses
//! - **Latency Simulation**: Add fixed or random delays
//! - **Failure Injection**: Simulate errors, timeouts, and corrupted responses
//! - **Match Limits**: Limit how many times a stub can be matched
//!
//! # Example Configuration
//!
//! ```yaml
//! stubs:
//!   - id: hello-world
//!     request:
//!       method: [GET]
//!       path:
//!         type: exact
//!         value: /hello
//!     response:
//!       status: 200
//!       body:
//!         type: json
//!         content:
//!           message: "Hello, World!"
//! ```

pub mod agent;
pub mod config;
pub mod matcher;
pub mod template;

pub use agent::MockServerAgent;
pub use config::MockServerConfig;
