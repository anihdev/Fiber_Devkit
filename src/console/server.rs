//! Minimal read-only HTTP/1.1 server for `fiber console`.
//! Uses `tokio::net::TcpListener` and embedded assets only; every endpoint is
//! GET-only and delegates to existing visibility, route, taxonomy, or report data.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::DevkitConfig;
use crate::console::assets;
use crate::diagnostic::taxonomy::ENTRIES;
use crate::reporter::formats::{output_dir, LAST_RUN_FILE};
use crate::route::analyzer::RouteAnalyzer;
use crate::visibility::{self, ChannelInspection, InspectStatus};
use crate::AppResult;

const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Bound console server ready to serve read-only local HTTP requests.
pub struct ConsoleServer {
    listener: TcpListener,
    url: String,
}

/// Binds the console listener before any browser open attempt is made.
pub async fn bind(port: u16) -> AppResult<ConsoleServer> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let url = format!("http://{}/", listener.local_addr()?);
    Ok(ConsoleServer { listener, url })
}

impl ConsoleServer {
    /// Returns the URL for the already-bound localhost listener.
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Serves the console until the process is interrupted.
    pub async fn serve(self, project_root: PathBuf) -> AppResult<()> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let project_root = project_root.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_connection(stream, project_root).await {
                    eprintln!("console request failed: {err}");
                }
            });
        }
    }
}

async fn handle_connection(mut stream: TcpStream, project_root: PathBuf) -> io::Result<()> {
    let mut buffer = vec![0_u8; MAX_REQUEST_BYTES];
    let bytes_read = stream.read(&mut buffer).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let response = route_request(&request, project_root).await;
    write_response(&mut stream, response).await
}

async fn route_request(request: &str, project_root: PathBuf) -> HttpResponse {
    let Some(first_line) = request.lines().next() else {
        return error_response(HttpStatus::BadRequest, "empty request");
    };
    let mut parts = first_line.split_whitespace();
    let Some(method) = parts.next() else {
        return error_response(HttpStatus::BadRequest, "missing request method");
    };
    let Some(target) = parts.next() else {
        return error_response(HttpStatus::BadRequest, "missing request target");
    };

    if method != "GET" {
        return error_response(
            HttpStatus::MethodNotAllowed,
            "fiber console only accepts GET",
        );
    }

    let (path, query) = split_target(target);
    match path {
        "/" => text_response(
            HttpStatus::Ok,
            "text/html; charset=utf-8",
            assets::INDEX_HTML,
        ),
        "/app.js" => text_response(
            HttpStatus::Ok,
            "application/javascript; charset=utf-8",
            assets::APP_JS,
        ),
        "/style.css" => text_response(HttpStatus::Ok, "text/css; charset=utf-8", assets::STYLE_CSS),
        "/api/nodes" => api_nodes(&project_root).await,
        "/api/predict" => api_predict(&project_root, query.unwrap_or_default()).await,
        "/api/taxonomy" => json_response(HttpStatus::Ok, ENTRIES),
        "/api/last-run" => api_last_run(&project_root).await,
        _ => {
            if let Some(name) = channel_node_name(path) {
                api_node_channels(&project_root, name).await
            } else {
                error_response(HttpStatus::NotFound, "endpoint not found")
            }
        }
    }
}

async fn api_nodes(project_root: &std::path::Path) -> HttpResponse {
    match visibility::inspect_project(project_root, None).await {
        Ok(output) => json_response(HttpStatus::Ok, &output),
        Err(err) => error_response(HttpStatus::InternalServerError, err.to_string()),
    }
}

async fn api_node_channels(project_root: &std::path::Path, encoded_name: &str) -> HttpResponse {
    let name = match percent_decode(encoded_name) {
        Ok(name) => name,
        Err(err) => return error_response(HttpStatus::BadRequest, err),
    };
    let config = match DevkitConfig::read_from_project(project_root) {
        Ok(config) => config,
        Err(err) => return error_response(HttpStatus::InternalServerError, err.to_string()),
    };
    if !config.nodes.iter().any(|node| node.name == name) {
        return error_response(
            HttpStatus::NotFound,
            format!("node `{name}` is not configured"),
        );
    }

    let output = visibility::inspect_config(&config, Some(&name)).await;
    let Some(node) = output.nodes.into_iter().next() else {
        return error_response(
            HttpStatus::NotFound,
            format!("node `{name}` was not inspected"),
        );
    };

    json_response(
        HttpStatus::Ok,
        &NodeChannelsResponse {
            node: node.name,
            status: node.status,
            rpc_endpoint: node.rpc_endpoint,
            channels: node.channels,
            error: node.error,
        },
    )
}

async fn api_predict(project_root: &std::path::Path, query: &str) -> HttpResponse {
    let params = match parse_query(query) {
        Ok(params) => params,
        Err(err) => return error_response(HttpStatus::BadRequest, err),
    };
    let from = match required_query_param(&params, "from") {
        Ok(value) => value,
        Err(err) => return error_response(HttpStatus::BadRequest, err),
    };
    let to = match required_query_param(&params, "to") {
        Ok(value) => value,
        Err(err) => return error_response(HttpStatus::BadRequest, err),
    };
    let amount = match required_query_param(&params, "amount") {
        Ok(value) => value,
        Err(err) => return error_response(HttpStatus::BadRequest, err),
    };
    let asset = params.get("asset").map(String::as_str).unwrap_or("CKB");
    let cross_chain = params
        .get("cross_chain")
        .or_else(|| params.get("cross-chain"))
        .is_some_and(|value| parse_bool(value));

    let analyzer = RouteAnalyzer::new(project_root.to_path_buf());
    if cross_chain {
        match analyzer.compare_routes(from, to, amount, asset).await {
            Ok(comparison) => json_response(HttpStatus::Ok, &comparison),
            Err(err) => error_response(HttpStatus::InternalServerError, err.to_string()),
        }
    } else {
        match analyzer.can_pay(from, to, amount, asset).await {
            Ok(prediction) => json_response(HttpStatus::Ok, &prediction),
            Err(err) => error_response(HttpStatus::InternalServerError, err.to_string()),
        }
    }
}

async fn api_last_run(project_root: &std::path::Path) -> HttpResponse {
    let path = output_dir(project_root).join(LAST_RUN_FILE);
    match fs::read_to_string(&path).await {
        Ok(raw) => text_response(HttpStatus::Ok, "application/json; charset=utf-8", raw),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            error_response(HttpStatus::NotFound, "last run artifact not found")
        }
        Err(err) => error_response(HttpStatus::InternalServerError, err.to_string()),
    }
}

fn channel_node_name(path: &str) -> Option<&str> {
    path.strip_prefix("/api/nodes/")
        .and_then(|rest| rest.strip_suffix("/channels"))
        .filter(|name| !name.is_empty())
}

fn split_target(target: &str) -> (&str, Option<&str>) {
    let without_fragment = target.split('#').next().unwrap_or(target);
    if let Some(index) = without_fragment.find('?') {
        (
            &without_fragment[..index],
            Some(&without_fragment[index + 1..]),
        )
    } else {
        (without_fragment, None)
    }
}

fn parse_query(query: &str) -> Result<HashMap<String, String>, String> {
    let mut params = HashMap::new();
    if query.is_empty() {
        return Ok(params);
    }

    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let mut parts = pair.splitn(2, '=');
        let key = percent_decode(parts.next().unwrap_or_default())?;
        let value = percent_decode(parts.next().unwrap_or_default())?;
        params.insert(key, value);
    }
    Ok(params)
}

fn required_query_param<'a>(
    params: &'a HashMap<String, String>,
    name: &str,
) -> Result<&'a str, String> {
    params
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing `{name}` query parameter"))
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn percent_decode(input: &str) -> Result<String, String> {
    let mut decoded = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("invalid percent-encoded value".to_string());
                }
                let high = hex_value(bytes[index + 1])
                    .ok_or_else(|| "invalid percent-encoded value".to_string())?;
                let low = hex_value(bytes[index + 2])
                    .ok_or_else(|| "invalid percent-encoded value".to_string())?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(decoded).map_err(|_| "query contained invalid utf-8".to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn write_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    let (status_code, reason) = response.status.code_reason();
    let header = format!(
        "HTTP/1.1 {status_code} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.content_type,
        response.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await
}

fn json_response<T: Serialize + ?Sized>(status: HttpStatus, value: &T) -> HttpResponse {
    match serde_json::to_string_pretty(value) {
        Ok(body) => text_response(status, "application/json; charset=utf-8", body),
        Err(err) => error_response(HttpStatus::InternalServerError, err.to_string()),
    }
}

fn error_response(status: HttpStatus, message: impl Into<String>) -> HttpResponse {
    json_response(status, &json!({ "error": message.into() }))
}

fn text_response(
    status: HttpStatus,
    content_type: &'static str,
    body: impl Into<String>,
) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body: body.into(),
    }
}

struct HttpResponse {
    status: HttpStatus,
    content_type: &'static str,
    body: String,
}

#[derive(Clone, Copy)]
enum HttpStatus {
    Ok,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    InternalServerError,
}

impl HttpStatus {
    fn code_reason(self) -> (u16, &'static str) {
        match self {
            Self::Ok => (200, "OK"),
            Self::BadRequest => (400, "Bad Request"),
            Self::NotFound => (404, "Not Found"),
            Self::MethodNotAllowed => (405, "Method Not Allowed"),
            Self::InternalServerError => (500, "Internal Server Error"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeChannelsResponse {
    node: String,
    status: InspectStatus,
    rpc_endpoint: String,
    channels: Vec<ChannelInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_decodes_query_parameters() {
        let params = parse_query("from=node-1&to=node-2&amount=1+CKB&asset=wrapped%2DBTC")
            .expect("query should decode");

        assert_eq!(params["from"], "node-1");
        assert_eq!(params["amount"], "1 CKB");
        assert_eq!(params["asset"], "wrapped-BTC");
    }

    #[test]
    fn extracts_channel_node_name() {
        assert_eq!(
            channel_node_name("/api/nodes/node-1/channels"),
            Some("node-1")
        );
        assert_eq!(channel_node_name("/api/nodes/node-1"), None);
    }
}
