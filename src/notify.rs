//! Notification delivery for plan-run events.
//!
//! Channels supported:
//!   * Telegram (Bot HTTP API; non-TLS by default — base URL is overridable
//!     via `RALPHTERM_TELEGRAM_BASE` or `NotifyConfig.telegram_base` for
//!     testability)
//!   * Slack (incoming webhook URL)
//!   * Generic HTTP webhook (POST JSON)
//!   * SMTP email (plain `smtp://` only; `smtps://` is skipped with a warning
//!     unless `RALPHTERM_NOTIFY_FORCE_TLS=1`, in which case the call is
//!     attempted but TLS is unsupported in core notifier — set up a TLS proxy
//!     or use a non-TLS endpoint).
//!
//! TLS is intentionally not supported in the core notifier to avoid pulling in
//! a heavy HTTP/TLS crate. URLs starting with `https://` (or `smtps://`) are
//! skipped with a `tracing::warn!`.
//!
//! Each `Notifier::notify` spawns a dedicated thread per channel with a 10s
//! delivery timeout. Errors are logged via `tracing::warn!` and never
//! propagated to the caller.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    thread,
    time::Duration,
};

use serde::Deserialize;
use serde_json::json;

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOn {
    PlanDone,
    TaskFailed,
    ReviewFailed,
    RateLimit,
}

impl NotifyOn {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "plan_done" | "plan-done" => Some(Self::PlanDone),
            "task_failed" | "task-failed" => Some(Self::TaskFailed),
            "review_failed" | "review-failed" => Some(Self::ReviewFailed),
            "rate_limit" | "rate-limit" | "ratelimit" => Some(Self::RateLimit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NotifyConfig {
    pub telegram_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    /// Override the base URL for the Telegram API (defaults to
    /// `https://api.telegram.org`). Useful for tests; supports `http://` so
    /// the in-process notifier can deliver without TLS.
    pub telegram_base: Option<String>,
    pub slack_webhook: Option<String>,
    pub email_smtp_url: Option<String>,
    pub email_from: Option<String>,
    pub email_to: Option<String>,
    pub webhook_url: Option<String>,
    pub notify_on: Vec<NotifyOn>,
}

impl NotifyConfig {
    fn has_any_channel(&self) -> bool {
        self.telegram_token.is_some()
            || self.slack_webhook.is_some()
            || self.email_smtp_url.is_some()
            || self.webhook_url.is_some()
    }
}

#[derive(Debug, Clone)]
pub enum NotifyEvent {
    PlanDone {
        plan: String,
        summary: String,
    },
    TaskFailed {
        plan: String,
        task: String,
        reason: String,
    },
    ReviewFailed {
        plan: String,
        task: String,
        reason: String,
    },
    RateLimit {
        detail: String,
    },
}

impl NotifyEvent {
    fn event_type(&self) -> NotifyOn {
        match self {
            NotifyEvent::PlanDone { .. } => NotifyOn::PlanDone,
            NotifyEvent::TaskFailed { .. } => NotifyOn::TaskFailed,
            NotifyEvent::ReviewFailed { .. } => NotifyOn::ReviewFailed,
            NotifyEvent::RateLimit { .. } => NotifyOn::RateLimit,
        }
    }

    fn event_label(&self) -> &'static str {
        match self.event_type() {
            NotifyOn::PlanDone => "plan_done",
            NotifyOn::TaskFailed => "task_failed",
            NotifyOn::ReviewFailed => "review_failed",
            NotifyOn::RateLimit => "rate_limit_detected",
        }
    }

    fn subject(&self) -> String {
        match self {
            NotifyEvent::PlanDone { plan, .. } => format!("ralphterm: plan done — {plan}"),
            NotifyEvent::TaskFailed { plan, task, .. } => {
                format!("ralphterm: task failed — {plan} ({task})")
            }
            NotifyEvent::ReviewFailed { plan, task, .. } => {
                format!("ralphterm: review failed — {plan} ({task})")
            }
            NotifyEvent::RateLimit { .. } => "ralphterm: rate limit detected".to_string(),
        }
    }

    fn text_body(&self) -> String {
        match self {
            NotifyEvent::PlanDone { plan, summary } => {
                format!("Plan {plan} completed successfully.\n\n{summary}")
            }
            NotifyEvent::TaskFailed { plan, task, reason } => {
                format!("Plan {plan} failed on task {task}.\n\n{reason}")
            }
            NotifyEvent::ReviewFailed { plan, task, reason } => {
                format!("Plan {plan} task {task} was rejected during review.\n\n{reason}")
            }
            NotifyEvent::RateLimit { detail } => {
                format!("Rate limit detected.\n\n{detail}")
            }
        }
    }

    fn json_body(&self) -> serde_json::Value {
        match self {
            NotifyEvent::PlanDone { plan, summary } => json!({
                "event": self.event_label(),
                "plan": plan,
                "summary": summary,
            }),
            NotifyEvent::TaskFailed { plan, task, reason } => json!({
                "event": self.event_label(),
                "plan": plan,
                "task": task,
                "reason": reason,
            }),
            NotifyEvent::ReviewFailed { plan, task, reason } => json!({
                "event": self.event_label(),
                "plan": plan,
                "task": task,
                "reason": reason,
            }),
            NotifyEvent::RateLimit { detail } => json!({
                "event": self.event_label(),
                "detail": detail,
            }),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Notifier {
    cfg: NotifyConfig,
}

impl Notifier {
    pub fn from_config(cfg: &NotifyConfig) -> Self {
        Self { cfg: cfg.clone() }
    }

    pub fn is_enabled(&self) -> bool {
        self.cfg.has_any_channel()
    }

    /// Fire-and-forget delivery. Returns immediately after spawning per-channel
    /// worker threads. Each thread has its own 10s delivery timeout. Errors
    /// are logged via `tracing::warn!` and not propagated.
    pub fn notify(&self, event: NotifyEvent) {
        if !self.cfg.has_any_channel() {
            return;
        }
        if !self.cfg.notify_on.is_empty() && !self.cfg.notify_on.contains(&event.event_type()) {
            return;
        }

        let cfg = self.cfg.clone();
        // Spawn channel workers. We do not join them so the runner can move on
        // without being blocked by slow networks. Each worker handles its own
        // timeouts.
        if let Some(token) = cfg.telegram_token.clone() {
            if let Some(chat) = cfg.telegram_chat_id.clone() {
                let base = telegram_base(&cfg);
                let event = event.clone();
                thread::spawn(move || {
                    if let Err(err) = deliver_telegram(&base, &token, &chat, &event) {
                        tracing::warn!(channel = "telegram", error = %err, "notification delivery failed");
                    }
                });
            }
        }
        if let Some(slack) = cfg.slack_webhook.clone() {
            let event = event.clone();
            thread::spawn(move || {
                if let Err(err) = deliver_slack(&slack, &event) {
                    tracing::warn!(channel = "slack", error = %err, "notification delivery failed");
                }
            });
        }
        if let Some(webhook) = cfg.webhook_url.clone() {
            let event = event.clone();
            thread::spawn(move || {
                if let Err(err) = deliver_webhook(&webhook, &event) {
                    tracing::warn!(channel = "webhook", error = %err, "notification delivery failed");
                }
            });
        }
        if let Some(smtp_url) = cfg.email_smtp_url.clone() {
            let from = cfg.email_from.clone();
            let to = cfg.email_to.clone();
            let event = event.clone();
            thread::spawn(move || {
                if let Err(err) = deliver_email(&smtp_url, from.as_deref(), to.as_deref(), &event) {
                    tracing::warn!(channel = "email", error = %err, "notification delivery failed");
                }
            });
        }
    }
}

fn telegram_base(cfg: &NotifyConfig) -> String {
    if let Ok(env_base) = std::env::var("RALPHTERM_TELEGRAM_BASE") {
        return env_base;
    }
    cfg.telegram_base
        .clone()
        .unwrap_or_else(|| "https://api.telegram.org".to_string())
}

fn deliver_telegram(
    base: &str,
    token: &str,
    chat_id: &str,
    event: &NotifyEvent,
) -> Result<(), String> {
    let url = format!("{}/bot{}/sendMessage", base.trim_end_matches('/'), token);
    let body = json!({
        "chat_id": chat_id,
        "text": format!("{}\n\n{}", event.subject(), event.text_body()),
    })
    .to_string();
    http_post_json(&url, &body)
}

fn deliver_slack(webhook: &str, event: &NotifyEvent) -> Result<(), String> {
    let body = json!({
        "text": format!("{}\n\n{}", event.subject(), event.text_body()),
        "event": event.event_label(),
        "payload": event.json_body(),
    })
    .to_string();
    http_post_json(webhook, &body)
}

fn deliver_webhook(url: &str, event: &NotifyEvent) -> Result<(), String> {
    let body = event.json_body().to_string();
    http_post_json(url, &body)
}

fn http_post_json(url: &str, body: &str) -> Result<(), String> {
    let parsed = parse_url(url)?;
    if parsed.scheme == UrlScheme::Https {
        let force = std::env::var("RALPHTERM_NOTIFY_FORCE_TLS").ok();
        if force.as_deref() != Some("1") {
            tracing::warn!(
                "[notify] skipped {url} (TLS endpoint not supported in core notifier; \
                 configure a non-TLS webhook or set RALPHTERM_NOTIFY_FORCE_TLS=1)"
            );
            return Ok(());
        }
        // Fall through and attempt plain TCP anyway; this will almost certainly
        // fail but at least surfaces a clear error.
    }
    let mut stream = open_tcp(&parsed.host, parsed.port)?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: ralphterm-notify/0.1\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        path = parsed.path,
        host = parsed.host_port_header(),
        len = body.len(),
        body = body,
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("write request: {err}"))?;
    stream.flush().ok();
    let mut response = Vec::new();
    let _ = stream.set_read_timeout(Some(DELIVERY_TIMEOUT));
    let _ = stream.read_to_end(&mut response);
    let status = parse_http_status(&response);
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("non-2xx response status {status}"))
    }
}

fn parse_http_status(response: &[u8]) -> u16 {
    let line_end = response
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(response.len());
    let status_line = String::from_utf8_lossy(&response[..line_end]);
    let mut parts = status_line.split_whitespace();
    let _version = parts.next();
    parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UrlScheme {
    Http,
    Https,
}

#[derive(Debug, Clone)]
struct ParsedUrl {
    scheme: UrlScheme,
    host: String,
    port: u16,
    path: String,
    default_port: u16,
}

impl ParsedUrl {
    fn host_port_header(&self) -> String {
        if self.port == self.default_port {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (UrlScheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (UrlScheme::Http, rest)
    } else {
        return Err(format!("unsupported URL scheme: {url}"));
    };
    let default_port = match scheme {
        UrlScheme::Http => 80,
        UrlScheme::Https => 443,
    };
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rfind(':') {
        Some(idx) => {
            let host = &authority[..idx];
            let port: u16 = authority[idx + 1..]
                .parse()
                .map_err(|_| format!("invalid port in URL: {url}"))?;
            (host.to_string(), port)
        }
        None => (authority.to_string(), default_port),
    };
    if host.is_empty() {
        return Err(format!("missing host in URL: {url}"));
    }
    Ok(ParsedUrl {
        scheme,
        host,
        port,
        path: path.to_string(),
        default_port,
    })
}

fn open_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|err| format!("resolve {host}:{port}: {err}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("no addresses resolved for {host}:{port}"));
    }
    let mut last_err: Option<String> = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, DELIVERY_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_write_timeout(Some(DELIVERY_TIMEOUT));
                let _ = stream.set_read_timeout(Some(DELIVERY_TIMEOUT));
                return Ok(stream);
            }
            Err(err) => last_err = Some(format!("connect {addr}: {err}")),
        }
    }
    Err(last_err.unwrap_or_else(|| "unknown connection error".to_string()))
}

fn deliver_email(
    smtp_url: &str,
    from: Option<&str>,
    to: Option<&str>,
    event: &NotifyEvent,
) -> Result<(), String> {
    let parsed = parse_smtp_url(smtp_url)?;
    if parsed.tls {
        let force = std::env::var("RALPHTERM_NOTIFY_FORCE_TLS").ok();
        if force.as_deref() != Some("1") {
            tracing::warn!(
                "[notify] skipped {smtp_url} (TLS SMTP not supported in core notifier; \
                 use smtp:// or set RALPHTERM_NOTIFY_FORCE_TLS=1)"
            );
            return Ok(());
        }
    }
    let from = from.ok_or_else(|| "email_from required".to_string())?;
    let to = to.ok_or_else(|| "email_to required".to_string())?;
    let mut stream = open_tcp(&parsed.host, parsed.port)?;
    read_smtp_line(&mut stream)?; // 220 greeting
    write_smtp_line(&mut stream, "EHLO ralphterm")?;
    drain_smtp_response(&mut stream)?;
    if let (Some(user), Some(pass)) = (parsed.user.as_deref(), parsed.pass.as_deref()) {
        write_smtp_line(&mut stream, "AUTH PLAIN")?;
        let _ = read_smtp_line(&mut stream)?;
        let credentials = base64_encode(format!("\0{user}\0{pass}").as_bytes());
        write_smtp_line(&mut stream, &credentials)?;
        let _ = read_smtp_line(&mut stream)?;
    }
    write_smtp_line(&mut stream, &format!("MAIL FROM:<{from}>"))?;
    let _ = read_smtp_line(&mut stream)?;
    write_smtp_line(&mut stream, &format!("RCPT TO:<{to}>"))?;
    let _ = read_smtp_line(&mut stream)?;
    write_smtp_line(&mut stream, "DATA")?;
    let _ = read_smtp_line(&mut stream)?;
    let subject = event.subject();
    let body = event.text_body();
    let payload = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n.\r\n"
    );
    stream
        .write_all(payload.as_bytes())
        .map_err(|err| format!("write data: {err}"))?;
    stream.flush().ok();
    let _ = read_smtp_line(&mut stream)?;
    write_smtp_line(&mut stream, "QUIT")?;
    let _ = read_smtp_line(&mut stream);
    Ok(())
}

struct ParsedSmtpUrl {
    host: String,
    port: u16,
    user: Option<String>,
    pass: Option<String>,
    tls: bool,
}

fn parse_smtp_url(url: &str) -> Result<ParsedSmtpUrl, String> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("smtps://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("smtp://") {
        (false, rest)
    } else {
        return Err(format!("unsupported SMTP URL scheme: {url}"));
    };
    let (auth, rest) = match rest.find('@') {
        Some(idx) => (Some(&rest[..idx]), &rest[idx + 1..]),
        None => (None, rest),
    };
    let (user, pass) = match auth {
        Some(value) => match value.find(':') {
            Some(idx) => (
                Some(value[..idx].to_string()),
                Some(value[idx + 1..].to_string()),
            ),
            None => (Some(value.to_string()), None),
        },
        None => (None, None),
    };
    let (host_part, _path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let (host, port) = match host_part.rfind(':') {
        Some(idx) => {
            let host = &host_part[..idx];
            let port: u16 = host_part[idx + 1..]
                .parse()
                .map_err(|_| format!("invalid SMTP port: {url}"))?;
            (host.to_string(), port)
        }
        None => {
            let default_port = if tls { 465 } else { 25 };
            (host_part.to_string(), default_port)
        }
    };
    if host.is_empty() {
        return Err(format!("missing SMTP host: {url}"));
    }
    Ok(ParsedSmtpUrl {
        host,
        port,
        user,
        pass,
        tls,
    })
}

fn read_smtp_line(stream: &mut TcpStream) -> Result<String, String> {
    let _ = stream.set_read_timeout(Some(DELIVERY_TIMEOUT));
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(err) => return Err(format!("read smtp: {err}")),
        }
    }
    Ok(String::from_utf8_lossy(&line).trim().to_string())
}

fn drain_smtp_response(stream: &mut TcpStream) -> Result<(), String> {
    // For simplicity drain a single response line; many SMTP servers emit
    // multi-line EHLO replies but our test server uses a single 250 OK.
    let _ = read_smtp_line(stream)?;
    Ok(())
}

fn write_smtp_line(stream: &mut TcpStream, line: &str) -> Result<(), String> {
    stream
        .write_all(line.as_bytes())
        .map_err(|err| format!("write smtp: {err}"))?;
    stream
        .write_all(b"\r\n")
        .map_err(|err| format!("write smtp crlf: {err}"))?;
    stream.flush().ok();
    Ok(())
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b111111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_extracts_host_port_path() {
        let parsed = parse_url("http://127.0.0.1:1234/hook").unwrap();
        assert_eq!(parsed.scheme, UrlScheme::Http);
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 1234);
        assert_eq!(parsed.path, "/hook");
    }

    #[test]
    fn parse_url_defaults_path_and_port() {
        let parsed = parse_url("http://example.com").unwrap();
        assert_eq!(parsed.path, "/");
        assert_eq!(parsed.port, 80);
    }

    #[test]
    fn parse_url_rejects_unknown_scheme() {
        assert!(parse_url("ftp://example.com").is_err());
    }

    #[test]
    fn parse_smtp_url_extracts_credentials() {
        let parsed = parse_smtp_url("smtp://alice:secret@mail.example:2525").unwrap();
        assert_eq!(parsed.host, "mail.example");
        assert_eq!(parsed.port, 2525);
        assert_eq!(parsed.user.as_deref(), Some("alice"));
        assert_eq!(parsed.pass.as_deref(), Some("secret"));
        assert!(!parsed.tls);
    }

    #[test]
    fn notify_on_parses_known_variants() {
        assert_eq!(NotifyOn::parse("plan_done"), Some(NotifyOn::PlanDone));
        assert_eq!(NotifyOn::parse("task-failed"), Some(NotifyOn::TaskFailed));
        assert_eq!(NotifyOn::parse("rate_limit"), Some(NotifyOn::RateLimit));
        assert!(NotifyOn::parse("unknown").is_none());
    }

    #[test]
    fn base64_encode_handles_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }
}
