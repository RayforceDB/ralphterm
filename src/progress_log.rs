use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct ProgressLog {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl ProgressLog {
    pub fn open(repo_root: &Path, plan_slug: &str) -> Result<Self> {
        let dir = repo_root.join(".ralphterm").join("progress");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create progress dir {}", dir.display()))?;
        let path = dir.join(format!("progress-{plan_slug}.txt"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open progress log {}", path.display()))?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write a control line — no timestamp. Used for phase headers and
    /// other framing emitted by the orchestrator itself.
    pub fn write_control(&mut self, line: &str) -> Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Write a narration line prefixed with `[YYYY-MM-DD HH:MM:SS] `.
    /// Used for every agent-produced line of output.
    pub fn write_narration(&mut self, line: &str) -> Result<()> {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        writeln!(self.writer, "[{}] {}", ts, line)?;
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_control_and_timestamped_narration_lines() {
        let tmp = std::env::temp_dir().join(format!(
            "rt-progress-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut log = ProgressLog::open(&tmp, "hello").unwrap();
        log.write_control("creating branch: hello").unwrap();
        log.write_narration("Picking Task 1.").unwrap();
        drop(log);

        let body =
            std::fs::read_to_string(tmp.join(".ralphterm/progress/progress-hello.txt")).unwrap();
        assert!(body.contains("creating branch: hello\n"));
        assert!(
            body.contains("] Picking Task 1.\n"),
            "missing timestamped narration; body:\n{body}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
