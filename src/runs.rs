use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunPhase {
    Planning,
    Executing,
    Reviewing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Created,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub phase: RunPhase,
    pub status: RunStatus,
    pub plan_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedRunRecord {
    pub id: Uuid,
    pub created_at: String,
    pub phase: RunPhase,
    pub status: RunStatus,
    pub plan_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub status: RunStatus,
    pub timestamp: String,
}

pub struct RunStore;

impl RunStore {
    pub fn create(base_dir: impl AsRef<Path>, request: RunRecord) -> Result<CreatedRunRecord> {
        let now = timestamp();
        let record = CreatedRunRecord {
            id: Uuid::new_v4(),
            created_at: now.clone(),
            phase: request.phase,
            status: request.status,
            plan_path: request.plan_path,
        };
        let run_dir = base_dir
            .as_ref()
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("create run directory {}", run_dir.display()))?;

        let run_json = serde_json::to_string_pretty(&record).context("serialize run record")?;
        fs::write(run_dir.join("run.json"), format!("{run_json}\n"))
            .with_context(|| format!("write {}", run_dir.join("run.json").display()))?;

        let event = RunEvent {
            event_type: "run_created".to_string(),
            status: record.status.clone(),
            timestamp: now,
        };
        let event_json = serde_json::to_string(&event).context("serialize run event")?;
        let mut events = OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join("events.jsonl"))
            .with_context(|| format!("open {}", run_dir.join("events.jsonl").display()))?;
        writeln!(events, "{event_json}").context("append run event")?;

        Ok(record)
    }

    pub fn list(base_dir: impl AsRef<Path>) -> Result<Vec<CreatedRunRecord>> {
        let runs_dir = base_dir.as_ref().join(".ralphterm").join("runs");
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&runs_dir)
            .with_context(|| format!("read runs directory {}", runs_dir.display()))?
        {
            let entry = entry.with_context(|| format!("read entry in {}", runs_dir.display()))?;
            if !entry.file_type().context("read run entry type")?.is_dir() {
                continue;
            }
            let run_json_path = entry.path().join("run.json");
            if !run_json_path.exists() {
                continue;
            }
            records.push(read_record(&run_json_path)?);
        }
        records.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(records)
    }

    pub fn get(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<CreatedRunRecord>> {
        let run_json_path = run_dir(base_dir.as_ref(), id).join("run.json");
        if !run_json_path.exists() {
            return Ok(None);
        }
        read_record(&run_json_path).map(Some)
    }

    pub fn events(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<Vec<RunEvent>>> {
        if Self::get(base_dir.as_ref(), id)?.is_none() {
            return Ok(None);
        }
        let events_path = run_dir(base_dir.as_ref(), id).join("events.jsonl");
        if !events_path.exists() {
            return Ok(Some(Vec::new()));
        }
        let input = fs::read_to_string(&events_path)
            .with_context(|| format!("read {}", events_path.display()))?;
        let mut events = Vec::new();
        for line in input.lines().filter(|line| !line.trim().is_empty()) {
            events.push(serde_json::from_str(line).context("parse run event")?);
        }
        Ok(Some(events))
    }

    pub fn cancel(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<CreatedRunRecord>> {
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        record.phase = RunPhase::Complete;
        record.status = RunStatus::Failed;
        write_record(base_dir.as_ref(), &record)?;
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: "run_cancelled".to_string(),
                status: record.status.clone(),
                timestamp: timestamp(),
            },
        )?;
        Ok(Some(record))
    }
}

fn run_dir(base_dir: &Path, id: Uuid) -> std::path::PathBuf {
    base_dir
        .join(".ralphterm")
        .join("runs")
        .join(id.to_string())
}

fn read_record(path: &Path) -> Result<CreatedRunRecord> {
    let input = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&input).with_context(|| format!("parse {}", path.display()))
}

fn write_record(base_dir: &Path, record: &CreatedRunRecord) -> Result<()> {
    let path = run_dir(base_dir, record.id).join("run.json");
    let run_json = serde_json::to_string_pretty(record).context("serialize run record")?;
    fs::write(&path, format!("{run_json}\n")).with_context(|| format!("write {}", path.display()))
}

fn append_event(base_dir: &Path, id: Uuid, event: RunEvent) -> Result<()> {
    let path = run_dir(base_dir, id).join("events.jsonl");
    let event_json = serde_json::to_string(&event).context("serialize run event")?;
    let mut events = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(events, "{event_json}").context("append run event")
}

fn timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis();
    format!("unix-ms:{millis}")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use serde_json::Value;

    use super::{RunPhase, RunRecord, RunStatus, RunStore};

    #[test]
    fn create_writes_run_json_and_initial_event() {
        let temp =
            std::env::temp_dir().join(format!("ralphterm-run-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Planning,
                status: RunStatus::Created,
                plan_path: Some("docs/plan.md".into()),
            },
        )
        .unwrap();

        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        let run_json = fs::read_to_string(run_dir.join("run.json")).unwrap();
        let persisted: Value = serde_json::from_str(&run_json).unwrap();
        assert_eq!(persisted["id"], record.id.to_string());
        assert_eq!(persisted["phase"], "planning");
        assert_eq!(persisted["status"], "created");
        assert_eq!(persisted["plan_path"], "docs/plan.md");
        assert!(!persisted["created_at"].as_str().unwrap().is_empty());

        let events = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let lines: Vec<_> = events.lines().collect();
        assert_eq!(lines.len(), 1);
        let event: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event["type"], "run_created");
        assert_eq!(event["status"], "created");
        assert!(!event["timestamp"].as_str().unwrap().is_empty());

        fs::remove_file(run_dir.join("events.jsonl")).unwrap();
        let missing_events = RunStore::events(&temp, record.id).unwrap().unwrap();
        assert!(
            missing_events.is_empty(),
            "missing event log for an existing run should return an empty event list"
        );

        remove_dir_all_if_exists(&temp);
    }

    fn remove_dir_all_if_exists(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
    }
}
