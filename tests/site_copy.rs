#[test]
fn docs_site_exposes_reviewed_plan_workflow_page() {
    let docs_index = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/index.html"),
    )
    .expect("read docs index");
    let workflows = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/workflows.html"),
    )
    .expect("read workflows page");

    assert!(
        docs_index.contains("/docs/workflows.html"),
        "docs index should link to the public workflows page"
    );
    for expected_href in ["/#what", "/#how", "/#api", "/docs/"] {
        assert!(
            workflows.contains(&format!("href=\"{expected_href}\"")),
            "workflows page navigation should link to existing landing-page target {expected_href}"
        );
    }
    for expected in [
        "REVIEW_PASS",
        "REVIEW_FAIL retry",
        "resume after a failed run",
        "validation output",
        "transcripts",
        "commit progress",
        "Plan-level validation commands",
        "dry run fails before agent execution",
        "same command is rejected in dry-run too",
    ] {
        assert!(
            workflows.contains(expected),
            "workflows page should mention {expected}"
        );
    }
    assert!(
        !workflows.contains("Each task can declare validation commands"),
        "workflows page must not claim unsupported per-task validation declarations"
    );
}

#[test]
fn public_docs_navigation_targets_existing_landing_sections() {
    let site_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    let site_index =
        std::fs::read_to_string(site_root.join("index.html")).expect("read site index");
    for docs_page in [
        "docs/index.html",
        "docs/api.html",
        "docs/architecture.html",
        "docs/security.html",
        "docs/milestone-one.html",
        "docs/workflows.html",
    ] {
        let html = std::fs::read_to_string(site_root.join(docs_page))
            .unwrap_or_else(|err| panic!("read {docs_page}: {err}"));
        for fragment in ["how", "what", "api"] {
            let href = format!("href=\"/#{fragment}\"");
            let target = format!("id=\"{fragment}\"");
            assert!(
                html.contains(&href),
                "{docs_page} should link to landing section /#{fragment}"
            );
            assert!(
                site_index.contains(&target),
                "landing page should expose target {target} for {docs_page}"
            );
        }
        for stale_href in [
            "href=\"/#why\"",
            "href=\"/#product\"",
            "href=\"/#workflow\"",
        ] {
            assert!(
                !html.contains(stale_href),
                "{docs_page} should not link to missing landing section {stale_href}"
            );
        }
    }
}

#[test]
fn landing_page_leads_with_plan_execution_not_pty_api() {
    let site_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    let site_index =
        std::fs::read_to_string(site_root.join("index.html")).expect("read site index");
    let docs_index =
        std::fs::read_to_string(site_root.join("docs/index.html")).expect("read docs index");
    let workflows = std::fs::read_to_string(site_root.join("docs/workflows.html"))
        .expect("read workflows page");

    assert!(
        site_index.contains("Write a plan. Let real terminal agents execute it, then review it."),
        "landing page should lead with plan execution plus cross-review verification"
    );
    let product_loop = "RalphTerm runs the maintainer loop directly: implementation agent, validation commands, independent reviewer, then accepted progress.";
    assert!(
        site_index.contains(product_loop),
        "landing page should state the verified plan-runner loop before PTY plumbing"
    );
    let product_loop_index = site_index
        .find(product_loop)
        .expect("landing page should contain verified plan-runner loop");
    let pty_index = site_index
        .find("real PTYs")
        .expect("landing page should still mention real PTY execution");
    assert!(
        product_loop_index < pty_index,
        "landing page should explain the verified plan-runner loop before low-level PTY plumbing"
    );
    assert!(
        site_index.contains("ralphterm run docs/plans/example.md --dry-run"),
        "landing page should show the safe plan preview command"
    );
    assert!(
        site_index.contains("ralphterm run docs/plans/example.md --dry-run \\")
            && site_index.contains("--require-review \\")
            && site_index.contains("--review-agent codex")
            && site_index.contains("Review: codex"),
        "landing hero should preview the reviewed plan path, not an unreviewed smoke path"
    );
    assert!(
        !site_index.contains("Review: skipped"),
        "landing hero should not lead with skipped review when review is the product boundary"
    );
    assert!(
        site_index.contains("ralphterm run docs/plans/example.md --agent claude"),
        "landing page should show the plan runner command"
    );
    assert!(
        workflows.contains("--review-command"),
        "workflows page should show that plan runs can require an independent review gate"
    );
    assert!(
        workflows.contains("REVIEW_FAIL retry"),
        "workflows page should explain that an initial REVIEW_FAIL retries implementation"
    );
    assert!(
        !docs_index.contains("REVIEW_FAIL</code> leaves the task unchecked"),
        "getting started copy should not imply the first REVIEW_FAIL immediately blocks without retry"
    );
    assert!(
        docs_index.contains("<code>REVIEW_FAIL</code> triggers one retry")
            && docs_index.contains("<code>--max-review-retries N</code>"),
        "getting started copy should describe the default retry behavior and configurable retry budget"
    );
    assert!(
        site_index.contains("without <code>claude -p</code>"),
        "landing page should state that RalphTerm does not use Claude prompt mode"
    );
}

#[test]
fn repo_docs_describe_review_retry_before_blocking() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README");
    let product =
        std::fs::read_to_string(root.join("docs/product.md")).expect("read product brief");

    assert!(
        readme.contains("REVIEW_FAIL` triggers one retry"),
        "README should describe the first REVIEW_FAIL as retry feedback, not final rejection"
    );
    assert!(
        readme.contains("--review-command") && readme.contains("--review-agent"),
        "README should tell users either review configuration satisfies --require-review"
    );
    assert!(
        readme.contains("second review failure leaves the task unchecked"),
        "README should describe final blocking only after the retry fails review"
    );
    assert!(
        readme.contains("--max-review-retries N"),
        "README should document the configurable review retry budget"
    );
    assert!(
        product.contains("cross-review step is the product boundary"),
        "product brief should center cross-review verification, not merely launching agents"
    );
}

#[test]
fn public_docs_mention_review_agent_as_supported_review_config() {
    let site_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    let docs_index =
        std::fs::read_to_string(site_root.join("docs/index.html")).expect("read docs index");
    let workflows = std::fs::read_to_string(site_root.join("docs/workflows.html"))
        .expect("read workflows page");

    for (name, html) in [("docs index", docs_index), ("workflows page", workflows)] {
        assert!(
            html.contains("--review-command") && html.contains("--review-agent"),
            "{name} should document both supported review configuration paths"
        );
        assert!(
            !html.contains("unless <code>--review-command</code> is also supplied"),
            "{name} should not imply --review-command is the only valid review configuration"
        );
    }
}

#[test]
fn docs_explain_workspace_isolated_plan_runs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README");
    let getting_started = std::fs::read_to_string(root.join("docs/getting-started.md"))
        .expect("read getting started markdown");
    let workflows =
        std::fs::read_to_string(root.join("docs/workflows.md")).expect("read workflows markdown");
    let docs_index =
        std::fs::read_to_string(root.join("site/docs/index.html")).expect("read public docs index");
    let workflows_html = std::fs::read_to_string(root.join("site/docs/workflows.html"))
        .expect("read public workflows page");

    for (name, text) in [
        ("README", readme.as_str()),
        ("getting started", getting_started.as_str()),
        ("workflows", workflows.as_str()),
        ("public docs index", docs_index.as_str()),
        ("public workflows", workflows_html.as_str()),
    ] {
        assert!(
            text.contains("--workspace-id"),
            "{name} should show the --workspace-id option"
        );
        assert!(
            text.contains(".ralphterm/workspaces/<id>")
                || text.contains(".ralphterm/workspaces/&lt;id&gt;"),
            "{name} should name the managed workspace directory"
        );
        assert!(
            text.contains("does not auto-clean"),
            "{name} should say plan runs preserve managed worktrees"
        );
    }

    for (name, text) in [
        ("getting started", docs_index),
        ("workflows", workflows_html),
    ] {
        assert!(
            text.contains("caller-relative plan path"),
            "{name} should explain plan paths are resolved relative to the caller before switching workspace"
        );
        assert!(
            text.contains("dry run only previews"),
            "{name} should explain dry-run does not create the workspace"
        );
    }
}

#[test]
fn docs_describe_run_api_as_asynchronous_and_expose_artifacts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README");
    let api_markdown =
        std::fs::read_to_string(root.join("docs/api.md")).expect("read API markdown");
    let api_html =
        std::fs::read_to_string(root.join("site/docs/api.html")).expect("read public API docs");

    for endpoint in [
        "POST /v1/runs",
        "GET  /v1/runs",
        "GET  /v1/runs/:id",
        "GET  /v1/runs/:id/events",
        "GET  /v1/runs/:id/summary",
        "GET  /v1/runs/:id/diff",
        "GET  /v1/runs/:id/progress",
        "GET  /v1/runs/:id/progress/:artifact",
        "POST /v1/runs/:id/cancel",
    ] {
        assert!(
            readme.contains(endpoint),
            "README current API list should expose run endpoint {endpoint}"
        );
        assert!(
            api_markdown.contains(endpoint),
            "API markdown current API list should expose run endpoint {endpoint}"
        );
        assert!(
            api_html.contains(endpoint),
            "public API docs should expose run endpoint {endpoint}"
        );
    }

    for (name, text) in [
        ("API markdown", api_markdown),
        ("public API docs", api_html),
    ] {
        assert!(
            text.contains("returns as soon as the run has started"),
            "{name} should state POST /v1/runs is asynchronous"
        );
        assert!(
            text.contains("\"phase\": \"executing\"") && text.contains("\"status\": \"running\""),
            "{name} should show the immediate running response, not a completed response"
        );
        assert!(
            text.contains("poll <code>GET /v1/runs/:id</code>")
                || text.contains("Poll <code>GET /v1/runs/:id</code>")
                || text.contains("poll `GET /v1/runs/:id`")
                || text.contains("Poll `GET /v1/runs/:id`"),
            "{name} should tell API users how to observe completion"
        );
        assert!(
            text.contains("\"phase\": \"planning\"")
                || text.contains("phase: \"planning\"")
                || text.contains("phase: &quot;planning&quot;"),
            "{name} should document planning phase when agent_command is omitted"
        );
        assert!(
            text.contains("\"status\": \"created\"")
                || text.contains("status: \"created\"")
                || text.contains("status: &quot;created&quot;"),
            "{name} should document created status when agent_command is omitted"
        );
        for phase in ["planning", "executing", "reviewing", "complete"] {
            assert!(
                text.contains(phase),
                "{name} should document run phase value {phase}"
            );
        }
        assert!(
            text.contains("reviewing means the independent review command or agent is active"),
            "{name} should explain the reviewing phase as the independent review command/agent being active"
        );
        assert!(
            text.contains("workspace_path") && text.contains("dry_run"),
            "{name} should document that API dry runs with workspace_id preview workspace_path"
        );
    }
}

#[test]
fn public_api_endpoint_list_includes_list_sessions() {
    let api_html = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/api.html"),
    )
    .expect("read public API docs");

    assert!(
        api_html.contains("GET  /v1/sessions\n"),
        "public API endpoint list should include the session list endpoint as its own line"
    );
}

#[test]
fn dashboard_surfaces_review_gate_state() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dashboard_html =
        std::fs::read_to_string(root.join("dashboard/index.html")).expect("read dashboard html");
    let dashboard_js =
        std::fs::read_to_string(root.join("dashboard/app.js")).expect("read dashboard js");

    assert!(
        dashboard_html.contains("<th scope=\"col\">Gate</th>"),
        "dashboard runs table should expose the active acceptance gate"
    );
    assert!(
        dashboard_js.contains("/events"),
        "dashboard should read run events instead of inferring review state from status alone"
    );
    for expected_event in [
        "task_started",
        "validation_passed",
        "review_started",
        "review_failed",
        "review_passed",
        "agent_retry_started",
        "task_failed",
        "task_marked_complete",
        "task_succeeded",
        "task_committed",
    ] {
        assert!(
            dashboard_js.contains(expected_event),
            "dashboard gate mapping should handle {expected_event} events"
        );
    }
    assert!(
        dashboard_js.contains("renderRunRows(runsWithEvents)"),
        "run rendering should receive event-enriched run records"
    );
}

#[test]
fn api_docs_describe_session_approval_pending_field() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let api_markdown = std::fs::read_to_string(root.join("docs/api.md")).expect("read api docs");
    let api_html =
        std::fs::read_to_string(root.join("site/docs/api.html")).expect("read public api docs");

    for (name, text) in [
        ("markdown api docs", api_markdown.as_str()),
        ("public api docs", api_html.as_str()),
    ] {
        assert!(
            text.contains("approval_pending"),
            "{name} should document the session approval_pending response field"
        );
        assert!(
            text.contains("pending approval"),
            "{name} should explain that approval_pending means a session is waiting for approval"
        );
    }
}

#[test]
fn getting_started_shows_minimal_plan_file_shape() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_markdown =
        std::fs::read_to_string(root.join("docs/getting-started.md")).expect("read docs markdown");
    let docs_html =
        std::fs::read_to_string(root.join("site/docs/index.html")).expect("read public docs index");

    for (name, text) in [("markdown docs", docs_markdown), ("public docs", docs_html)] {
        assert!(
            text.contains("## Validation Commands"),
            "{name} should show where plan-level validation commands go"
        );
        assert!(
            text.contains("- [ ]"),
            "{name} should show unchecked task items that RalphTerm can mark complete"
        );
        assert!(
            text.contains("reviewer sees the transcript, validation output, and git diff"),
            "{name} should explain the evidence sent through the review gate"
        );
    }
}

#[test]
fn dashboard_run_form_supports_isolated_workspace_runs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dashboard_html =
        std::fs::read_to_string(root.join("dashboard/index.html")).expect("read dashboard html");
    let dashboard_js =
        std::fs::read_to_string(root.join("dashboard/app.js")).expect("read dashboard js");

    assert!(
        dashboard_html.contains("name=\"workspace_id\""),
        "dashboard reviewed run form should let maintainers choose an isolated workspace id"
    );
    assert!(
        dashboard_html.contains("placeholder=\"docs-slice\""),
        "workspace id field should show the same concrete example used in docs"
    );
    assert!(
        dashboard_js.contains("workspace_id"),
        "dashboard run request body should send workspace_id to POST /v1/runs"
    );
}

#[test]
fn dashboard_run_form_validates_review_retry_budget_before_post() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dashboard_html =
        std::fs::read_to_string(root.join("dashboard/index.html")).expect("read dashboard html");
    let dashboard_js =
        std::fs::read_to_string(root.join("dashboard/app.js")).expect("read dashboard js");

    assert!(
        dashboard_html.contains("name=\"max_review_retries\"")
            && dashboard_html.contains("type=\"number\"")
            && dashboard_html.contains("min=\"0\"")
            && dashboard_html.contains("value=\"1\""),
        "dashboard retry budget field should preserve the visible default and nonnegative hint"
    );
    assert!(
        dashboard_js.contains("parseMaxReviewRetries(formData.get('max_review_retries'))"),
        "dashboard should parse max_review_retries explicitly so blank values can default to 1"
    );
    assert!(
        dashboard_js.contains("Number.isInteger(body.max_review_retries)")
            && dashboard_js.contains("body.max_review_retries < 0"),
        "dashboard should reject non-integer and negative retry budgets before POST /v1/runs"
    );

    let validation_index = dashboard_js
        .find("const validationError = validateRunRequestBody(body);")
        .expect("dashboard should validate the run request body");
    let post_index = dashboard_js
        .find("method: 'POST'")
        .expect("dashboard should POST valid run requests");
    assert!(
        validation_index < post_index,
        "dashboard should validate retry budget before POST /v1/runs"
    );
}

#[test]
fn api_docs_expose_reviewed_run_api_not_only_raw_sessions() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let api_markdown = std::fs::read_to_string(root.join("docs/api.md")).expect("read api docs");
    let workflows_html = std::fs::read_to_string(root.join("site/docs/workflows.html"))
        .expect("read public workflows docs");
    let api_html =
        std::fs::read_to_string(root.join("site/docs/api.html")).expect("read public api docs");

    for (name, text) in [
        ("markdown api docs", api_markdown),
        ("public api docs", api_html),
    ] {
        assert!(
            text.contains("POST /v1/runs"),
            "{name} should document the run API entry point"
        );
        assert!(
            text.contains("GET /v1/runs/:id/events"),
            "{name} should document run event polling"
        );
        assert!(
            text.contains("GET /v1/runs/:id/summary") && text.contains("GET /v1/runs/:id/diff"),
            "{name} should document HTTP access to persisted run artifacts"
        );
        assert!(
            text.contains("GET /v1/runs/:id/summary.json") && text.contains("summary.json"),
            "{name} should document machine-readable run summaries"
        );
        assert!(
            text.contains("runner-generated"),
            "{name} should make clear summary.json is produced by the plan runner"
        );
        assert!(
            text.contains("accepted") && text.contains("acceptance_gates"),
            "{name} should document machine-readable acceptance gates instead of forcing API callers to infer task acceptance"
        );
        assert!(
            text.contains("review_command") && text.contains("require_review"),
            "{name} should show how API callers require independent review"
        );
        assert!(
            text.contains("summary.md") && text.contains("diff.patch"),
            "{name} should document persisted run result artifacts"
        );
    }

    assert!(
        workflows_html.contains("summary.json"),
        "public workflows docs should document machine-readable run summaries"
    );
}
