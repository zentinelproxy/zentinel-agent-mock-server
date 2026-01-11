//! Template engine for dynamic responses.
//!
//! Uses Handlebars for template rendering with request context.

use crate::matcher::MatchContext;
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::HashMap;

/// Template engine for rendering dynamic responses.
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
}

/// Context for template rendering.
#[derive(Debug, Serialize)]
pub struct TemplateContext {
    /// Path parameters from URL template matching
    pub path: HashMap<String, String>,
    /// Query parameters
    pub query: HashMap<String, String>,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Regex capture groups
    pub captures: HashMap<String, String>,
    /// Request method
    pub method: String,
    /// Request path
    pub request_path: String,
    /// Request body (as string, if text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Request body as JSON (if parseable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
}

impl TemplateEngine {
    /// Create a new template engine.
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        // Register custom helpers
        handlebars.register_helper("json", Box::new(json_helper));
        handlebars.register_helper("uuid", Box::new(uuid_helper));
        handlebars.register_helper("now", Box::new(now_helper));
        handlebars.register_helper("random", Box::new(random_helper));
        handlebars.register_helper("default", Box::new(default_helper));
        handlebars.register_helper("upper", Box::new(upper_helper));
        handlebars.register_helper("lower", Box::new(lower_helper));

        // Don't escape HTML by default (we're not rendering HTML)
        handlebars.register_escape_fn(handlebars::no_escape);

        Self { handlebars }
    }

    /// Render a template string with the given context.
    pub fn render(
        &self,
        template: &str,
        match_ctx: &MatchContext,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Result<String, handlebars::RenderError> {
        let body_str = body.and_then(|b| std::str::from_utf8(b).ok()).map(String::from);
        let json_body = body_str
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok());

        let ctx = TemplateContext {
            path: match_ctx.path_params.clone(),
            query: match_ctx.query_params.clone(),
            headers: headers.clone(),
            captures: match_ctx.captures.clone(),
            method: method.to_string(),
            request_path: path.to_string(),
            body: body_str,
            json: json_body,
        };

        self.handlebars.render_template(template, &ctx)
    }

    /// Render a JSON value with templates in string fields.
    pub fn render_json(
        &self,
        json: &serde_json::Value,
        match_ctx: &MatchContext,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        body: Option<&[u8]>,
    ) -> Result<serde_json::Value, handlebars::RenderError> {
        let body_str = body.and_then(|b| std::str::from_utf8(b).ok()).map(String::from);
        let json_body = body_str
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok());

        let ctx = TemplateContext {
            path: match_ctx.path_params.clone(),
            query: match_ctx.query_params.clone(),
            headers: headers.clone(),
            captures: match_ctx.captures.clone(),
            method: method.to_string(),
            request_path: path.to_string(),
            body: body_str,
            json: json_body,
        };

        self.render_json_value(json, &ctx)
    }

    fn render_json_value(
        &self,
        value: &serde_json::Value,
        ctx: &TemplateContext,
    ) -> Result<serde_json::Value, handlebars::RenderError> {
        match value {
            serde_json::Value::String(s) => {
                // Check if it contains template syntax
                if s.contains("{{") {
                    let rendered = self.handlebars.render_template(s, ctx)?;
                    Ok(serde_json::Value::String(rendered))
                } else {
                    Ok(value.clone())
                }
            }
            serde_json::Value::Array(arr) => {
                let rendered: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|v| self.render_json_value(v, ctx))
                    .collect();
                Ok(serde_json::Value::Array(rendered?))
            }
            serde_json::Value::Object(obj) => {
                let mut rendered = serde_json::Map::new();
                for (k, v) in obj {
                    rendered.insert(k.clone(), self.render_json_value(v, ctx)?);
                }
                Ok(serde_json::Value::Object(rendered))
            }
            _ => Ok(value.clone()),
        }
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

// Custom Handlebars helpers

fn json_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    let param = h.param(0).and_then(|v| v.value().as_str()).unwrap_or("");
    // Pretty print JSON
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(param) {
        out.write(&serde_json::to_string_pretty(&json).unwrap_or_default())?;
    } else {
        out.write(param)?;
    }
    Ok(())
}

fn uuid_helper(
    _: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let uuid = format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        rng.gen::<u32>(),
        rng.gen::<u16>(),
        rng.gen::<u16>() & 0x0fff,
        (rng.gen::<u16>() & 0x3fff) | 0x8000,
        rng.gen::<u64>() & 0xffffffffffff,
    );
    out.write(&uuid)?;
    Ok(())
}

fn now_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    use chrono::Utc;

    let format = h
        .param(0)
        .and_then(|v| v.value().as_str())
        .unwrap_or("%Y-%m-%dT%H:%M:%S%.3fZ");

    let now = Utc::now();
    out.write(&now.format(format).to_string())?;
    Ok(())
}

fn random_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    use rand::Rng;

    let min = h
        .param(0)
        .and_then(|v| v.value().as_i64())
        .unwrap_or(0);
    let max = h
        .param(1)
        .and_then(|v| v.value().as_i64())
        .unwrap_or(100);

    let mut rng = rand::thread_rng();
    let value = rng.gen_range(min..=max);
    out.write(&value.to_string())?;
    Ok(())
}

fn default_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    let value = h.param(0).map(|v| v.value());
    let default = h.param(1).and_then(|v| v.value().as_str()).unwrap_or("");

    match value {
        Some(v) if !v.is_null() => {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    out.write(s)?;
                    return Ok(());
                }
            } else {
                out.write(&v.to_string())?;
                return Ok(());
            }
        }
        _ => {}
    }

    out.write(default)?;
    Ok(())
}

fn upper_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    let value = h.param(0).and_then(|v| v.value().as_str()).unwrap_or("");
    out.write(&value.to_uppercase())?;
    Ok(())
}

fn lower_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    let value = h.param(0).and_then(|v| v.value().as_str()).unwrap_or("");
    out.write(&value.to_lowercase())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_template() {
        let engine = TemplateEngine::new();
        let mut ctx = MatchContext::default();
        ctx.path_params.insert("id".to_string(), "123".to_string());

        let result = engine
            .render("User ID: {{path.id}}", &ctx, "GET", "/users/123", &HashMap::new(), None)
            .unwrap();

        assert_eq!(result, "User ID: 123");
    }

    #[test]
    fn test_query_params() {
        let engine = TemplateEngine::new();
        let mut ctx = MatchContext::default();
        ctx.query_params.insert("page".to_string(), "1".to_string());

        let result = engine
            .render("Page: {{query.page}}", &ctx, "GET", "/list", &HashMap::new(), None)
            .unwrap();

        assert_eq!(result, "Page: 1");
    }

    #[test]
    fn test_headers() {
        let engine = TemplateEngine::new();
        let ctx = MatchContext::default();
        let mut headers = HashMap::new();
        headers.insert("user-agent".to_string(), "test-client".to_string());

        let result = engine
            .render(
                "Client: {{headers.user-agent}}",
                &ctx,
                "GET",
                "/",
                &headers,
                None,
            )
            .unwrap();

        assert_eq!(result, "Client: test-client");
    }

    #[test]
    fn test_request_body() {
        let engine = TemplateEngine::new();
        let ctx = MatchContext::default();
        let body = br#"{"name":"John"}"#;

        let result = engine
            .render(
                "Name: {{json.name}}",
                &ctx,
                "POST",
                "/users",
                &HashMap::new(),
                Some(body),
            )
            .unwrap();

        assert_eq!(result, "Name: John");
    }

    #[test]
    fn test_uuid_helper() {
        let engine = TemplateEngine::new();
        let ctx = MatchContext::default();

        let result = engine
            .render("ID: {{uuid}}", &ctx, "GET", "/", &HashMap::new(), None)
            .unwrap();

        // UUID format: xxxxxxxx-xxxx-4xxx-xxxx-xxxxxxxxxxxx
        assert!(result.starts_with("ID: "));
        let uuid = &result[4..];
        assert_eq!(uuid.len(), 36);
        assert!(uuid.chars().nth(8) == Some('-'));
    }

    #[test]
    fn test_default_helper() {
        let engine = TemplateEngine::new();
        let ctx = MatchContext::default();

        let result = engine
            .render(
                "Value: {{default query.missing \"default_value\"}}",
                &ctx,
                "GET",
                "/",
                &HashMap::new(),
                None,
            )
            .unwrap();

        assert_eq!(result, "Value: default_value");
    }

    #[test]
    fn test_upper_lower_helpers() {
        let engine = TemplateEngine::new();
        let mut ctx = MatchContext::default();
        ctx.path_params.insert("name".to_string(), "John".to_string());

        let result = engine
            .render(
                "Upper: {{upper path.name}}, Lower: {{lower path.name}}",
                &ctx,
                "GET",
                "/",
                &HashMap::new(),
                None,
            )
            .unwrap();

        assert_eq!(result, "Upper: JOHN, Lower: john");
    }

    #[test]
    fn test_render_json() {
        let engine = TemplateEngine::new();
        let mut ctx = MatchContext::default();
        ctx.path_params.insert("id".to_string(), "123".to_string());

        let json = serde_json::json!({
            "id": "{{path.id}}",
            "name": "User {{path.id}}",
            "static": "no template"
        });

        let result = engine
            .render_json(&json, &ctx, "GET", "/users/123", &HashMap::new(), None)
            .unwrap();

        assert_eq!(result["id"], "123");
        assert_eq!(result["name"], "User 123");
        assert_eq!(result["static"], "no template");
    }
}
