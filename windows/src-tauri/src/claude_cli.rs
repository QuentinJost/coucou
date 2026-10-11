// Claude Code as a chat provider: each turn runs the Claude Code CLI
// (`claude -p`), so the user's own Claude subscription answers, with no API
// key. The turn goes in as stream-json, with the same content blocks as the
// Anthropic API gets (files, window context), and the CLI's session carries the
// conversation on through --resume. Only web search and fetch are available to
// it: no shell, no file edit, no MCP server, and no hook (Coucou's own would
// report the chat as one more Claude Code session).

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::claude;

/// The provider id, as in settings.json and core/providers.ts.
pub const PROVIDER: &str = "claudecode";
/// The CLI takes family aliases that follow the latest model of each.
pub const DEFAULT_MODEL: &str = "sonnet";
/// A turn may search the web several times before it answers.
const TIMEOUT: Duration = Duration::from_secs(300);

/// The models the CLI knows by alias; no list to fetch, no key needed.
pub fn models() -> Vec<ModelInfo> {
    [("opus", "Claude Opus"), ("sonnet", "Claude Sonnet"), ("haiku", "Claude Haiku")]
        .into_iter()
        .map(|(id, label)| ModelInfo { id: id.into(), label: label.into() })
        .collect()
}

/// One chat turn through the Claude Code CLI that `command` runs.
pub async fn send(chat: &Chat, command: &str, model: &str, query: String, context: Option<ChatContext>) -> Result<ChatReply, String> {
    let turn = chat.begin(PROVIDER);
    let content = claude::user_content(turn.first, context.as_ref(), &query);
    let resume = last_session(&turn.history);
    let (text, session) = match &resume {
        Some(id) => match run(command, model, content.clone(), Some(id)).await {
            // The CLI no longer has that session (another machine, cleaned up):
            // the conversation goes on in a new one, its earlier turns as text.
            Err(err) if err.contains("No conversation found") => run(command, model, with_earlier(&turn.history, content), None).await?,
            other => other?,
        },
        None => run(command, model, with_earlier(&turn.history, content), None).await?,
    };
    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    chat.commit(
        &turn,
        json!({ "role": "user", "content": plain }),
        json!({ "role": "assistant", "content": text, "session": session }),
        &plain,
        &text,
    );
    Ok(ChatReply { text })
}

/// The CLI session that answered last, kept on its answers in the history.
fn last_session(history: &[Value]) -> Option<String> {
    history.iter().rev().find_map(|m| m.get("session").and_then(Value::as_str)).map(str::to_string)
}

/// The turn's content, after the earlier turns as text when there are some no
/// CLI session holds: answered by another provider, or by a session gone since.
fn with_earlier(history: &[Value], mut content: Vec<Value>) -> Vec<Value> {
    let earlier: Vec<String> = history
        .iter()
        .filter_map(|m| {
            let who = if m.get("role")?.as_str()? == "user" { "User" } else { "Assistant" };
            Some(format!("{who}: {}", m.get("content")?.as_str()?))
        })
        .collect();
    if !earlier.is_empty() {
        content.insert(0, json!({ "type": "text", "text": format!("Earlier in this conversation:\n\n{}", earlier.join("\n\n")) }));
    }
    content
}

/// Runs one `claude -p` turn; its answer and the CLI session it ran in.
async fn run(command: &str, model: &str, content: Vec<Value>, resume: Option<&str>) -> Result<(String, String), String> {
    let mut words = command.split_whitespace();
    let program = words.next().ok_or("Claude Code command missing. Open settings.")?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(words).args([
        "-p",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--model", model,
        "--tools", "WebSearch,WebFetch",
        "--allowedTools", "WebSearch,WebFetch",
        "--strict-mcp-config",
        "--settings", r#"{"disableAllHooks":true}"#,
        "--system-prompt", &chat::system_prompt(true),
    ]);
    if let Some(id) = resume {
        cmd.args(["--resume", id]);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| format!("Could not run {program}: {e}"))?;
    let line = json!({ "type": "user", "message": { "role": "user", "content": content } });
    let mut input = serde_json::to_vec(&line).map_err(|e| e.to_string())?;
    input.push(b'\n');
    let mut stdin = child.stdin.take().ok_or("No stdin for Claude Code.")?;
    let write = async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await // EOF: the CLI answers this one turn, then exits
    };
    let (written, output) = tokio::time::timeout(TIMEOUT, async { tokio::join!(write, child.wait_with_output()) })
        .await
        .map_err(|_| "Claude Code took too long.".to_string())?;
    let output = output.map_err(|e| e.to_string())?;
    written.map_err(|e| format!("Could not talk to Claude Code: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    match result_line(&stdout) {
        Some(result) => parse_result(&result),
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Claude Code: {}", stderr.trim().lines().last().unwrap_or("no answer")))
        }
    }
}

/// The `result` event among the stream-json lines.
fn result_line(stdout: &str) -> Option<Value> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("type").and_then(Value::as_str) == Some("result"))
}

/// The answer and session of a `result` event, or what went wrong.
fn parse_result(result: &Value) -> Result<(String, String), String> {
    let text = result.get("result").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if result.get("is_error").and_then(Value::as_bool) == Some(true) {
        let errors = result
            .get("errors")
            .and_then(Value::as_array)
            .map(|e| e.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let why = [text.as_str(), errors.as_str()].into_iter().find(|s| !s.is_empty()).unwrap_or("the turn failed.");
        return Err(format!("Claude Code: {why}"));
    }
    if text.is_empty() {
        return Err("No response text.".into());
    }
    let session = result.get("session_id").and_then(Value::as_str).unwrap_or("").to_string();
    Ok((text, session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_line_gives_the_answer_and_its_session() {
        let out = concat!(
            r#"{"type":"system","subtype":"init"}"#, "\n",
            r#"{"type":"result","is_error":false,"result":" Hi ","session_id":"abc"}"#, "\n",
        );
        let result = result_line(out).unwrap();
        assert_eq!(parse_result(&result).unwrap(), ("Hi".to_string(), "abc".to_string()));
    }

    #[test]
    fn an_error_says_why() {
        let err = parse_result(&json!({ "type": "result", "is_error": true, "errors": ["No conversation found"] })).unwrap_err();
        assert_eq!(err, "Claude Code: No conversation found");
        assert!(parse_result(&json!({ "type": "result", "is_error": false, "result": "  " })).is_err());
    }

    #[test]
    fn the_last_session_is_resumed() {
        let history = vec![
            json!({ "role": "user", "content": "a" }),
            json!({ "role": "assistant", "content": "b", "session": "s1" }),
            json!({ "role": "user", "content": "c" }),
            json!({ "role": "assistant", "content": "d", "session": "s2" }),
        ];
        assert_eq!(last_session(&history).as_deref(), Some("s2"));
        assert_eq!(last_session(&[]), None);
    }

    #[test]
    fn earlier_turns_without_a_session_go_first_as_text() {
        let history = vec![json!({ "role": "user", "content": "hi" }), json!({ "role": "assistant", "content": "hello" })];
        let content = with_earlier(&history, vec![json!({ "type": "text", "text": "next" })]);
        assert_eq!(content[0]["text"], "Earlier in this conversation:\n\nUser: hi\n\nAssistant: hello");
        assert_eq!(content[1]["text"], "next");
        // Nothing earlier: the turn alone.
        assert_eq!(with_earlier(&[], vec![json!({ "type": "text", "text": "x" })]).len(), 1);
    }

    #[test]
    fn the_models_are_the_cli_aliases() {
        let ids: Vec<String> = models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["opus", "sonnet", "haiku"]);
    }
}
