#[test]
fn landing_page_leads_with_plan_execution_not_pty_api() {
    let site_index = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("site/index.html"),
    )
    .expect("read site index");

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
        site_index.contains("without <code>claude -p</code>"),
        "landing page should state that RalphTerm does not use Claude prompt mode"
    );
}
