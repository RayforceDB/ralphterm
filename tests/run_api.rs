use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn dashboard_shell_serves_html_css_and_runs_javascript() {
    let _guard = server_test_lock();
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

    let html = request_json(port, "GET /dashboard HTTP/1.1", None);
    assert_eq!(html.status, 200, "{}", html.body);
    assert!(
        html.content_type.starts_with("text/html"),
        "{}",
        html.content_type
    );
    assert!(html.body.contains("RalphTerm Dashboard"), "{}", html.body);
    assert!(html.body.contains("Runs"), "{}", html.body);
    assert!(html.body.contains("Sessions"), "{}", html.body);
    assert!(html.body.contains("Artifacts"), "{}", html.body);
    assert!(html.body.contains("/dashboard/styles.css"), "{}", html.body);
    assert!(html.body.contains("/dashboard/app.js"), "{}", html.body);

    let css = request_json(port, "GET /dashboard/styles.css HTTP/1.1", None);
    assert_eq!(css.status, 200, "{}", css.body);
    assert!(
        css.content_type.starts_with("text/css"),
        "{}",
        css.content_type
    );
    assert!(css.body.contains("RalphTerm Dashboard"), "{}", css.body);

    let js = request_json(port, "GET /dashboard/app.js HTTP/1.1", None);
    assert_eq!(js.status, 200, "{}", js.body);
    assert!(
        js.content_type.starts_with("application/javascript"),
        "{}",
        js.content_type
    );
    assert!(js.body.contains("fetch('/v1/runs')"), "{}", js.body);
    assert!(js.body.contains("renderRunRows"), "{}", js.body);
    assert!(
        js.body.contains("/v1/runs/${run.id}/summary"),
        "{}",
        js.body
    );
    assert!(
        js.body.contains("/v1/runs/${run.id}/summary.json"),
        "{}",
        js.body
    );
    assert!(js.body.contains("/v1/runs/${run.id}/diff"), "{}", js.body);
    assert!(js.body.contains("/v1/runs/${run.id}/events"), "{}", js.body);
    assert!(
        js.body.contains("Summary artifact for run ${run.id}"),
        "{}",
        js.body
    );
    assert!(
        js.body
            .contains("renderErrorRow(runsBody, error.message, 5)"),
        "{}",
        js.body
    );
}

#[test]
fn dashboard_lists_active_sessions_and_javascript_renders_them() {
    let _guard = server_test_lock();
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

    let js = request_json(port, "GET /dashboard/app.js HTTP/1.1", None);
    assert_eq!(js.status, 200, "{}", js.body);
    assert!(js.body.contains("fetch('/v1/sessions')"), "{}", js.body);
    assert!(js.body.contains("renderSessionRows"), "{}", js.body);

    let create_body = serde_json::json!({
        "agent": "codex",
        "prompt": "hello from dashboard test",
        "command": "/bin/sh",
        "args": ["-c", "printf 'PLAN_READY\\n'; sleep 30"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&create_body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let listed = wait_for_json(port, "GET /v1/sessions HTTP/1.1", |json| {
        json.as_array()
            .and_then(|sessions| sessions.iter().find(|session| session["id"] == id))
            .and_then(|session| {
                (session["agent"] == "codex"
                    && session["status"] == "running"
                    && session["signal"] == "PLAN_READY")
                    .then(|| json.clone())
            })
    });
    let sessions = listed.as_array().expect("session list");
    assert_eq!(sessions.len(), 1, "{listed}");
    assert_eq!(sessions[0]["id"], id);
    assert_eq!(sessions[0]["agent"], "codex");
    assert_eq!(sessions[0]["status"], "running");
    assert_eq!(sessions[0]["signal"], "PLAN_READY");
}

#[test]
fn session_list_and_dashboard_expose_pending_approval_status() {
    let _guard = server_test_lock();
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

    let html = request_json(port, "GET /dashboard HTTP/1.1", None);
    assert_eq!(html.status, 200, "{}", html.body);
    assert!(html.body.contains("Approval"), "{}", html.body);

    let js = request_json(port, "GET /dashboard/app.js HTTP/1.1", None);
    assert_eq!(js.status, 200, "{}", js.body);
    assert!(js.body.contains("approval_pending"), "{}", js.body);
    assert!(js.body.contains("Pending"), "{}", js.body);
    assert!(js.body.contains("Clear"), "{}", js.body);

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'PLAN_READY\\n'; read gate; printf 'Approve? '; read answer; printf 'GATE:%s\\nANSWER:%s\\n' \"$gate\" \"$answer\"; sleep 30"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("PLAN_READY"),
    );
    let before_prompt = wait_for_json(port, "GET /v1/sessions HTTP/1.1", |json| {
        json.as_array()
            .and_then(|sessions| sessions.iter().find(|session| session["id"] == id))
            .and_then(|session| {
                (session["status"] == "running" && session["signal"] == "PLAN_READY")
                    .then(|| session.clone())
            })
    });
    assert_eq!(before_prompt["approval_pending"], false, "{before_prompt}");

    let gate_body = serde_json::json!({"text": "continue", "enter": true}).to_string();
    let gate_released = request_json(
        port,
        &format!("POST /v1/sessions/{id}/input HTTP/1.1"),
        Some(&gate_body),
    );
    assert_eq!(gate_released.status, 202, "{}", gate_released.body);

    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    let pending = wait_for_json(port, "GET /v1/sessions HTTP/1.1", |json| {
        json.as_array()
            .and_then(|sessions| sessions.iter().find(|session| session["id"] == id))
            .and_then(|session| (session["approval_pending"] == true).then(|| session.clone()))
    });
    assert_eq!(pending["approval_pending"], true, "{pending}");
}

#[test]
fn session_approval_decision_posts_to_pty_and_emits_event() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Approve? '; read answer; printf 'ANSWER:%s\\n' \"$answer\"; sleep 1"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let mut events = connect_ws(port, &format!("/v1/sessions/{id}/events"));
    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-requested").then(|| event.clone())
    });

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 202, "{}", approved.body);
    let approved_json: serde_json::Value =
        serde_json::from_str(&approved.body).expect("approval response json");
    assert_eq!(approved_json["id"], id);
    assert_eq!(approved_json["approved"], true);

    let transcript = wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("ANSWER:y"),
    );
    assert!(transcript.contains("ANSWER:y"), "{transcript}");

    let decision_event = read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-decision").then(|| event.clone())
    });
    assert_eq!(decision_event["approved"], true, "{decision_event}");
}

#[test]
fn session_approval_decision_rejects_stale_prompt_after_approval_output() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Approve? '; read answer; printf 'ANSWER:%s\\nPOST_APPROVAL\\n' \"$answer\"; read second; printf 'SECOND:%s\\n' \"$second\"; sleep 1"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let mut events = connect_ws(port, &format!("/v1/sessions/{id}/events"));
    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-requested").then(|| event.clone())
    });

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 202, "{}", approved.body);

    let transcript = wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("POST_APPROVAL"),
    );
    assert!(transcript.contains("ANSWER:y"), "{transcript}");

    let duplicate = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(duplicate.status, 409, "{}", duplicate.body);
    let duplicate_json: serde_json::Value =
        serde_json::from_str(&duplicate.body).expect("approval error json");
    assert_eq!(duplicate_json["error"], "no approval pending");

    thread::sleep(Duration::from_millis(200));
    let transcript = request_json(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        None,
    );
    assert_eq!(transcript.status, 200, "{}", transcript.body);
    assert!(!transcript.body.contains("SECOND:"), "{}", transcript.body);
}

#[test]
fn session_approval_decision_rejects_prompt_after_session_exits() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Approve? '; exit 0"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let mut events = connect_ws(port, &format!("/v1/sessions/{id}/events"));
    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-requested").then(|| event.clone())
    });
    wait_for_json(port, &format!("GET /v1/sessions/{id} HTTP/1.1"), |json| {
        (json["status"] == "exited").then(|| json.clone())
    });

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 409, "{}", approved.body);
    let approved_json: serde_json::Value =
        serde_json::from_str(&approved.body).expect("approval error json");
    assert_eq!(approved_json["error"], "no approval pending");
    assert_no_approval_decision_event(&mut events);
}

#[test]
fn session_approval_decision_rejects_prompt_after_session_cancel() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Approve? '; read answer; printf 'ANSWER:%s\\n' \"$answer\"; sleep 30"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let mut events = connect_ws(port, &format!("/v1/sessions/{id}/events"));
    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-requested").then(|| event.clone())
    });

    let cancelled = request_json(
        port,
        &format!("POST /v1/sessions/{id}/cancel HTTP/1.1"),
        Some("{}"),
    );
    assert_eq!(cancelled.status, 202, "{}", cancelled.body);
    wait_for_json(port, &format!("GET /v1/sessions/{id} HTTP/1.1"), |json| {
        (json["status"] == "cancelled").then(|| json.clone())
    });

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 409, "{}", approved.body);
    let approved_json: serde_json::Value =
        serde_json::from_str(&approved.body).expect("approval error json");
    assert_eq!(approved_json["error"], "no approval pending");
    assert_no_approval_decision_event(&mut events);

    thread::sleep(Duration::from_millis(200));
    let transcript = request_json(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        None,
    );
    assert_eq!(transcript.status, 200, "{}", transcript.body);
    assert!(!transcript.body.contains("ANSWER:y"), "{}", transcript.body);
}

#[test]
fn session_approval_decision_rejects_unknown_session_with_404() {
    let _guard = server_test_lock();
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

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        "POST /v1/sessions/00000000-0000-0000-0000-000000000000/approval HTTP/1.1",
        Some(&approval_body),
    );
    assert_eq!(approved.status, 404, "{}", approved.body);
    let approved_json: serde_json::Value =
        serde_json::from_str(&approved.body).expect("approval error json");
    assert_eq!(approved_json["error"], "session not found");
}

#[test]
fn session_approval_decision_rejects_when_no_approval_is_pending() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Ready '; read answer; printf 'ANSWER:%s\\n' \"$answer\"; sleep 1"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Ready"),
    );

    let approval_body = serde_json::json!({"approved": true}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 409, "{}", approved.body);
    let approved_json: serde_json::Value =
        serde_json::from_str(&approved.body).expect("approval error json");
    assert_eq!(approved_json["error"], "no approval pending");

    let transcript = request_json(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        None,
    );
    assert_eq!(transcript.status, 200, "{}", transcript.body);
    assert!(!transcript.body.contains("ANSWER:y"), "{}", transcript.body);
}

#[test]
fn session_approval_rejection_posts_n_to_pty() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored initial prompt",
        "command": "/bin/sh",
        "args": ["-c", "read _ignored; printf 'Approve? '; read answer; printf 'ANSWER:%s\\n' \"$answer\"; sleep 1"]
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let mut events = connect_ws(port, &format!("/v1/sessions/{id}/events"));
    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("Approve?"),
    );
    read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-requested").then(|| event.clone())
    });

    let approval_body = serde_json::json!({"approved": false}).to_string();
    let approved = request_json(
        port,
        &format!("POST /v1/sessions/{id}/approval HTTP/1.1"),
        Some(&approval_body),
    );
    assert_eq!(approved.status, 202, "{}", approved.body);

    let transcript = wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("ANSWER:n"),
    );
    assert!(transcript.contains("ANSWER:n"), "{transcript}");

    let decision_event = read_ws_json_until(&mut events, |event| {
        (event["type"] == "approval-decision").then(|| event.clone())
    });
    assert_eq!(decision_event["approved"], false, "{decision_event}");
}

#[test]
fn run_api_creates_lists_reads_events_and_cancels_run_records() {
    let _guard = server_test_lock();
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

    let missing_summary = request_json(port, &format!("GET /v1/runs/{id}/summary HTTP/1.1"), None);
    assert_eq!(missing_summary.status, 404, "{}", missing_summary.body);
    let missing_summary_json: serde_json::Value =
        serde_json::from_str(&missing_summary.body).expect("missing summary error json");
    assert_eq!(missing_summary_json["error"], "summary artifact not found");

    let missing_diff = request_json(port, &format!("GET /v1/runs/{id}/diff HTTP/1.1"), None);
    assert_eq!(missing_diff.status, 404, "{}", missing_diff.body);
    let missing_diff_json: serde_json::Value =
        serde_json::from_str(&missing_diff.body).expect("missing diff error json");
    assert_eq!(missing_diff_json["error"], "diff artifact not found");

    let unknown_summary = request_json(
        port,
        "GET /v1/runs/00000000-0000-0000-0000-000000000000/summary HTTP/1.1",
        None,
    );
    assert_eq!(unknown_summary.status, 404, "{}", unknown_summary.body);
    let unknown_summary_json: serde_json::Value =
        serde_json::from_str(&unknown_summary.body).expect("unknown summary error json");
    assert_eq!(unknown_summary_json["error"], "run not found");

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

#[test]
fn run_api_lists_newest_runs_first() {
    let _guard = server_test_lock();
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

    let older = request_json(
        port,
        "POST /v1/runs HTTP/1.1",
        Some(r#"{"plan_path":"docs/older.md"}"#),
    );
    assert_eq!(older.status, 200, "{}", older.body);
    let older_json: serde_json::Value = serde_json::from_str(&older.body).expect("older run json");
    let older_id = older_json["id"].as_str().expect("older run id");
    let older_created_at = older_json["created_at"]
        .as_str()
        .expect("older created_at")
        .to_string();

    let mut newest_json = None;
    for attempt in 0..20 {
        std::thread::sleep(Duration::from_millis(1));
        let created = request_json(
            port,
            "POST /v1/runs HTTP/1.1",
            Some(&format!(r#"{{"plan_path":"docs/newer-{attempt}.md"}}"#)),
        );
        assert_eq!(created.status, 200, "{}", created.body);
        let created_json: serde_json::Value =
            serde_json::from_str(&created.body).expect("newer run json");
        let created_at = created_json["created_at"]
            .as_str()
            .expect("newer created_at");
        if created_at > older_created_at.as_str() {
            newest_json = Some(created_json);
            break;
        }
    }
    let newest_json = newest_json.expect("newer run timestamp should advance");
    let newest_id = newest_json["id"].as_str().expect("newer run id");

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    let runs = listed_json.as_array().expect("run list");
    assert!(runs.len() >= 2, "{listed_json}");
    assert_eq!(runs[0]["id"], newest_id, "{listed_json}");
    assert_eq!(
        runs.last().expect("oldest run")["id"],
        older_id,
        "{listed_json}"
    );
}

#[test]
fn cancelling_run_blocks_later_tasks_from_starting() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands

### Task 1: Create first file
- [ ] Write first.txt

### Task 2: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("slow-two-task-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");

    wait_for_file(repo.path.join("first.txt"));
    let cancelled = request_json(
        port,
        &format!("POST /v1/runs/{id}/cancel HTTP/1.1"),
        Some("{}"),
    );
    assert_eq!(cancelled.status, 202, "{}", cancelled.body);

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "failed").then(|| json.clone())
    });

    let summary = request_json(port, &format!("GET /v1/runs/{id}/summary HTTP/1.1"), None);
    assert_eq!(summary.status, 200, "{}", summary.body);
    assert!(summary.body.contains("Result: failed"), "{}", summary.body);
    assert!(summary.body.contains("cancelled"), "{}", summary.body);
    assert!(
        summary
            .body
            .contains(&plan_path.to_string_lossy().to_string()),
        "{}",
        summary.body
    );

    let diff = request_json(port, &format!("GET /v1/runs/{id}/diff HTTP/1.1"), None);
    assert_eq!(diff.status, 200, "{}", diff.body);
    assert_eq!(diff.body, "");

    thread::sleep(Duration::from_millis(1200));

    assert!(repo.path.join("first.txt").exists());
    assert!(
        !repo.path.join("second.txt").exists(),
        "cancelled run should not start task 2"
    );
}

#[test]
fn run_api_returns_running_record_before_background_plan_finishes() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("slow-fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");

    assert!(
        !repo.path.join("first.txt").exists(),
        "POST /v1/runs should return before the slow agent finishes"
    );

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert!(
        events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "run_started" && event["status"] == "running"),
        "{events_json}"
    );

    let completed = wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });
    assert_eq!(completed["phase"], "complete");
}

#[test]
fn cancelling_running_run_prevents_late_success_overwrite() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("slow-fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["status"], "running");

    let cancelled = request_json(
        port,
        &format!("POST /v1/runs/{id}/cancel HTTP/1.1"),
        Some("{}"),
    );
    assert_eq!(cancelled.status, 202, "{}", cancelled.body);

    thread::sleep(Duration::from_secs(3));

    let viewed = request_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), None);
    assert_eq!(viewed.status, 200, "{}", viewed.body);
    let viewed_json: serde_json::Value = serde_json::from_str(&viewed.body).expect("view json");
    assert_eq!(viewed_json["phase"], "complete");
    assert_eq!(viewed_json["status"], "failed");

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert!(
        events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "run_cancelled"),
        "{events_json}"
    );
    assert!(
        !events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "run_succeeded"),
        "{events_json}"
    );
    assert!(
        !events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "task_succeeded"),
        "cancelled run must not emit task_succeeded after cancellation: {events_json}"
    );

    let plan = std::fs::read_to_string(&plan_path).expect("read plan after cancelled run");
    assert!(
        plan.contains("- [ ] Write first.txt"),
        "cancelled run must not mark the task complete:\n{plan}"
    );
    assert!(
        !plan.contains("- [x] Write first.txt"),
        "cancelled run must not accept task after cancellation:\n{plan}"
    );
}

#[test]
fn cancelling_run_during_agent_does_not_mark_task_complete_without_validation_or_review() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("slow-fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");

    wait_for_file(repo.path.join("slow-fake-agent-last-prompt.txt"));

    let cancelled = request_json(
        port,
        &format!("POST /v1/runs/{id}/cancel HTTP/1.1"),
        Some("{}"),
    );
    assert_eq!(cancelled.status, 202, "{}", cancelled.body);

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "failed").then(|| json.clone())
    });

    thread::sleep(Duration::from_secs(3));

    let plan = std::fs::read_to_string(&plan_path).expect("read plan after cancelled run");
    assert!(
        plan.contains("- [ ] Write first.txt"),
        "cancelled run must not mark the task complete:\n{plan}"
    );
    assert!(
        !plan.contains("- [x] Write first.txt"),
        "cancelled run must not accept task after cancellation:\n{plan}"
    );

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert!(
        !events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "task_succeeded"),
        "cancelled run must not emit task_succeeded after cancellation: {events_json}"
    );
}

#[test]
fn run_api_archives_rejected_review_attempt_transcripts_after_retry_success() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("retry-after-review-agent.sh").to_string_lossy(),
        "review_command": fixture_path("review-fail-once.sh").to_string_lossy(),
        "require_review": true,
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let attempt_one_review = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/plan-task-1-attempt-1-review.transcript HTTP/1.1"),
        None,
    );
    assert_eq!(
        attempt_one_review.status, 200,
        "{}",
        attempt_one_review.body
    );
    assert!(
        attempt_one_review.body.contains("REVIEW_FAIL"),
        "{}",
        attempt_one_review.body
    );
}

#[test]
fn run_api_executes_plan_with_agent_and_review_agent_shortcuts() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let bin_dir = repo.path.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create fake cli bin dir");
    std::os::unix::fs::symlink(fixture_path("fake-agent.sh"), bin_dir.join("claude"))
        .expect("link fake claude cli");
    std::os::unix::fs::symlink(fixture_path("review-pass.sh"), bin_dir.join("codex"))
        .expect("link fake codex cli");

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let server = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("PATH", path)
        .args(["serve", "--bind", &bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ralphterm serve");
    let mut server = ChildGuard::new(server);
    wait_for_server(port, server.child_mut());

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent": "claude",
        "review_agent": "codex",
        "require_review": true,
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let plan = std::fs::read_to_string(&plan_path).expect("read updated plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read run summary artifact");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(summary.contains("Review transcript:"), "{summary}");
}

#[test]
fn run_api_executes_plan_with_agent_command_and_persists_result_artifacts() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);
    std::fs::create_dir_all(repo.path.join(".ralphterm/progress"))
        .expect("create stale progress dir");
    std::fs::write(
        repo.path
            .join(".ralphterm/progress/unrelated-task-99.transcript"),
        "stale unrelated transcript\n",
    )
    .expect("write unrelated stale progress artifact");

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");
    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let plan = std::fs::read_to_string(&plan_path).expect("read updated plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read run summary artifact");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");

    let diff_path = repo.path.join(format!(".ralphterm/runs/{id}/diff.patch"));
    let diff = std::fs::read_to_string(&diff_path).expect("read run diff artifact");
    assert!(
        diff.contains("diff --git a/first.txt b/first.txt"),
        "{diff}"
    );
    assert!(diff.contains("+created by fake agent"), "{diff}");
    assert!(diff.contains("diff --git a/plan.md b/plan.md"), "{diff}");

    let summary_response = request_json(port, &format!("GET /v1/runs/{id}/summary HTTP/1.1"), None);
    assert_eq!(summary_response.status, 200, "{}", summary_response.body);
    assert_eq!(summary_response.body, summary);

    let summary_json_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/summary.json HTTP/1.1"),
        None,
    );
    assert_eq!(
        summary_json_response.status, 200,
        "{}",
        summary_json_response.body
    );
    assert!(
        summary_json_response
            .content_type
            .starts_with("application/json"),
        "{}",
        summary_json_response.content_type
    );
    let summary_json: serde_json::Value =
        serde_json::from_str(&summary_json_response.body).expect("summary json response");
    assert_eq!(summary_json["plan"], "plan.md");
    assert_eq!(summary_json["result"], "passed");
    assert_eq!(summary_json["tasks"][0]["title"], "Create first file");

    let diff_response = request_json(port, &format!("GET /v1/runs/{id}/diff HTTP/1.1"), None);
    assert_eq!(diff_response.status, 200, "{}", diff_response.body);
    assert_eq!(diff_response.body, diff);

    let run_progress_dir = repo.path.join(format!(".ralphterm/runs/{id}/progress"));
    let transcript = std::fs::read_to_string(run_progress_dir.join("plan-task-1.transcript"))
        .expect("read copied implementation transcript from run artifact directory");
    assert!(transcript.contains("COMPLETED"), "{transcript}");
    let validation = std::fs::read_to_string(run_progress_dir.join("plan-task-1-validation.txt"))
        .expect("read copied validation output from run artifact directory");
    assert!(
        validation.contains("Validation: test -f first.txt"),
        "{validation}"
    );
    let progress_log = std::fs::read_to_string(run_progress_dir.join("plan.log"))
        .expect("read copied progress log from run artifact directory");
    assert!(
        progress_log.contains("task_start number=1"),
        "{progress_log}"
    );
    assert!(
        !run_progress_dir
            .join("unrelated-task-99.transcript")
            .exists(),
        "run artifact directory must not copy stale progress files from unrelated runs"
    );

    let progress_transcript_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/plan-task-1.transcript HTTP/1.1"),
        None,
    );
    assert_eq!(
        progress_transcript_response.status, 200,
        "{}",
        progress_transcript_response.body
    );
    assert_eq!(progress_transcript_response.body, transcript);

    let progress_validation_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/plan-task-1-validation.txt HTTP/1.1"),
        None,
    );
    assert_eq!(
        progress_validation_response.status, 200,
        "{}",
        progress_validation_response.body
    );
    assert_eq!(progress_validation_response.body, validation);

    let progress_log_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/plan.log HTTP/1.1"),
        None,
    );
    assert_eq!(
        progress_log_response.status, 200,
        "{}",
        progress_log_response.body
    );
    assert_eq!(progress_log_response.body, progress_log);

    let unrelated_progress_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/unrelated-task-99.transcript HTTP/1.1"),
        None,
    );
    assert_eq!(
        unrelated_progress_response.status, 404,
        "{}",
        unrelated_progress_response.body
    );
    let unrelated_progress_json: serde_json::Value =
        serde_json::from_str(&unrelated_progress_response.body).expect("progress error json");
    assert_eq!(
        unrelated_progress_json["error"],
        "progress artifact not found"
    );
    assert!(
        !unrelated_progress_response.body.contains(".ralphterm"),
        "{}",
        unrelated_progress_response.body
    );

    let traversal_progress_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/progress/..%2Fsummary.md HTTP/1.1"),
        None,
    );
    assert_ne!(
        traversal_progress_response.status, 200,
        "path traversal must not expose summary artifact: {}",
        traversal_progress_response.body
    );
    assert!(
        !traversal_progress_response.body.contains(".ralphterm"),
        "{}",
        traversal_progress_response.body
    );
    assert!(
        !traversal_progress_response.body.contains(&id.to_string()),
        "{}",
        traversal_progress_response.body
    );

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert!(
        events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "run_succeeded"),
        "{events_json}"
    );
}

#[test]
fn run_api_dry_run_succeeds_without_spawning_agent_or_changing_repo() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    let plan_path = write_committed_api_dry_run_plan(&repo);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": repo.path.join("missing-agent-command").to_string_lossy(),
        "dry_run": true
    })
    .to_string();

    let id = create_and_wait_for_dry_run(port, &body);
    assert_api_dry_run_artifacts_and_clean_repo(port, id, &repo, &plan_path);
}

#[test]
fn run_api_dry_run_without_agent_command_executes_preview() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    let plan_path = write_committed_api_dry_run_plan(&repo);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "dry_run": true
    })
    .to_string();

    let id = create_and_wait_for_dry_run(port, &body);
    assert_api_dry_run_artifacts_and_clean_repo(port, id, &repo, &plan_path);
}

#[test]
fn run_api_dry_run_with_workspace_id_does_not_create_workspace() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    let plan_path = write_committed_api_dry_run_plan(&repo);
    let workspace_path = repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-dry-run-preview");

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

    let body = serde_json::json!({
        "workspace_id": "api-dry-run-preview",
        "plan_path": "plan.md",
        "agent_command": repo.path.join("missing-agent-command").to_string_lossy(),
        "dry_run": true
    })
    .to_string();

    let id = create_and_wait_for_dry_run(port, &body);
    assert_api_dry_run_artifacts_and_clean_repo(port, id, &repo, &plan_path);
    assert!(
        !workspace_path.exists(),
        "API dry-run with workspace_id must not create a worktree at {}",
        workspace_path.display()
    );
}

#[test]
fn run_api_rejects_workspace_plan_paths_that_escape_source_repo_without_creating_run() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);
    std::fs::write(repo.path.join("plan.md"), "# Plan\n").expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add plan"]);

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

    let absolute_body = serde_json::json!({
        "workspace_id": "api-escape-absolute",
        "plan_path": repo.path.join("plan.md").to_string_lossy(),
    })
    .to_string();
    let absolute = request_json(port, "POST /v1/runs HTTP/1.1", Some(&absolute_body));
    assert_eq!(absolute.status, 400, "{}", absolute.body);

    let parent_body = serde_json::json!({
        "workspace_id": "api-escape-parent",
        "plan_path": "../plan.md",
    })
    .to_string();
    let parent = request_json(port, "POST /v1/runs HTTP/1.1", Some(&parent_body));
    assert_eq!(parent.status, 400, "{}", parent.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
    assert!(!repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-escape-absolute")
        .exists());
    assert!(!repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-escape-parent")
        .exists());
}

#[test]
fn run_api_workspace_id_without_agent_command_creates_planning_run_without_workspace_path() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);
    std::fs::write(repo.path.join("plan.md"), "# Plan\n").expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add plan"]);

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

    let body = serde_json::json!({
        "workspace_id": "api-planning-only",
        "plan_path": "plan.md"
    })
    .to_string();
    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    assert_eq!(created_json["phase"], "planning");
    assert_eq!(created_json["status"], "created");
    assert_eq!(created_json["plan_path"], "plan.md");
    assert!(
        created_json.get("workspace_path").is_none(),
        "{created_json}"
    );
    assert!(!repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-planning-only")
        .exists());
}

#[test]
fn run_api_workspace_id_executes_plan_in_isolated_workspace_and_persists_result_artifacts() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let workspace_path = repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-task");
    let body = serde_json::json!({
        "workspace_id": "api-task",
        "plan_path": "plan.md",
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");
    assert_eq!(created_json["plan_path"], "plan.md");
    assert_eq!(
        created_json["workspace_path"],
        workspace_path.to_string_lossy().as_ref()
    );

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let source_plan = std::fs::read_to_string(&plan_path).expect("read source plan");
    assert!(
        source_plan.contains("- [ ] Write first.txt"),
        "{source_plan}"
    );
    assert!(!repo.path.join("first.txt").exists());

    let workspace_plan =
        std::fs::read_to_string(workspace_path.join("plan.md")).expect("read workspace plan");
    assert!(
        workspace_plan.contains("- [x] Write first.txt"),
        "{workspace_plan}"
    );
    assert!(workspace_path.join("first.txt").exists());

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read run summary artifact");
    assert!(summary.contains("Result: passed"), "{summary}");

    let diff_path = repo.path.join(format!(".ralphterm/runs/{id}/diff.patch"));
    let diff = std::fs::read_to_string(&diff_path).expect("read run diff artifact");
    assert!(
        diff.contains("diff --git a/first.txt b/first.txt"),
        "{diff}"
    );
    assert!(diff.contains("diff --git a/plan.md b/plan.md"), "{diff}");

    let summary_response = request_json(port, &format!("GET /v1/runs/{id}/summary HTTP/1.1"), None);
    assert_eq!(summary_response.status, 200, "{}", summary_response.body);
    assert_eq!(summary_response.body, summary);

    let diff_response = request_json(port, &format!("GET /v1/runs/{id}/diff HTTP/1.1"), None);
    assert_eq!(diff_response.status, 200, "{}", diff_response.body);
    assert_eq!(diff_response.body, diff);
}

#[test]
fn run_api_rejects_preexisting_plain_workspace_directory_without_creating_run() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);
    std::fs::write(
        repo.path.join("plan.md"),
        "# Example plan\n\n### Task 1: Write first file\n- [ ] Write first.txt\n",
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

    let plain_workspace_path = repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-isolated");
    std::fs::create_dir_all(&plain_workspace_path).expect("create plain workspace dir");
    std::fs::write(
        plain_workspace_path.join("sentinel.txt"),
        "must not execute here\n",
    )
    .expect("write sentinel");

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

    let body = serde_json::json!({
        "workspace_id": "api-isolated",
        "plan_path": "plan.md",
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
    assert!(plain_workspace_path.join("sentinel.txt").exists());
    assert!(!plain_workspace_path.join("first.txt").exists());
}

#[test]
fn session_without_cwd_uses_server_base_dir_while_workspace_run_changes_process_cwd() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let workspace_path = repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-cwd-race");
    let body = serde_json::json!({
        "workspace_id": "api-cwd-race",
        "plan_path": "plan.md",
        "agent_command": fixture_path("slow-fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();
    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);

    wait_for_file(workspace_path.join("slow-fake-agent-last-prompt.txt"));

    let session_body = serde_json::json!({
        "agent": "codex",
        "prompt": "ignored",
        "command": "/bin/pwd",
        "args": []
    })
    .to_string();
    let created_session = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&session_body));
    assert_eq!(created_session.status, 200, "{}", created_session.body);
    let created_session_json: serde_json::Value =
        serde_json::from_str(&created_session.body).expect("created session json");
    let session_id = created_session_json["id"].as_str().expect("session id");

    let transcript = wait_for_text(
        port,
        &format!("GET /v1/sessions/{session_id}/transcript HTTP/1.1"),
        |text| text.contains(repo.path.to_string_lossy().as_ref()),
    );
    assert!(
        transcript.contains(repo.path.to_string_lossy().as_ref()),
        "{transcript}"
    );
    assert!(
        !transcript.contains(workspace_path.to_string_lossy().as_ref()),
        "{transcript}"
    );
}

#[test]
fn run_api_exposes_reviewing_phase_while_review_command_runs() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

    let review_path = repo.path.join("slow-review.sh");
    std::fs::write(
        &review_path,
        "#!/usr/bin/env sh\nset -eu\ncat > review-prompt.txt\nsleep 3\nprintf 'Review: pass\\nREVIEW_PASS\\n'\n",
    )
    .expect("write slow review script");
    let mut permissions = std::fs::metadata(&review_path)
        .expect("slow review script metadata")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&review_path, permissions)
        .expect("make slow review script executable");

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "review_command": review_path.to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["status"], "running");

    wait_for_json(
        port,
        &format!("GET /v1/runs/{id}/events HTTP/1.1"),
        |json| {
            json.as_array()
                .expect("event list")
                .iter()
                .any(|event| event["type"] == "review_started")
                .then(|| json.clone())
        },
    );
    let viewed = request_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), None);
    assert_eq!(viewed.status, 200, "{}", viewed.body);
    let reviewing: serde_json::Value = serde_json::from_str(&viewed.body).expect("run json");
    assert_eq!(reviewing["phase"], "reviewing", "{reviewing}");
    assert_eq!(reviewing["status"], "running", "{reviewing}");

    let completed = wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });
    assert_eq!(completed["phase"], "complete");
}

#[test]
fn run_api_executes_plan_with_review_command_and_persists_review_transcript() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "review_command": fixture_path("review-pass.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");
    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read run summary artifact");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(summary.contains("Review transcript:"), "{summary}");

    let run_progress_dir = repo.path.join(format!(".ralphterm/runs/{id}/progress"));
    let review_transcript =
        std::fs::read_to_string(run_progress_dir.join("plan-task-1-review.transcript"))
            .expect("read copied review transcript from run artifact directory");
    assert!(
        review_transcript.contains("REVIEW_PASS"),
        "{review_transcript}"
    );
}

#[test]
fn run_api_plan_run_records_structured_task_progress_events() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "review_command": fixture_path("review-pass.sh").to_string_lossy(),
        "no_commit": false
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    let event_types: Vec<_> = events_json
        .as_array()
        .expect("event list")
        .iter()
        .map(|event| event["type"].as_str().expect("event type"))
        .collect();
    assert_eq!(
        event_types,
        vec![
            "run_created",
            "run_started",
            "task_started",
            "validation_passed",
            "review_started",
            "review_passed",
            "task_committed",
            "task_succeeded",
            "run_succeeded",
        ],
        "{events_json}"
    );

    let task_started = events_json
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "task_started")
        .expect("task_started event");
    assert_eq!(task_started["task_number"], 1);
    assert_eq!(task_started["task_title"], "Create first file");

    let committed = events_json
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "task_committed")
        .expect("task_committed event");
    assert!(
        committed["message"]
            .as_str()
            .is_some_and(|message| message.len() >= 7),
        "{events_json}"
    );
    assert!(
        committed["message"]
            .as_str()
            .is_some_and(|message| message.contains("task: Create first file")),
        "{events_json}"
    );
}

#[test]
fn run_api_validation_failure_records_structured_failure_events() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "review_command": fixture_path("review-pass.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "failed").then(|| json.clone())
    });

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    let event_types: Vec<_> = events_json
        .as_array()
        .expect("event list")
        .iter()
        .map(|event| event["type"].as_str().expect("event type"))
        .collect();
    assert!(event_types.contains(&"validation_failed"), "{events_json}");
    assert!(event_types.contains(&"task_failed"), "{events_json}");
    assert!(!event_types.contains(&"review_started"), "{events_json}");
    assert!(!event_types.contains(&"task_committed"), "{events_json}");

    let task_failed = events_json
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "task_failed")
        .expect("task_failed event");
    assert_eq!(task_failed["task_number"], 1);
    assert_eq!(task_failed["task_title"], "Create first file");
    assert!(
        task_failed["message"].as_str().is_some_and(|message| {
            message.contains("validation") || message.contains("command failed")
        }),
        "{events_json}"
    );
}

#[test]
fn run_api_rejects_agent_command_without_plan_path_without_creating_run() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_required_review_without_review_command_without_creating_run() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "require_review": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_workspace_required_review_without_review_command_without_side_effects() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);
    std::fs::write(repo.path.join("plan.md"), "# Plan\n").expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add plan"]);

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

    let body = serde_json::json!({
        "workspace_id": "api-invalid-review",
        "plan_path": "plan.md",
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "require_review": true,
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
    assert!(!repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("api-invalid-review")
        .exists());
}

#[test]
fn run_api_rejects_agent_shortcut_conflicts_without_creating_run() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "agent": "claude",
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    assert!(response.body.contains("agent conflicts with agent_command"));

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_review_agent_shortcut_conflicts_without_creating_run() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "review_agent": "codex",
        "review_command": fixture_path("review-pass.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    assert!(response
        .body
        .contains("review_agent conflicts with review_command"));

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_same_agent_and_review_agent_shortcuts_without_creating_run() {
    let _guard = server_test_lock();
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

    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "agent": "claude",
        "review_agent": "claude",
        "require_review": true,
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    assert!(response.body.contains("must be different"));

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_identical_agent_and_review_commands_without_creating_run() {
    let _guard = server_test_lock();
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

    let command = fixture_path("fake-agent.sh").to_string_lossy().to_string();
    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "agent_command": command,
        "review_command": command,
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_rejects_parsed_equivalent_agent_and_review_commands_without_creating_run() {
    let _guard = server_test_lock();
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

    let command = fixture_path("fake-agent.sh").to_string_lossy().to_string();
    let body = serde_json::json!({
        "plan_path": "docs/plan.md",
        "agent_command": format!("  {command}  "),
        "review_command": format!("'{command}'"),
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 0);
}

#[test]
fn run_api_no_pending_plan_succeeds_and_persists_summary() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Already done
- [x] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(
        &repo.path,
        ["commit", "-m", "docs: add completed test plan"],
    );

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");
    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read run summary artifact");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(summary.contains("No pending tasks."), "{summary}");
}

#[test]
fn run_api_records_failed_execution_when_agent_does_not_complete() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);

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

    let body = serde_json::json!({
        "plan_path": plan_path.to_string_lossy(),
        "agent_command": fixture_path("fake-agent-no-completed.sh").to_string_lossy(),
        "no_commit": true
    })
    .to_string();

    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id");
    assert_eq!(created_json["phase"], "executing");
    assert_eq!(created_json["status"], "running");

    let failed = wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "failed").then(|| json.clone())
    });
    assert_eq!(failed["phase"], "complete");

    let listed = request_json(port, "GET /v1/runs HTTP/1.1", None);
    assert_eq!(listed.status, 200, "{}", listed.body);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(listed_json.as_array().expect("run list").len(), 1);
    assert_eq!(listed_json[0]["id"], id);
    assert_eq!(listed_json[0]["phase"], "complete");
    assert_eq!(listed_json[0]["status"], "failed");

    let events = request_json(port, &format!("GET /v1/runs/{id}/events HTTP/1.1"), None);
    assert_eq!(events.status, 200, "{}", events.body);
    let events_json: serde_json::Value = serde_json::from_str(&events.body).expect("events json");
    assert!(
        events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "run_failed"),
        "{events_json}"
    );
    assert!(
        events_json
            .as_array()
            .expect("event list")
            .iter()
            .any(|event| event["type"] == "task_failed"
                && event["task_number"] == 1
                && event["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("COMPLETED"))),
        "{events_json}"
    );

    let summary_path = repo.path.join(format!(".ralphterm/runs/{id}/summary.md"));
    let summary = std::fs::read_to_string(&summary_path).expect("read failed run summary artifact");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("missing required COMPLETED"), "{summary}");

    let summary_json_response = request_json(
        port,
        &format!("GET /v1/runs/{id}/summary.json HTTP/1.1"),
        None,
    );
    assert_eq!(
        summary_json_response.status, 200,
        "{}",
        summary_json_response.body
    );
    assert!(
        summary_json_response
            .content_type
            .starts_with("application/json"),
        "{}",
        summary_json_response.content_type
    );
    let summary_json: serde_json::Value = serde_json::from_str(&summary_json_response.body)
        .expect("failed run summary json response");
    assert_eq!(summary_json["result"], "failed");
    assert_eq!(summary_json["failed_task"]["title"], "Create first file");
    assert_eq!(summary_json["failed_task"]["phase"], "agent completion");
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
    content_type: String,
    body: String,
}

fn connect_ws(port: u16, path: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect websocket");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .expect("write websocket handshake");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set websocket read timeout");
    let mut raw = Vec::new();
    let mut buf = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut buf)
            .expect("read websocket handshake");
        raw.push(buf[0]);
    }
    let headers = String::from_utf8(raw).expect("websocket handshake utf8");
    assert!(
        headers.starts_with("HTTP/1.1 101"),
        "websocket upgrade failed: {headers}"
    );
    stream
}

fn read_ws_json_until(
    stream: &mut TcpStream,
    predicate: impl Fn(&serde_json::Value) -> Option<serde_json::Value>,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_event = String::new();
    while Instant::now() < deadline {
        if let Some(text) = read_ws_text_frame(stream) {
            last_event = text;
            let json: serde_json::Value =
                serde_json::from_str(&last_event).expect("websocket json");
            if let Some(value) = predicate(&json) {
                return value;
            }
        }
    }
    panic!("timed out waiting for websocket event; last event: {last_event}");
}

fn assert_no_approval_decision_event(stream: &mut TcpStream) {
    let previous_timeout = stream.read_timeout().expect("read websocket timeout");
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set websocket read timeout");
        if let Some(text) = read_ws_text_frame(stream) {
            let json: serde_json::Value = serde_json::from_str(&text).expect("websocket json");
            assert_ne!(json["type"], "approval-decision", "{json}");
        }
    }
    stream
        .set_read_timeout(previous_timeout)
        .expect("restore websocket read timeout");
}

fn read_ws_text_frame(stream: &mut TcpStream) -> Option<String> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).ok()?;
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    let mut len = u64::from(header[1] & 0x7f);
    if len == 126 {
        let mut extended = [0u8; 2];
        stream
            .read_exact(&mut extended)
            .expect("read websocket length");
        len = u64::from(u16::from_be_bytes(extended));
    } else if len == 127 {
        let mut extended = [0u8; 8];
        stream
            .read_exact(&mut extended)
            .expect("read websocket length");
        len = u64::from_be_bytes(extended);
    }
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask).expect("read websocket mask");
    }
    let mut payload = vec![0u8; len as usize];
    stream
        .read_exact(&mut payload)
        .expect("read websocket payload");
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    (opcode == 1).then(|| String::from_utf8(payload).expect("websocket text utf8"))
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local addr").port()
}

fn git<const N: usize>(repo: &std::path::Path, args: [&str; N]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_committed_api_dry_run_plan(repo: &TempDir) -> PathBuf {
    git(&repo.path, ["init"]);
    git(&repo.path, ["config", "user.email", "test@example.com"]);
    git(&repo.path, ["config", "user.name", "Test User"]);

    std::fs::write(repo.path.join(".gitignore"), ".ralphterm/\n").expect("write gitignore");
    let plan_path = repo.path.join("plan.md");
    std::fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    git(&repo.path, ["add", ".gitignore", "plan.md"]);
    git(&repo.path, ["commit", "-m", "docs: add test plan"]);
    plan_path
}

fn create_and_wait_for_dry_run(port: u16, body: &str) -> String {
    let created = request_json(port, "POST /v1/runs HTTP/1.1", Some(body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created run json");
    let id = created_json["id"].as_str().expect("run id").to_string();

    wait_for_json(port, &format!("GET /v1/runs/{id} HTTP/1.1"), |json| {
        (json["status"] == "succeeded").then(|| json.clone())
    });

    id
}

fn assert_api_dry_run_artifacts_and_clean_repo(
    port: u16,
    id: String,
    repo: &TempDir,
    plan_path: &std::path::Path,
) {
    let summary_response = request_json(port, &format!("GET /v1/runs/{id}/summary HTTP/1.1"), None);
    assert_eq!(summary_response.status, 200, "{}", summary_response.body);
    assert!(
        summary_response.body.contains("Dry run: plan.md"),
        "{}",
        summary_response.body
    );
    assert!(
        summary_response.body.contains("Task 1: Create first file"),
        "{}",
        summary_response.body
    );

    let diff_response = request_json(port, &format!("GET /v1/runs/{id}/diff HTTP/1.1"), None);
    assert_eq!(diff_response.status, 200, "{}", diff_response.body);
    assert_eq!(diff_response.body, "");

    let plan = std::fs::read_to_string(plan_path).expect("read plan after dry run");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");
    assert!(!repo.path.join("first.txt").exists());

    let status = Command::new("git")
        .current_dir(&repo.path)
        .args(["status", "--short"])
        .output()
        .expect("git status");
    assert!(status.status.success(), "git status failed");
    assert_eq!(String::from_utf8_lossy(&status.stdout), "");
}

fn server_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("server test lock poisoned")
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
    let content_type = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-type")
                    .then(|| value.trim().to_string())
            })
        })
        .unwrap_or_default();
    Response {
        status,
        content_type,
        body: body.to_string(),
    }
}

fn wait_for_json(
    port: u16,
    request_line: &str,
    predicate: impl Fn(&serde_json::Value) -> Option<serde_json::Value>,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_response = String::new();
    while Instant::now() < deadline {
        let response = request_json(port, request_line, None);
        assert_eq!(response.status, 200, "{}", response.body);
        assert!(
            response.content_type.starts_with("application/json"),
            "{}",
            response.content_type
        );
        last_response = response.body;
        let json: serde_json::Value = serde_json::from_str(&last_response).expect("json response");
        if let Some(value) = predicate(&json) {
            return value;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for JSON predicate; last response: {last_response}");
}

fn wait_for_text(port: u16, request_line: &str, predicate: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_response = String::new();
    while Instant::now() < deadline {
        let response = request_json(port, request_line, None);
        assert_eq!(response.status, 200, "{}", response.body);
        last_response = response.body;
        if predicate(&last_response) {
            return last_response;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for text predicate; last response: {last_response}");
}

fn wait_for_file(path: PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for file {}", path.display());
}
