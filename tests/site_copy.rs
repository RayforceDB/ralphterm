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
        site_index.contains("Write a plan. Let real terminal agents execute it."),
        "landing page should lead with the concrete plan-runner product promise"
    );
    assert!(
        site_index.contains("ralphterm run docs/plans/example.md --dry-run"),
        "landing page should show the safe plan preview command"
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
        docs_index.contains("REVIEW_FAIL</code> triggers one retry"),
        "getting started copy should describe retry behavior before final failure"
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
