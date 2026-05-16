use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn run_api_creates_lists_reads_events_and_cancels_run_records() {
    let repo = TempDir::new();
    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let server = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args(["serve", "--bind", &bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ralphterm serve");
    let mut server = ChildGuard::new(server);
    wait_for_server(port, server.child_mut());

    let created = request_json(
        port,
        "POST /v1/runs HTTP/1.1",
        Some(r#"{"phase":"complete","status":"failed","plan_path":"docs/plan.md"}"#),
    );
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "planning");
    assert_eq!(created_json["status"], "created");
    assert_eq!(created_json["plan_path"], "docs/plan.md");

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 1);
    assert_eq!(listed_json[0]["id"], id);

    let viewed = request_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), None);
    assert_eq!(viewed.status, 200, "{}", viewed.body);
    let viewed_json: serde_json::Value = serde_json::from_str(&viewed.body).expect("view json");
    assert_eq!(viewed_json["id"], id);
    assert_eq!(viewed_json["status"], "created");

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert_eq!(events_json.as_array().expect("event list").len(), 1);
    assert_eq!(events_json[0]["type"], "run_created");

    let cancelled = request_json(
        port,
        &format!("POST /v1/runs/{id}/cancel HTTP/1.1"),
        Some("{}"),
    );
    assert_eq!(cancelled.status, 202, "{}", cancelled.body);

    let viewed_after_cancel = request_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), None);
    assert_eq!(
        viewed_after_cancel.status, 200,
        "{}",
        viewed_after_cancel.body
    );
    let cancelled_json: serde_json::Value =
        serde_json::from_str(&viewed_after_cancel.body).expect("cancelled json");
    assert_eq!(cancelled_json["status"], "failed");
    assert_eq!(cancelled_json["phase"], "complete");
}

struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-run-api-test-{suffix}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

struct Response {
    status: u16,
    body: String,
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local addr").port()
}

fn wait_for_server(port: u16, server: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = server.try_wait().expect("server status") {
            panic!("server exited before ready: {status}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("server did not become ready");
}

fn request_json(port: u16, request_line: &str, body: Option<&str>) -> Response {
    let body = body.unwrap_or("");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    write!(
        stream,
        "{request_line}\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");

    let mut raw = String::new();
    stream.read_to_string(&mut raw).expect("read response");
    let (headers, body) = raw.split_once("\r\n\r\n").expect("http response");
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .expect("status code");
    Response {
        status,
        body: body.to_string(),
    }
}
