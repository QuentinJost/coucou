// Orchestra panel — the tasks of the Claude Code orchestra mod (`/orch`), read
// from the queue files it keeps, one per orchestrator session:
// <config>/orchestra/queues/<session>.json. Read only: Coucou never writes there.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// An approved task stays on the panel this long, then leaves it.
const DONE_SHOWN_MS: f64 = 10.0 * 60.0 * 1000.0;
/// A queue file not saved for this long belongs to an orchestrator long gone.
const STALE_MS: f64 = 24.0 * 3600.0 * 1000.0;
const TITLE_MAX: usize = 90;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OrchTask {
    pub id: u64,
    /// The orchestrator session's name.
    pub orchestrator: String,
    /// The repo's last path segment.
    pub repo: String,
    pub role: String,
    /// queued, working, checking, reviewing, approved, blocked or escalated.
    pub state: String,
    pub round: u64,
    /// The session holding the task: its reviewer while in review, else its worker.
    pub session: String,
    pub model: String,
    /// The task's first line.
    pub title: String,
    pub updated_at: f64,
}

/// The queues folder: the setting when given (a leading `~/` is the home
/// folder), else the default Claude Code config folder's.
pub fn queues_dir(setting: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    let setting = setting.trim();
    if setting.is_empty() {
        return home.join(".claude").join("orchestra").join("queues");
    }
    match setting.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(setting),
    }
}

/// Every task worth showing, across the queue files of `dir`: the ones in
/// flight first, then those waiting for the person, the queued ones, the
/// freshly approved ones.
pub fn snapshot(dir: &Path, now_ms: f64) -> Vec<OrchTask> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut tasks: Vec<OrchTask> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .flat_map(|saved| tasks_of(&saved, now_ms))
        .collect();
    tasks.sort_by(|a, b| rank(&a.state).cmp(&rank(&b.state)).then(a.id.cmp(&b.id)));
    tasks
}

fn rank(state: &str) -> u8 {
    match state {
        "working" | "checking" | "reviewing" => 0,
        "blocked" | "escalated" => 1,
        "queued" => 2,
        _ => 3,
    }
}

fn tasks_of(saved: &Value, now_ms: f64) -> Vec<OrchTask> {
    let saved_at = saved.get("savedAt").and_then(Value::as_f64).unwrap_or(0.0);
    if now_ms - saved_at > STALE_MS {
        return Vec::new();
    }
    let orchestrator = str_of(saved, "owner");
    let workers = saved.get("workers").and_then(Value::as_array).cloned().unwrap_or_default();
    let model_of = |name: &str| {
        workers
            .iter()
            .find(|w| w.get("name").and_then(Value::as_str) == Some(name))
            .map(|w| str_of(w, "model"))
    };

    let Some(list) = saved.get("tasks").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|task| {
            let state = str_of(task, "state");
            let updated_at = task.get("updatedAt").and_then(Value::as_f64).unwrap_or(saved_at);
            let shown = match state.as_str() {
                "dropped" | "" => false,
                "approved" => now_ms - updated_at < DONE_SHOWN_MS,
                _ => true,
            };
            if !shown {
                return None;
            }
            let reviewing = state == "reviewing";
            let session = str_of(task, if reviewing { "reviewer" } else { "worker" });
            let sizing = task.get("sizing").and_then(|s| s.get(if reviewing { "review" } else { "work" }));
            let model = model_of(&session)
                .filter(|m| !m.is_empty())
                .or_else(|| sizing.map(|s| str_of(s, "model")))
                .unwrap_or_default();
            Some(OrchTask {
                id: task.get("id").and_then(Value::as_u64).unwrap_or(0),
                orchestrator: orchestrator.clone(),
                repo: str_of(task, "repo").rsplit('/').next().unwrap_or("").to_string(),
                role: str_of(task, "role"),
                state,
                round: task.get("round").and_then(Value::as_u64).unwrap_or(1),
                session,
                model,
                title: title_of(&str_of(task, "text")),
                updated_at,
            })
        })
        .collect()
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= TITLE_MAX {
        return line.to_string();
    }
    let cut: String = line.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: f64 = 1_000_000_000.0;

    fn saved() -> Value {
        json!({
            "owner": "orch-main",
            "sessionId": "s1",
            "savedAt": NOW - 1000.0,
            "workers": [{ "name": "w-front", "model": "claude-opus-5", "task": 2 }],
            "tasks": [
                { "id": 1, "repo": "/app/mtg", "role": "lead-backend", "text": "Queued one", "state": "queued", "round": 1,
                  "worker": "", "reviewer": "", "updatedAt": NOW, "sizing": { "work": { "model": "opus" }, "review": { "model": "sonnet" } } },
                { "id": 2, "repo": "/app/bluspark-stack/core/bluspark-frontend", "role": "lead-frontend",
                  "text": "\n  Build the login page\nwith details", "state": "working", "round": 2,
                  "worker": "w-front", "reviewer": "", "updatedAt": NOW },
                { "id": 3, "repo": "/app/mtg", "role": "qa-automation", "text": "Review me", "state": "reviewing", "round": 1,
                  "worker": "w-qa", "reviewer": "r-3", "updatedAt": NOW, "sizing": { "work": { "model": "opus" }, "review": { "model": "sonnet" } } },
                { "id": 4, "repo": "/app/mtg", "role": "x", "text": "Old", "state": "approved", "updatedAt": NOW - DONE_SHOWN_MS - 1.0 },
                { "id": 5, "repo": "/app/mtg", "role": "x", "text": "Fresh", "state": "approved", "updatedAt": NOW - 1.0 },
                { "id": 6, "repo": "/app/mtg", "role": "x", "text": "Gone", "state": "dropped", "updatedAt": NOW }
            ]
        })
    }

    #[test]
    fn keeps_open_and_fresh_tasks_with_their_session() {
        let tasks = tasks_of(&saved(), NOW);
        let ids: Vec<u64> = tasks.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 5]);

        let front = &tasks[1];
        assert_eq!(front.repo, "bluspark-frontend");
        assert_eq!(front.title, "Build the login page");
        assert_eq!(front.session, "w-front");
        assert_eq!(front.model, "claude-opus-5");
        assert_eq!(front.round, 2);
        assert_eq!(front.orchestrator, "orch-main");

        let review = &tasks[2];
        assert_eq!(review.session, "r-3");
        assert_eq!(review.model, "sonnet");
    }

    #[test]
    fn a_stale_queue_shows_nothing() {
        assert!(tasks_of(&saved(), NOW + STALE_MS + 1.0).is_empty());
    }

    #[test]
    fn snapshot_reads_the_folder_and_puts_work_in_flight_first() {
        let dir = std::env::temp_dir().join(format!("coucou-orch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.json"), saved().to_string()).unwrap();
        std::fs::write(dir.join("broken.json"), b"{").unwrap();
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
        let states: Vec<String> = snapshot(&dir, NOW).into_iter().map(|t| t.state).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(states, vec!["working", "reviewing", "queued", "approved"]);
    }

    #[test]
    fn queues_dir_expands_the_home_folder() {
        let home = queues_dir("").parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(queues_dir("~/dev/x"), home.join("dev/x"));
        assert_eq!(queues_dir("/abs/q"), PathBuf::from("/abs/q"));
    }

    #[test]
    fn long_titles_are_cut() {
        let t = title_of(&"a".repeat(200));
        assert_eq!(t.chars().count(), TITLE_MAX);
        assert!(t.ends_with('…'));
    }
}
