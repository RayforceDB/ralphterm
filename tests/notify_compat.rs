use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use uuid::Uuid;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_minimal_plan(path: &Path) {
    fs::write(
        path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
}

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ralphterm-notify-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        git(&path, ["init", "--initial-branch", "main"]);
        git(&path, ["config", "user.email", "test@example.com"]);
        git(&path, ["config", "user.name", "Test User"]);
        fs::write(path.join("README.md"), "hello\n").unwrap();
        git(&path, ["add", "README.md"]);
        git(&path, ["commit", "-m", "initial"]);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git<I, S>(cwd: &Path, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct CapturedRequest {
    body: String,
    path: String,
}

fn spawn_http_collector() -> (String, Arc<Mutex<Vec<CapturedRequest>>>, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
    let addr = listener.local_addr().expect("local addr");
    let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_for_server = captured.clone();
    let listener_clone = listener.try_clone().expect("clone listener");
    thread::spawn(move || {
        for stream in listener_clone.incoming() {
            let stream = match stream {
                Ok(stream) => stream,
                Err(_) => break,
            };
            let captured = captured_for_server.clone();
            thread::spawn(move || {
                handle_http_request(stream, captured);
            });
        }
    });
    (format!("http://{addr}"), captured, listener)
}

fn handle_http_request(mut stream: TcpStream, captured: Arc<Mutex<Vec<CapturedRequest>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let mut tmp = [0u8; 1024];
    let mut headers_end: Option<usize> = None;
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&tmp[..n]);
                if let Some(idx) = find_double_crlf(&buffer) {
                    headers_end = Some(idx + 4);
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let headers_end = match headers_end {
        Some(idx) => idx,
        None => return,
    };

    let header_str = String::from_utf8_lossy(&buffer[..headers_end]).to_string();
    let mut content_length: usize = 0;
    let mut request_path = String::new();
    for (i, line) in header_str.lines().enumerate() {
        if i == 0 {
            // request line: METHOD PATH HTTP/1.1
            let mut parts = line.split_whitespace();
            let _method = parts.next();
            if let Some(path) = parts.next() {
                request_path = path.to_string();
            }
        }
        if let Some(value) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(|v| v.trim().to_string())
        {
            if let Ok(parsed) = value.parse::<usize>() {
                content_length = parsed;
            }
        }
    }

    while buffer.len() < headers_end + content_length {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buffer.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }

    let body =
        String::from_utf8_lossy(&buffer[headers_end..headers_end + content_length]).to_string();
    captured.lock().unwrap().push(CapturedRequest {
        body,
        path: request_path,
    });

    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
    let _ = stream.flush();
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(3) {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

#[test]
fn webhook_fires_on_plan_done() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let (base_url, captured, _listener) = spawn_http_collector();
    let webhook_url = format!("{base_url}/hook");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--notify-webhook",
            &webhook_url,
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let received = captured.lock().unwrap();
            if !received.is_empty() {
                let request = &received[0];
                assert_eq!(request.path, "/hook");
                assert!(
                    request.body.contains("plan_done"),
                    "expected plan_done event in body: {}",
                    request.body
                );
                assert!(
                    request.body.contains("plan.md"),
                    "expected plan name in body: {}",
                    request.body
                );
                return;
            }
        }
        if Instant::now() > deadline {
            panic!("webhook did not receive a request within 5s");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn webhook_fires_on_task_failed_when_filter_set() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    fs::write(
        &plan_path,
        r#"# Failing plan

## Validation Commands
- `test -f never.txt`

### Task 1: Will fail
- [ ] Do something
"#,
    )
    .unwrap();

    let (base_url, captured, _listener) = spawn_http_collector();
    let webhook_url = format!("{base_url}/hook");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("failing-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--notify-webhook",
            &webhook_url,
            "--notify-on",
            "task_failed",
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(!output.status.success(), "expected failure run");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let received = captured.lock().unwrap();
            if !received.is_empty() {
                let request = &received[0];
                assert!(
                    request.body.contains("task_failed"),
                    "body: {}",
                    request.body
                );
                return;
            }
        }
        if Instant::now() > deadline {
            panic!("webhook did not receive a request within 5s");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn telegram_via_env_override_base_url() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let (base_url, captured, _listener) = spawn_http_collector();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .env("RALPHTERM_TELEGRAM_BASE", &base_url)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--notify-telegram-token",
            "test-token",
            "--notify-telegram-chat",
            "test-chat",
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let received = captured.lock().unwrap();
            if !received.is_empty() {
                let request = &received[0];
                assert!(
                    request.path.contains("test-token"),
                    "expected token in url: {}",
                    request.path
                );
                assert!(
                    request.path.contains("sendMessage")
                        || request.body.contains("sendMessage")
                        || request.body.contains("test-chat"),
                    "expected sendMessage path or chat in body: {} body={}",
                    request.path,
                    request.body
                );
                return;
            }
        }
        if Instant::now() > deadline {
            panic!("telegram did not receive a request within 5s");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn slack_via_webhook_with_base_override() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let (base_url, captured, _listener) = spawn_http_collector();
    let slack_webhook = format!("{base_url}/services/T000/B000/secret");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--notify-slack",
            &slack_webhook,
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let received = captured.lock().unwrap();
            if !received.is_empty() {
                let request = &received[0];
                assert!(
                    request.path.starts_with("/services/T000/B000/"),
                    "expected slack webhook path; got {}",
                    request.path
                );
                assert!(
                    request.body.contains("plan.md") || request.body.contains("plan_done"),
                    "body did not contain expected text: {}",
                    request.body
                );
                return;
            }
        }
        if Instant::now() > deadline {
            panic!("slack did not receive a request within 5s");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn spawn_smtp_collector() -> (String, Arc<Mutex<Vec<String>>>, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind smtp");
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let listener_clone = listener.try_clone().unwrap();
    thread::spawn(move || {
        for stream in listener_clone.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let captured = captured_clone.clone();
            thread::spawn(move || handle_smtp_session(stream, captured));
        }
    });
    (format!("{}:{}", addr.ip(), addr.port()), captured, listener)
}

fn handle_smtp_session(mut stream: TcpStream, captured: Arc<Mutex<Vec<String>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.write_all(b"220 test ESMTP ready\r\n");
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut buffer = String::new();
    let mut in_data = false;
    let mut data = String::new();
    loop {
        let mut line = String::new();
        let read = match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let _ = read;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if in_data {
            if trimmed == "." {
                in_data = false;
                captured.lock().unwrap().push(data.clone());
                let _ = stream.write_all(b"250 OK\r\n");
                continue;
            }
            data.push_str(trimmed);
            data.push('\n');
            continue;
        }
        buffer.push_str(&line);
        let upper = trimmed.to_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            let _ = stream.write_all(b"250-test\r\n250 OK\r\n");
        } else if upper.starts_with("DATA") {
            let _ = stream.write_all(b"354 send data\r\n");
            in_data = true;
            data.clear();
        } else if upper.starts_with("QUIT") {
            let _ = stream.write_all(b"221 bye\r\n");
            break;
        } else if upper.starts_with("AUTH") {
            let _ = stream.write_all(b"235 auth ok\r\n");
        } else {
            let _ = stream.write_all(b"250 OK\r\n");
        }
    }
}

#[test]
fn smtp_email_body_contains_plan_name() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let (smtp_addr, captured, _listener) = spawn_smtp_collector();
    let smtp_url = format!("smtp://{}", smtp_addr);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--notify-email-smtp-url",
            &smtp_url,
            "--notify-email-from",
            "sender@example.com",
            "--notify-email-to",
            "recipient@example.com",
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let received = captured.lock().unwrap();
            if !received.is_empty() {
                let body = &received[0];
                assert!(
                    body.contains("plan.md") || body.contains("Plan"),
                    "expected plan in email body: {body}"
                );
                return;
            }
        }
        if Instant::now() > deadline {
            panic!("smtp did not receive a message within 5s");
        }
        thread::sleep(Duration::from_millis(50));
    }
}
