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
    for expected_href in ["/", "/docs/"] {
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
fn milestone_docs_name_the_real_acceptance_gate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let milestone_md =
        std::fs::read_to_string(root.join("docs/milestones/m1-autonomous-engineering.md"))
            .expect("read milestone markdown");
    let milestone_html = std::fs::read_to_string(root.join("site/docs/milestone-one.html"))
        .expect("read milestone page");

    for (name, text) in [
        ("milestone markdown", milestone_md.as_str()),
        ("public milestone page", milestone_html.as_str()),
    ] {
        assert!(
            text.contains("implement -> validate -> independent-review -> accept/commit"),
            "{name} should describe the acceptance gate as implementation, validation, independent review, then acceptance"
        );
        assert!(
            !text.contains("self-review"),
            "{name} should not imply self-review is the product boundary"
        );
        assert!(
            !text.contains("external-review"),
            "{name} should name independent review instead of external-review"
        );
    }
}

#[test]
fn public_docs_navigation_targets_existing_landing_sections() {
    let site_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    let site_index =
        std::fs::read_to_string(site_root.join("index.html")).expect("read site index");

    // Collect every id="..." value present on the landing page.
    let mut landing_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut search = site_index.as_str();
    let needle = "id=\"";
    while let Some(pos) = search.find(needle) {
        let after = &search[pos + needle.len()..];
        if let Some(end) = after.find('"') {
            landing_ids.insert(after[..end].to_string());
            search = &after[end + 1..];
        } else {
            break;
        }
    }

    let docs_dir = site_root.join("docs");
    let entries = std::fs::read_dir(&docs_dir).expect("read docs dir");
    for entry in entries {
        let entry = entry.expect("read docs entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("html") {
            continue;
        }
        let display = path
            .strip_prefix(&site_root)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        let html =
            std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {display}: {err}"));

        // Walk every href="/#..." occurrence in the docs page.
        let mut hay = html.as_str();
        let href_needle = "href=\"/#";
        while let Some(pos) = hay.find(href_needle) {
            let after = &hay[pos + href_needle.len()..];
            let end = after
                .find('"')
                .unwrap_or_else(|| panic!("{display} has unterminated href=\"/#...\""));
            let fragment = &after[..end];
            assert!(
                landing_ids.contains(fragment),
                "{display} links to /#{fragment} but landing page has no matching id=\"{fragment}\""
            );
            hay = &after[end + 1..];
        }
    }
}

#[test]
fn landing_page_leads_with_plan_execution_not_pty_api() {
    let site_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    let site_index =
        std::fs::read_to_string(site_root.join("index.html")).expect("read site index");

    let h1_start = site_index
        .find("<h1")
        .expect("landing page should have an h1");
    let h1_open_end = site_index[h1_start..]
        .find('>')
        .map(|offset| h1_start + offset + 1)
        .expect("landing h1 opening tag should close");
    let h1_close = site_index[h1_open_end..]
        .find("</h1>")
        .map(|offset| h1_open_end + offset)
        .expect("landing h1 should close");
    let h1_text = site_index[h1_open_end..h1_close].to_lowercase();
    // Hero must speak to the workflow story: a plan, walking away,
    // unattended automation. NOT internal API mechanics.
    assert!(
        h1_text.contains("plan")
            || h1_text.contains("walk")
            || h1_text.contains("unattended")
            || h1_text.contains("session")
            || h1_text.contains("ai coding"),
        "landing hero h1 should describe the plan-and-walk-away workflow (got: {h1_text:?})"
    );

    let head_len = site_index.len().min(3000);
    let head = &site_index[..head_len];
    for forbidden in ["/v1/sessions", "POST /v1/sessions", "Current API"] {
        assert!(
            !head.contains(forbidden),
            "landing page lead must not surface PTY-API copy {forbidden:?} in the first 3000 chars"
        );
    }

    assert!(
        site_index.contains("\u{2014} capabilities"),
        "landing page should expose the capabilities spec-sheet eyebrow"
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
fn workflow_docs_define_acceptance_gates_before_checked_progress() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflows_md =
        std::fs::read_to_string(root.join("docs/workflows.md")).expect("read workflows markdown");
    let workflows_html = std::fs::read_to_string(root.join("site/docs/workflows.html"))
        .expect("read public workflows page");

    for (name, text) in [
        ("workflows markdown", workflows_md.as_str()),
        ("public workflows page", workflows_html.as_str()),
    ] {
        assert!(
            text.contains("Acceptance gates"),
            "{name} should have a compact acceptance gates section"
        );
        assert!(
            text.contains("agent completion is only the first gate"),
            "{name} should say agent completion alone is not accepted progress"
        );
        let ordered_gates = [
            "implementation signal",
            "validation pass",
            "independent review pass",
            "plan checkbox + commit",
        ];
        let mut previous = 0;
        for expected in ordered_gates {
            let index = text
                .find(expected)
                .unwrap_or_else(|| panic!("{name} should name the {expected} acceptance gate"));
            assert!(
                index >= previous,
                "{name} should list acceptance gates in execution order"
            );
            previous = index;
        }
        assert!(
            text.contains("COMPLETED") && text.contains("REVIEW_PASS"),
            "{name} should name the concrete implementation and review signals"
        );
        assert!(
            text.contains("unless `--no-commit` is set")
                || text.contains("unless <code>--no-commit</code> is set"),
            "{name} should not imply every accepted task creates a commit"
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
        for phase in [
            "planning",
            "executing",
            "validating",
            "reviewing",
            "complete",
        ] {
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
fn dashboard_run_table_surfaces_workspace_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dashboard_html =
        std::fs::read_to_string(root.join("dashboard/index.html")).expect("read dashboard html");
    let dashboard_js =
        std::fs::read_to_string(root.join("dashboard/app.js")).expect("read dashboard js");

    assert!(
        dashboard_html.contains("<th scope=\"col\">Workspace</th>"),
        "dashboard runs table should expose the workspace path recorded on run intake"
    );
    assert!(
        dashboard_js.contains("cell(run.workspace_path)"),
        "dashboard runs table should render the workspace_path field from run records"
    );
    assert!(
        dashboard_html.contains("<tr><td colspan=\"7\">Loading runs…</td></tr>"),
        "dashboard loading row should span all run table columns"
    );
    assert!(
        dashboard_js.contains("renderEmptyRow(runsBody, 'No runs yet.', 7)"),
        "dashboard empty row should span all run table columns"
    );
    assert!(
        dashboard_js.contains("renderErrorRow(runsBody, error.message, 7)"),
        "dashboard error row should span all run table columns"
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
fn landing_hero_describes_what_problem_ralphterm_solves() {
    // The site no longer leads with "drop-in for ralphex" — that's
    // covered separately in the migration guide. The hero now has to
    // describe the actual problem: long, multi-prompt, unattended AI
    // coding sessions. Pin enough vocabulary that the lead can't
    // silently regress to internal-API or marketing-positioning copy.
    let site_index = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/index.html"),
    )
    .expect("read site index");

    let h1_marker = "<h1 class=\"rt-display\"";
    let h1_start = site_index
        .find(h1_marker)
        .expect("landing page should have the rt-display h1");
    let h1_open_end = site_index[h1_start..]
        .find('>')
        .map(|offset| h1_start + offset + 1)
        .expect("landing h1 opening tag should close");
    let h1_close = site_index[h1_open_end..]
        .find("</h1>")
        .map(|offset| h1_open_end + offset)
        .expect("landing h1 should close");
    let hero_section_end = site_index[h1_close..]
        .find("</section>")
        .map(|offset| h1_close + offset)
        .unwrap_or(site_index.len());
    let hero_body = site_index[h1_close..hero_section_end].to_lowercase();

    // At least one each: the workflow (plan + walk-away + cross-review)
    // and the mechanism (which agents drive it). Lets the marketing
    // text shift without false-positive-ing on every paragraph rewrite.
    let workflow_terms = [
        "plan",
        "walk away",
        "unattended",
        "cross-review",
        "review",
        "implement",
        "agent",
    ];
    let mechanism_terms = ["claude", "codex", "pty", "interactive", "agent"];
    assert!(
        workflow_terms.iter().any(|w| hero_body.contains(w)),
        "hero body should describe the workflow problem (any of {workflow_terms:?}); got: {hero_body:?}"
    );
    assert!(
        mechanism_terms.iter().any(|w| hero_body.contains(w)),
        "hero body should name the mechanism (any of {mechanism_terms:?}); got: {hero_body:?}"
    );
}

#[test]
fn migration_guide_page_exists_and_describes_swap() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("site/docs/migrate-from-ralphex.html"),
    )
    .expect("read migration guide");
    for expected in [
        "Install ralphterm",
        "Point your scripts at ralphex",
        "--tasks-only",
    ] {
        assert!(
            page.contains(expected),
            "migration guide should mention {expected}"
        );
    }
}

#[test]
fn ralphex_compat_page_lists_full_flag_table() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/ralphex-compat.html"),
    )
    .expect("read ralphex compat page");
    let required_flags = [
        "--tasks-only",
        "--review",
        "--external-only",
        "--codex-only",
        "--max-iterations",
        "--review-patience",
        "--task-model",
        "--review-model",
        "--claude-command",
        "--claude-args",
        "--external-review-tool",
        "--custom-review-script",
        "--base-ref",
        "--session-timeout",
        "--idle-timeout",
        "--wait",
        "--worktree",
        "--branch",
        "--serve",
        "--port",
        "--host",
        "--watch",
        "--debug",
        "--no-color",
    ];
    let mut present = 0;
    for flag in required_flags {
        if page.contains(flag) {
            present += 1;
        }
    }
    assert!(
        present >= 20,
        "ralphex compat page should list at least 20 ralphex flag names (found {present})"
    );
}

#[test]
fn cli_reference_page_exists() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/cli.html"),
    )
    .expect("read cli reference page");
    assert!(
        page.contains("<dl"),
        "cli reference page should use definition lists"
    );
    let candidate_flags = [
        "--tasks-only",
        "--review",
        "--external-only",
        "--codex-only",
        "--max-iterations",
        "--review-patience",
        "--task-model",
        "--review-model",
        "--claude-command",
        "--claude-args",
        "--external-review-tool",
        "--custom-review-script",
        "--worktree",
        "--branch",
        "--serve",
        "--port",
        "--host",
        "--watch",
        "--debug",
        "--no-color",
        "--docker",
        "--docker-image",
    ];
    let mut present = 0;
    for flag in candidate_flags {
        if page.contains(flag) {
            present += 1;
        }
    }
    assert!(
        present >= 10,
        "cli reference should list at least 10 flag names (found {present})"
    );
}

#[test]
fn providers_page_documents_four_providers() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/providers.html"),
    )
    .expect("read providers page");
    for provider in ["codex", "copilot", "gemini", "opencode"] {
        assert!(
            page.contains(provider),
            "providers page should document {provider}"
        );
    }
}

#[test]
fn notifications_page_documents_four_channels() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/notifications.html"),
    )
    .expect("read notifications page");
    for channel in ["Telegram", "Slack", "Email", "Webhook"] {
        assert!(
            page.contains(channel),
            "notifications page should document {channel}"
        );
    }
}

#[test]
fn docker_page_documents_image_flag() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/docs/docker.html"),
    )
    .expect("read docker page");
    assert!(
        page.contains("--docker-image"),
        "docker page should document --docker-image"
    );
}

#[test]
fn sitemap_includes_new_docs_pages() {
    let sitemap = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/sitemap.xml"),
    )
    .expect("read sitemap");
    for url in [
        "https://ralphterm.rayforcedb.com/docs/migrate-from-ralphex.html",
        "https://ralphterm.rayforcedb.com/docs/ralphex-compat.html",
        "https://ralphterm.rayforcedb.com/docs/cli.html",
        "https://ralphterm.rayforcedb.com/docs/providers.html",
        "https://ralphterm.rayforcedb.com/docs/notifications.html",
        "https://ralphterm.rayforcedb.com/docs/docker.html",
    ] {
        assert!(
            sitemap.contains(url),
            "sitemap should include new doc URL {url}"
        );
    }
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

#[test]
fn landing_links_to_self_hosted_fonts_only() {
    let html = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    assert!(
        !html.contains("fonts.googleapis.com") && !html.contains("fonts.gstatic.com"),
        "landing page must not reference Google Fonts CDN at runtime"
    );
}

#[test]
fn stylesheet_uses_ibm_plex_via_font_face() {
    let css = std::fs::read_to_string("site/assets/styles.css").expect("read styles.css");
    assert!(
        css.contains("IBM Plex Sans") && css.contains("IBM Plex Mono"),
        "styles.css must reference both IBM Plex Sans and IBM Plex Mono"
    );
    let fonts_css = std::fs::read_to_string("site/assets/fonts.css").expect("read fonts.css");
    assert!(
        fonts_css.matches("@font-face").count() >= 7,
        "fonts.css must declare at least seven @font-face rules"
    );
}

#[test]
fn landing_brand_color_is_monochrome() {
    let css = std::fs::read_to_string("site/assets/styles.css").expect("read styles.css");
    assert!(
        !css.contains("#00d992"),
        "styles.css must not contain the legacy brand color #00d992"
    );
    let landing = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    assert!(
        !landing.contains("#00d992"),
        "site/index.html must not contain the legacy brand color #00d992"
    );
}

#[test]
fn landing_logo_is_pixel_grid() {
    let svg = std::fs::read_to_string("site/assets/logo.svg").expect("read logo.svg");
    let rect_count = svg.matches("<rect").count();
    assert_eq!(
        rect_count, 9,
        "logo.svg must contain exactly 9 <rect> elements (got {rect_count})"
    );
}

#[test]
fn favicon_svg_present_and_matches_logo_pattern() {
    let svg = std::fs::read_to_string("site/assets/favicon.svg").expect("read favicon.svg");
    let dark_rect_count = svg.matches("fill=\"#0a0a0b\"").count();
    assert!(
        dark_rect_count >= 9,
        "favicon.svg must render the 9-cell pixel grid in dark color (counted {dark_rect_count})"
    );
}

#[test]
fn landing_uses_spec_sheet_eyebrows() {
    let html = std::fs::read_to_string("site/index.html").expect("read site/index.html");
    for marker in &["— capabilities", "— how it works", "— invoke", "— install"] {
        assert!(
            html.contains(marker),
            "landing must include the '{marker}' eyebrow"
        );
    }
}

#[test]
fn webmanifest_references_new_icons() {
    let manifest = std::fs::read_to_string("site/site.webmanifest").expect("read webmanifest");
    assert!(
        manifest.contains("\"src\": \"/assets/favicon.svg\""),
        "webmanifest must reference favicon.svg in its icons[]"
    );
    assert!(
        manifest.contains("\"src\": \"/assets/apple-touch-icon.png\""),
        "webmanifest must reference apple-touch-icon.png in its icons[]"
    );
}
