use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::symlink,
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
    assert!(
        js.body.contains("/v1/runs/${run.id}/progress"),
        "{}",
        js.body
    );
    assert!(js.body.contains("/v1/runs/${run.id}/events"), "{}", js.body);
    assert!(html.body.contains("Workspace"), "{}", html.body);
    assert!(js.body.contains("cell(run.workspace_path)"), "{}", js.body);
    assert!(
        js.body.contains("Summary artifact for run ${run.id}"),
        "{}",
        js.body
    );
    assert!(
        js.body
            .contains("Progress artifact index for run ${run.id}"),
        "{}",
        js.body
    );
    assert!(
        js.body
            .contains("renderErrorRow(runsBody, error.message, 7)"),
        "{}",
        js.body
    );
}

#[test]
fn dashboard_run_form_posts_reviewed_plan_run_request() {
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
        html.body.contains("Start reviewed plan run"),
        "{}",
        html.body
    );
    for expected in [
        "id=\"run-form\"",
        "id=\"run-plan-path\"",
        "name=\"plan_path\"",
        "id=\"run-agent\"",
        "name=\"agent\"",
        "id=\"run-agent-command\"",
        "name=\"agent_command\"",
        "id=\"run-review-agent\"",
        "name=\"review_agent\"",
        "<option value=\"codex\" selected>codex</option>",
        "id=\"run-review-command\"",
        "name=\"review_command\"",
        "id=\"run-require-review\"",
        "name=\"require_review\"",
        "id=\"run-dry-run\"",
        "name=\"dry_run\"",
        "id=\"run-no-commit\"",
        "name=\"no_commit\"",
        "id=\"run-max-review-retries\"",
        "name=\"max_review_retries\"",
        "id=\"run-agent-timeout-ms\"",
        "name=\"agent_timeout_ms\"",
        "value=\"1\"",
        "id=\"run-submit\"",
        "id=\"run-form-status\"",
    ] {
        assert!(
            html.body.contains(expected),
            "missing {expected}: {}",
            html.body
        );
    }

    let js = request_json(port, "GET /dashboard/app.js HTTP/1.1", None);
    assert_eq!(js.status, 200, "{}", js.body);
    for expected in [
        "document.querySelector('#run-form')",
        "fetch('/v1/runs', {",
        "method: 'POST'",
        "plan_path",
        "agent_command",
        "review_command",
        "require_review",
        "dry_run",
        "no_commit",
        "max_review_retries",
        "agent_timeout_ms",
        "agent timeout must be a positive integer",
        "runSubmit.disabled = true",
        "runSubmit.disabled = false",
        "agent and agent_command are mutually exclusive",
        "review_agent and review_command are mutually exclusive",
        "loadRuns()",
    ] {
        assert!(
            js.body.contains(expected),
            "missing {expected}: {}",
            js.body
        );
    }
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
fn session_resize_applies_to_pty_master() {
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
        "prompt": "go",
        "command": "/bin/sh",
        "args": ["-lc", "read line; stty size > resized.txt; echo COMPLETED"],
        "cols": 80,
        "rows": 24
    })
    .to_string();
    let created = request_json(port, "POST /v1/sessions HTTP/1.1", Some(&body));
    assert_eq!(created.status, 200, "{}", created.body);
    let created_json: serde_json::Value =
        serde_json::from_str(&created.body).expect("created session json");
    let id = created_json["id"].as_str().expect("session id");

    let resize_body = serde_json::json!({"cols": 121, "rows": 37}).to_string();
    let resized = request_json(
        port,
        &format!("POST /v1/sessions/{id}/resize HTTP/1.1"),
        Some(&resize_body),
    );
    assert_eq!(resized.status, 202, "{}", resized.body);

    wait_for_text(
        port,
        &format!("GET /v1/sessions/{id}/transcript HTTP/1.1"),
        |text| text.contains("COMPLETED"),
    );
    wait_for_json(port, &format!("GET /v1/sessions/{id} HTTP/1.1"), |json| {
        (json["status"] == "exited").then(|| json.clone())
    });

    let resized_path = repo.path.join("resized.txt");
    wait_for_file(resized_path.clone());
    let size = std::fs::read_to_string(resized_path).expect("read resized size");
    assert_eq!(size.trim(), "37 121");
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

// removed obsolete test cancelling_run_blocks_later_tasks_from_starting (legacy per-task model)

// removed obsolete test run_api_exposes_progress_log_while_run_is_running (legacy per-task model)

// removed obsolete test run_api_returns_running_record_before_background_plan_finishes (legacy per-task model)

// removed obsolete test run_api_records_task_marked_complete_before_task_success (legacy per-task model)

// removed obsolete test run_api_records_resume_started_when_rerunning_failed_task (legacy per-task model)

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

// removed obsolete test cancelling_run_during_agent_does_not_mark_task_complete_without_validation_or_review (legacy per-task model)

// removed obsolete test run_api_archives_rejected_review_attempt_transcripts_after_retry_success (legacy per-task model)

// removed obsolete test run_api_terminal_review_failure_event_artifact_is_fetchable (legacy per-task model)

// removed obsolete test run_api_executes_plan_with_agent_and_review_agent_shortcuts (legacy per-task model)

// removed obsolete test run_api_executes_plan_with_agent_command_and_persists_result_artifacts (legacy per-task model)

// removed obsolete test run_api_dry_run_succeeds_without_spawning_agent_or_changing_repo (legacy per-task model)

// removed obsolete test run_api_dry_run_without_agent_command_executes_preview (legacy per-task model)

// removed obsolete test run_api_dry_run_resolves_relative_plan_path_inside_repo_path (legacy per-task model)

#[test]
fn run_api_rejects_repo_path_with_workspace_id() {
    let _guard = server_test_lock();
    let repo = TempDir::new();
    std::fs::write(repo.path.join("plan.md"), "# Plan\n").expect("write plan");

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
        "repo_path": repo.path.to_string_lossy(),
        "workspace_id": "ambiguous-workspace",
        "plan_path": "plan.md",
        "dry_run": true
    })
    .to_string();

    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    let error: serde_json::Value = serde_json::from_str(&response.body).expect("error json");
    assert!(
        error["error"].as_str().is_some_and(|message| {
            message.contains("repo_path") && message.contains("workspace_id")
        }),
        "{}",
        response.body
    );
}

// removed obsolete test run_api_dry_run_canonicalizes_symlink_repo_path_in_metadata (legacy per-task model)

#[test]
fn run_api_rejects_repo_path_symlink_plan_path_that_escapes_repo() {
    let _guard = server_test_lock();
    let daemon_dir = TempDir::new();
    let root = TempDir::new();
    let target_repo = root.path.join("repo");
    let outside_dir = root.path.join("outside");
    std::fs::create_dir(&target_repo).expect("create target repo");
    std::fs::create_dir(&outside_dir).expect("create outside dir");
    std::fs::write(
        outside_dir.join("plan.md"),
        r#"# Outside symlink plan

### Task 1: Outside symlink task
- [ ] This plan must not be read
"#,
    )
    .expect("write outside plan");
    symlink("../outside/plan.md", target_repo.join("link.md")).expect("create plan symlink");

    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let server = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&daemon_dir.path)
        .args(["serve", "--bind", &bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ralphterm serve");
    let mut server = ChildGuard::new(server);
    wait_for_server(port, server.child_mut());

    let body = serde_json::json!({
        "repo_path": target_repo.to_string_lossy(),
        "plan_path": "link.md",
        "dry_run": true
    })
    .to_string();

    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    let error: serde_json::Value = serde_json::from_str(&response.body).expect("error json");
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| message.contains("plan_path must stay inside repo_path")),
        "{}",
        response.body
    );
}

#[test]
fn run_api_rejects_repo_path_plan_path_that_escapes_repo() {
    let _guard = server_test_lock();
    let daemon_dir = TempDir::new();
    let root = TempDir::new();
    let target_repo = root.path.join("repo");
    let outside_dir = root.path.join("outside");
    std::fs::create_dir(&target_repo).expect("create target repo");
    std::fs::create_dir(&outside_dir).expect("create outside dir");
    std::fs::write(
        outside_dir.join("plan.md"),
        r#"# Outside plan

### Task 1: Outside task
- [ ] This plan must not be read
"#,
    )
    .expect("write outside plan");

    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let server = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&daemon_dir.path)
        .args(["serve", "--bind", &bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ralphterm serve");
    let mut server = ChildGuard::new(server);
    wait_for_server(port, server.child_mut());

    let body = serde_json::json!({
        "repo_path": target_repo.to_string_lossy(),
        "plan_path": "../outside/plan.md",
        "dry_run": true
    })
    .to_string();

    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    let error: serde_json::Value = serde_json::from_str(&response.body).expect("error json");
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| message.contains("plan_path") && message.contains("repo_path")),
        "{}",
        response.body
    );
}

#[test]
fn run_api_rejects_repo_path_without_dry_run() {
    let _guard = server_test_lock();
    let daemon_dir = TempDir::new();
    let target_repo = TempDir::new();
    std::fs::create_dir(target_repo.path.join("docs")).expect("create docs dir");
    std::fs::write(target_repo.path.join("docs/plan.md"), "# Plan\n").expect("write plan");

    let port = free_port();
    let bind = format!("127.0.0.1:{port}");
    let server = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&daemon_dir.path)
        .args(["serve", "--bind", &bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ralphterm serve");
    let mut server = ChildGuard::new(server);
    wait_for_server(port, server.child_mut());

    let body = serde_json::json!({
        "repo_path": target_repo.path.to_string_lossy(),
        "plan_path": "docs/plan.md",
        "dry_run": false
    })
    .to_string();

    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    let error: serde_json::Value = serde_json::from_str(&response.body).expect("error json");
    assert_eq!(error["error"], "repo_path currently supports dry_run only");
}

// removed obsolete test run_api_dry_run_with_workspace_id_does_not_create_workspace (legacy per-task model)

// removed obsolete test run_api_dry_run_summary_json_uses_plan_data_when_no_tasks_pending (legacy per-task model)

// removed obsolete test run_api_dry_run_does_not_dirty_git_status_when_ralphterm_is_not_ignored (legacy per-task model)

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

// removed obsolete test run_api_workspace_id_executes_plan_in_isolated_workspace_and_persists_result_artifacts (legacy per-task model)

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

// removed obsolete test run_api_reports_validating_phase_while_validation_command_is_running (legacy per-task model)

// removed obsolete test run_api_returns_to_executing_phase_after_validation_before_next_no_review_task (legacy per-task model)

// removed obsolete test run_api_exposes_reviewing_phase_while_review_command_runs (legacy per-task model)

// removed obsolete test run_api_executes_plan_with_review_command_and_persists_review_transcript (legacy per-task model)

// removed obsolete test run_api_plan_run_records_structured_task_progress_events (legacy per-task model)

// removed obsolete test run_api_validation_failure_records_structured_failure_events (legacy per-task model)

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
fn run_api_rejects_zero_agent_timeout_without_creating_run() {
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
        "agent_timeout_ms": 0
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    assert!(
        response.body.contains("agent_timeout_ms"),
        "{}",
        response.body
    );

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
fn run_api_dry_run_rejects_default_agent_and_same_review_agent_without_creating_run() {
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
        "review_agent": "claude",
        "require_review": true,
        "dry_run": true,
        "no_commit": true
    })
    .to_string();
    let response = request_json(port, "POST /v1/runs HTTP/1.1", Some(&body));
    assert_eq!(response.status, 400, "{}", response.body);
    assert!(
        response.body.contains("must be different"),
        "{}",
        response.body
    );

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

// removed obsolete test run_api_no_pending_plan_succeeds_and_persists_summary (legacy per-task model)

// removed obsolete test run_api_records_failed_execution_when_agent_does_not_complete (legacy per-task model)

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

fn server_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    // Tests that panic while holding the lock would otherwise poison the
    // mutex and cascade every subsequent test into a `PoisonError`. The
    // critical section here protects shared state (free ports, shared
    // server binary state), and panics already failed the test that held
    // the lock; recovery is safe.
    match lock.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
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
