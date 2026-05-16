use ralphterm::plan::{parse_plan, CheckboxState};

const SAMPLE_PLAN: &str = r#"# Plan: Add auth

## Overview
Build login support.

## Validation Commands
- `cargo test --all`
- `cargo clippy --all-targets -- -D warnings`

### Task 1: Add user model
- [ ] Create user struct
- [ ] Add tests

Some extra context for task one.

### Task 2: Wire login
- [x] Add route
- [ ] Add session cookie

### Notes
- [ ] This checkbox is not a task.
"#;

#[test]
fn parses_validation_commands() {
    let plan = parse_plan(SAMPLE_PLAN).expect("parse plan");

    assert_eq!(
        plan.validation_commands,
        vec![
            "cargo test --all".to_string(),
            "cargo clippy --all-targets -- -D warnings".to_string(),
        ]
    );
}

#[test]
fn parses_task_sections_and_checkboxes() {
    let plan = parse_plan(SAMPLE_PLAN).expect("parse plan");

    assert_eq!(plan.tasks.len(), 2);
    assert_eq!(plan.tasks[0].number, 1);
    assert_eq!(plan.tasks[0].title, "Add user model");
    assert_eq!(plan.tasks[0].checkboxes.len(), 2);
    assert_eq!(plan.tasks[0].checkboxes[0].state, CheckboxState::Open);
    assert_eq!(plan.tasks[0].checkboxes[0].text, "Create user struct");
    assert!(plan.tasks[0].body.contains("Some extra context"));

    assert_eq!(plan.tasks[1].number, 2);
    assert_eq!(plan.tasks[1].checkboxes[0].state, CheckboxState::Done);
    assert_eq!(plan.tasks[1].checkboxes[1].state, CheckboxState::Open);
}

#[test]
fn returns_pending_tasks_only() {
    let plan = parse_plan(SAMPLE_PLAN).expect("parse plan");
    let pending = plan.pending_tasks();

    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].title, "Add user model");
    assert_eq!(pending[1].title, "Wire login");
}
