//! Rendering an [`ApiDoc`] to Markdown.
//!
//! Mirrors Servant's `markdown`/`markdownWith`: each endpoint becomes an `H2`
//! heading (`## METHOD /path`) followed by a **fixed, load-bearing** section
//! order. We render the sections we model:
//!
//! 1. Summary / Description note (Servant's "Notes"),
//! 2. Captures,
//! 3. Headers,
//! 4. Query parameters,
//! 5. Query string,
//! 6. Fragment,
//! 7. Request (content types),
//! 8. Response (status + content types).
//!
//! **[diff]** Endpoints are rendered in API (left-to-right) order rather than
//! Servant's sort-by-`(path, method)`, matching how the rest of the workspace
//! preserves the description's left-biased structure. Example payload bodies and
//! curl samples (Servant's `ToSample`/`curlStr`) are not emitted.

use crate::model::{ApiDoc, EndpointDoc, ParamKind, PathPart};

/// Render `doc` to a Markdown string with a fixed section order per endpoint.
pub fn markdown(doc: &ApiDoc) -> String {
    let mut lines: Vec<String> = Vec::new();
    for ep in doc.endpoints() {
        render_endpoint(ep, &mut lines);
    }
    // Trailing newline, like `unlines`.
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn render_path(path: &[PathPart]) -> String {
    if path.is_empty() {
        return "/".to_string();
    }
    let mut s = String::new();
    for part in path {
        s.push('/');
        s.push_str(&part.render());
    }
    s
}

fn render_endpoint(ep: &EndpointDoc, lines: &mut Vec<String>) {
    lines.push(format!(
        "## {} {}",
        ep.method.as_str(),
        render_path(&ep.path)
    ));
    lines.push(String::new());

    render_notes(ep, lines);
    render_captures(ep, lines);
    render_headers(ep, lines);
    render_params(ep, lines);
    render_query_string(ep, lines);
    render_fragment(ep, lines);
    render_request(ep, lines);
    render_response(ep, lines);
}

fn render_notes(ep: &EndpointDoc, lines: &mut Vec<String>) {
    if ep.summary.is_none() && ep.description.is_none() && ep.operation_id.is_none() {
        return;
    }
    if let Some(summary) = &ep.summary {
        lines.push(format!("### {summary}"));
        lines.push(String::new());
    }
    if let Some(description) = &ep.description {
        lines.push(description.clone());
        lines.push(String::new());
    }
    if let Some(operation_id) = &ep.operation_id {
        lines.push(format!("OperationId: {operation_id}"));
        lines.push(String::new());
    }
}

fn render_captures(ep: &EndpointDoc, lines: &mut Vec<String>) {
    let caps: Vec<(&str, &str)> = ep
        .path
        .iter()
        .filter_map(|p| match p {
            PathPart::Capture { name, type_name } => Some((name.as_str(), *type_name)),
            PathPart::CaptureAll { name, type_name } => Some((name.as_str(), *type_name)),
            PathPart::Static(_) => None,
        })
        .collect();
    if caps.is_empty() {
        return;
    }
    lines.push("### Captures:".to_string());
    lines.push(String::new());
    for (name, type_name) in caps {
        lines.push(format!("- *{name}*: `{type_name}`"));
    }
    lines.push(String::new());
}

fn render_headers(ep: &EndpointDoc, lines: &mut Vec<String>) {
    if ep.headers.is_empty() {
        return;
    }
    lines.push("### Headers:".to_string());
    lines.push(String::new());
    for name in &ep.headers {
        lines.push(format!(
            "- This endpoint is sensitive to the value of the **{name}** HTTP header."
        ));
    }
    lines.push(String::new());
}

fn render_params(ep: &EndpointDoc, lines: &mut Vec<String>) {
    if ep.query_params.is_empty() {
        return;
    }
    lines.push(format!("### {} Parameters:", ep.method.as_str()));
    lines.push(String::new());
    for param in &ep.query_params {
        lines.push(format!("- {}", param.name));
        match param.kind {
            ParamKind::Normal => {
                lines.push(format!("     - **Values**: `{}`", param.type_name));
            }
            ParamKind::List => {
                lines.push(format!("     - **Values**: `{}`", param.type_name));
                lines.push(format!(
                    "     - This parameter is a **list**. All {} parameters with the name {}[] will forward their values in a list to the handler.",
                    ep.method.as_str(),
                    param.name
                ));
            }
            ParamKind::Flag => {
                lines.push(
                    "     - This parameter is a **flag**. This means no value is expected to be associated to this parameter.".to_string(),
                );
            }
        }
    }
    lines.push(String::new());
}

fn render_fragment(ep: &EndpointDoc, lines: &mut Vec<String>) {
    let Some(fragment) = &ep.fragment else {
        return;
    };
    lines.push("### Fragment:".to_string());
    lines.push(String::new());
    lines.push(format!(
        "- *{}*: {}",
        short_type_name(fragment.type_name),
        fragment.description
    ));
    lines.push(String::new());
}

fn render_query_string(ep: &EndpointDoc, lines: &mut Vec<String>) {
    if ep.query_string.is_none() {
        return;
    }
    lines.push("### Query String:".to_string());
    lines.push(String::new());
    lines.push(
        "- Captures the raw query string when available and decoded ordered query pairs."
            .to_string(),
    );
    lines.push(String::new());
}

fn render_request(ep: &EndpointDoc, lines: &mut Vec<String>) {
    let Some(body) = &ep.request_body else {
        return;
    };
    lines.push("### Request:".to_string());
    lines.push(String::new());
    lines.push(format!("- Decoded as: `{}`", body.type_name));
    lines.push("- Supported content types are:".to_string());
    lines.push(String::new());
    for ct in &body.content_types {
        lines.push(format!("    - `{ct}`"));
    }
    lines.push(String::new());
}

fn short_type_name(type_name: &str) -> &str {
    match type_name.rsplit("::").next() {
        Some(short) => short,
        None => type_name,
    }
}

fn render_response(ep: &EndpointDoc, lines: &mut Vec<String>) {
    lines.push("### Response:".to_string());
    lines.push(String::new());
    lines.push(format!("- Status code {}", ep.status.as_u16()));
    if ep.response_types.is_empty() {
        lines.push("- No response body".to_string());
    } else {
        lines.push("- Supported content types are:".to_string());
        lines.push(String::new());
        for ct in &ep.response_types {
            lines.push(format!("    - `{ct}`"));
        }
    }
    lines.push(String::new());
}
