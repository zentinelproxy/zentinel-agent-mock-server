//! Request matching logic.
//!
//! Matches incoming requests against stub definitions.

use crate::config::{
    BodyMatcher, HeaderMatcher, PathMatcher, QueryMatcher, RequestMatcher, StubDefinition,
};
use regex::Regex;
use std::collections::HashMap;

/// Context captured during matching (for template variables).
#[derive(Debug, Clone, Default)]
pub struct MatchContext {
    /// Path parameters extracted from template matching
    pub path_params: HashMap<String, String>,
    /// Query parameters
    pub query_params: HashMap<String, String>,
    /// Regex capture groups
    pub captures: HashMap<String, String>,
}

/// Result of matching a request against stubs.
#[derive(Debug)]
pub struct MatchResult<'a> {
    /// The matched stub
    pub stub: &'a StubDefinition,
    /// Context captured during matching
    pub context: MatchContext,
}

/// Request matcher engine.
pub struct Matcher {
    /// Compiled path matchers (Option because path matcher is optional per stub)
    path_matchers: Vec<Option<CompiledPathMatcher>>,
}

enum CompiledPathMatcher {
    Exact(String),
    Prefix(String),
    Regex(Regex),
    Glob(globset::GlobMatcher),
    Template(PathTemplate),
}

struct PathTemplate {
    segments: Vec<TemplateSegment>,
}

enum TemplateSegment {
    Literal(String),
    Param(String),
}

impl PathTemplate {
    fn parse(template: &str) -> Self {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut in_param = false;
        let mut param_name = String::new();

        for ch in template.chars() {
            if ch == '{' && !in_param {
                if !current.is_empty() {
                    segments.push(TemplateSegment::Literal(current.clone()));
                    current.clear();
                }
                in_param = true;
                param_name.clear();
            } else if ch == '}' && in_param {
                segments.push(TemplateSegment::Param(param_name.clone()));
                in_param = false;
                param_name.clear();
            } else if in_param {
                param_name.push(ch);
            } else {
                current.push(ch);
            }
        }

        if !current.is_empty() {
            segments.push(TemplateSegment::Literal(current));
        }

        Self { segments }
    }

    fn matches(&self, path: &str) -> Option<HashMap<String, String>> {
        let mut params = HashMap::new();
        let mut remaining = path;

        for segment in &self.segments {
            match segment {
                TemplateSegment::Literal(lit) => {
                    if remaining.starts_with(lit) {
                        remaining = &remaining[lit.len()..];
                    } else {
                        return None;
                    }
                }
                TemplateSegment::Param(name) => {
                    // Find the next literal or end of string
                    let end_pos = if let Some(next_segment) = self.segments.iter().skip_while(|s| {
                        !matches!(s, TemplateSegment::Literal(_))
                    }).next() {
                        if let TemplateSegment::Literal(next_lit) = next_segment {
                            remaining.find(next_lit.as_str()).unwrap_or(remaining.len())
                        } else {
                            remaining.len()
                        }
                    } else {
                        // Find next slash or end
                        remaining.find('/').unwrap_or(remaining.len())
                    };

                    if end_pos == 0 {
                        return None;
                    }

                    let value = &remaining[..end_pos];
                    params.insert(name.clone(), value.to_string());
                    remaining = &remaining[end_pos..];
                }
            }
        }

        // Must consume entire path
        if remaining.is_empty() {
            Some(params)
        } else {
            None
        }
    }
}

impl Matcher {
    /// Create a new matcher from stub definitions.
    pub fn new(stubs: &[StubDefinition]) -> Self {
        let path_matchers = stubs
            .iter()
            .map(|stub| {
                stub.request.path.as_ref().map(|p| match p {
                    PathMatcher::Exact { value } => CompiledPathMatcher::Exact(value.clone()),
                    PathMatcher::Prefix { value } => CompiledPathMatcher::Prefix(value.clone()),
                    PathMatcher::Regex { pattern } => {
                        CompiledPathMatcher::Regex(Regex::new(pattern).unwrap())
                    }
                    PathMatcher::Glob { pattern } => {
                        let glob = globset::Glob::new(pattern).unwrap();
                        CompiledPathMatcher::Glob(glob.compile_matcher())
                    }
                    PathMatcher::Template { template } => {
                        CompiledPathMatcher::Template(PathTemplate::parse(template))
                    }
                })
            })
            .collect();

        Self { path_matchers }
    }

    /// Find the first matching stub for a request.
    pub fn find_match<'a>(
        &self,
        stubs: &'a [StubDefinition],
        method: &str,
        path: &str,
        query_string: Option<&str>,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Option<MatchResult<'a>> {
        // Sort by priority (highest first)
        let mut indexed_stubs: Vec<_> = stubs.iter().enumerate().collect();
        indexed_stubs.sort_by(|a, b| b.1.priority.cmp(&a.1.priority));

        for (idx, stub) in indexed_stubs {
            if !stub.enabled {
                continue;
            }

            if let Some(context) = self.matches_request(
                idx,
                &stub.request,
                method,
                path,
                query_string,
                headers,
                body,
            ) {
                return Some(MatchResult { stub, context });
            }
        }

        None
    }

    fn matches_request(
        &self,
        stub_idx: usize,
        matcher: &RequestMatcher,
        method: &str,
        path: &str,
        query_string: Option<&str>,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Option<MatchContext> {
        let mut context = MatchContext::default();

        // Check method
        if !matcher.method.is_empty() {
            let method_upper = method.to_uppercase();
            if !matcher.method.iter().any(|m| m.to_uppercase() == method_upper) {
                return None;
            }
        }

        // Check path
        if let Some(Some(path_matcher)) = self.path_matchers.get(stub_idx) {
            if !self.matches_path(path_matcher, path, &mut context) {
                return None;
            }
        }

        // Parse query string
        let query_params = parse_query_string(query_string.unwrap_or(""));
        context.query_params = query_params.clone();

        // Check query parameters
        for (name, qm) in &matcher.query {
            if !self.matches_query(&query_params, name, qm) {
                return None;
            }
        }

        // Check headers
        for (name, hm) in &matcher.headers {
            if !self.matches_header(headers, name, hm) {
                return None;
            }
        }

        // Check body
        if let Some(bm) = &matcher.body {
            if !self.matches_body(body, bm) {
                return None;
            }
        }

        Some(context)
    }

    fn matches_path(
        &self,
        matcher: &CompiledPathMatcher,
        path: &str,
        context: &mut MatchContext,
    ) -> bool {
        match matcher {
            CompiledPathMatcher::Exact(value) => path == value,
            CompiledPathMatcher::Prefix(value) => path.starts_with(value),
            CompiledPathMatcher::Regex(regex) => {
                if let Some(captures) = regex.captures(path) {
                    for (i, cap) in captures.iter().enumerate().skip(1) {
                        if let Some(m) = cap {
                            context.captures.insert(format!("{}", i), m.as_str().to_string());
                        }
                    }
                    // Also add named captures
                    for name in regex.capture_names().flatten() {
                        if let Some(m) = captures.name(name) {
                            context.captures.insert(name.to_string(), m.as_str().to_string());
                        }
                    }
                    true
                } else {
                    false
                }
            }
            CompiledPathMatcher::Glob(glob) => glob.is_match(path),
            CompiledPathMatcher::Template(template) => {
                if let Some(params) = template.matches(path) {
                    context.path_params = params;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn matches_query(
        &self,
        query_params: &HashMap<String, String>,
        name: &str,
        matcher: &QueryMatcher,
    ) -> bool {
        match matcher {
            QueryMatcher::Exact { value } => query_params.get(name) == Some(value),
            QueryMatcher::Regex { pattern } => {
                if let Some(val) = query_params.get(name) {
                    if let Ok(regex) = Regex::new(pattern) {
                        return regex.is_match(val);
                    }
                }
                false
            }
            QueryMatcher::Present => query_params.contains_key(name),
            QueryMatcher::Absent => !query_params.contains_key(name),
        }
    }

    fn matches_header(
        &self,
        headers: &HashMap<String, String>,
        name: &str,
        matcher: &HeaderMatcher,
    ) -> bool {
        // Case-insensitive header lookup
        let header_value = headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name.to_lowercase())
            .map(|(_, v)| v);

        match matcher {
            HeaderMatcher::Exact { value } => header_value == Some(value),
            HeaderMatcher::Regex { pattern } => {
                if let Some(val) = header_value {
                    if let Ok(regex) = Regex::new(pattern) {
                        return regex.is_match(val);
                    }
                }
                false
            }
            HeaderMatcher::Present => header_value.is_some(),
            HeaderMatcher::Absent => header_value.is_none(),
            HeaderMatcher::Contains { value } => {
                header_value.map(|v| v.contains(value)).unwrap_or(false)
            }
        }
    }

    fn matches_body(&self, body: Option<&[u8]>, matcher: &BodyMatcher) -> bool {
        let body_str = body.and_then(|b| std::str::from_utf8(b).ok());

        match matcher {
            BodyMatcher::Exact { value } => body_str == Some(value.as_str()),
            BodyMatcher::Regex { pattern } => {
                if let Some(bs) = body_str {
                    if let Ok(regex) = Regex::new(pattern) {
                        return regex.is_match(bs);
                    }
                }
                false
            }
            BodyMatcher::JsonPath { expressions } => {
                if let Some(bs) = body_str {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(bs) {
                        return self.matches_json_paths(&json, expressions);
                    }
                }
                false
            }
            BodyMatcher::Contains { value } => {
                body_str.map(|bs| bs.contains(value)).unwrap_or(false)
            }
            BodyMatcher::Json => {
                body_str
                    .map(|bs| serde_json::from_str::<serde_json::Value>(bs).is_ok())
                    .unwrap_or(false)
            }
            BodyMatcher::Empty => body.map(|b| b.is_empty()).unwrap_or(true),
        }
    }

    fn matches_json_paths(
        &self,
        json: &serde_json::Value,
        expressions: &HashMap<String, serde_json::Value>,
    ) -> bool {
        use jsonpath_rust::JsonPath;

        for (path_expr, expected) in expressions {
            let path = match JsonPath::try_from(path_expr.as_str()) {
                Ok(p) => p,
                Err(_) => return false,
            };

            let results = path.find(json);

            // Check if any result matches the expected value
            // If expected is null, just check that the path exists (returns non-null)
            let matches = if expected.is_null() {
                // Check if the path resolved to something
                !results.is_null()
            } else {
                // Compare results to expected
                results == *expected
            };
            if !matches {
                return false;
            }
        }
        true
    }
}

/// Parse a query string into key-value pairs.
fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();

    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        if let Some((key, value)) = part.split_once('=') {
            params.insert(
                urlencoding_decode(key),
                urlencoding_decode(value),
            );
        } else {
            params.insert(urlencoding_decode(part), String::new());
        }
    }

    params
}

/// Simple URL decoding.
fn urlencoding_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            result.push('%');
            result.push_str(&hex);
        } else if ch == '+' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ResponseDefinition;

    fn make_stub(id: &str, path: PathMatcher) -> StubDefinition {
        StubDefinition {
            id: id.to_string(),
            name: None,
            request: RequestMatcher {
                method: vec![],
                path: Some(path),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: None,
            },
            response: ResponseDefinition {
                status: 200,
                headers: HashMap::new(),
                body: None,
                template: false,
            },
            priority: 0,
            enabled: true,
            max_matches: 0,
            delay: None,
            fault: None,
        }
    }

    #[test]
    fn test_exact_path_matching() {
        let stubs = vec![make_stub(
            "exact",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        )];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_some());

        let result = matcher.find_match(&stubs, "GET", "/api/posts", None, &HashMap::new(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_prefix_path_matching() {
        let stubs = vec![make_stub(
            "prefix",
            PathMatcher::Prefix {
                value: "/api/".to_string(),
            },
        )];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_some());

        let result = matcher.find_match(&stubs, "GET", "/api/posts/123", None, &HashMap::new(), None);
        assert!(result.is_some());

        let result = matcher.find_match(&stubs, "GET", "/other", None, &HashMap::new(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_template_path_matching() {
        let stubs = vec![make_stub(
            "template",
            PathMatcher::Template {
                template: "/users/{id}".to_string(),
            },
        )];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(&stubs, "GET", "/users/123", None, &HashMap::new(), None);
        assert!(result.is_some());
        let ctx = result.unwrap().context;
        assert_eq!(ctx.path_params.get("id"), Some(&"123".to_string()));

        let result = matcher.find_match(&stubs, "GET", "/users/", None, &HashMap::new(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_method_matching() {
        let mut stub = make_stub(
            "method",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        );
        stub.request.method = vec!["GET".to_string(), "POST".to_string()];

        let stubs = vec![stub];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_some());

        let result = matcher.find_match(&stubs, "DELETE", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_query_matching() {
        let mut stub = make_stub(
            "query",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        );
        stub.request.query.insert(
            "page".to_string(),
            QueryMatcher::Exact {
                value: "1".to_string(),
            },
        );

        let stubs = vec![stub];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(
            &stubs,
            "GET",
            "/api/users",
            Some("page=1"),
            &HashMap::new(),
            None,
        );
        assert!(result.is_some());

        let result = matcher.find_match(
            &stubs,
            "GET",
            "/api/users",
            Some("page=2"),
            &HashMap::new(),
            None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_header_matching() {
        let mut stub = make_stub(
            "header",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        );
        stub.request.headers.insert(
            "authorization".to_string(),
            HeaderMatcher::Present,
        );

        let stubs = vec![stub];
        let matcher = Matcher::new(&stubs);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &headers, None);
        assert!(result.is_some());

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_none());
    }

    #[test]
    fn test_priority_matching() {
        let mut stub1 = make_stub(
            "low-priority",
            PathMatcher::Prefix {
                value: "/api/".to_string(),
            },
        );
        stub1.priority = 0;

        let mut stub2 = make_stub(
            "high-priority",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        );
        stub2.priority = 10;

        let stubs = vec![stub1, stub2];
        let matcher = Matcher::new(&stubs);

        let result = matcher.find_match(&stubs, "GET", "/api/users", None, &HashMap::new(), None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().stub.id, "high-priority");
    }

    #[test]
    fn test_body_json_matching() {
        let mut stub = make_stub(
            "json-body",
            PathMatcher::Exact {
                value: "/api/users".to_string(),
            },
        );
        stub.request.body = Some(BodyMatcher::Json);

        let stubs = vec![stub];
        let matcher = Matcher::new(&stubs);

        let body = br#"{"name": "John"}"#;
        let result = matcher.find_match(&stubs, "POST", "/api/users", None, &HashMap::new(), Some(body));
        assert!(result.is_some());

        let body = b"not json";
        let result = matcher.find_match(&stubs, "POST", "/api/users", None, &HashMap::new(), Some(body));
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_query_string() {
        let params = parse_query_string("foo=bar&baz=qux");
        assert_eq!(params.get("foo"), Some(&"bar".to_string()));
        assert_eq!(params.get("baz"), Some(&"qux".to_string()));

        let params = parse_query_string("name=John%20Doe");
        assert_eq!(params.get("name"), Some(&"John Doe".to_string()));
    }
}
