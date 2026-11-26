//! The adapter extension of the end-to-end tests: the adapter of the
//! made-up tool `testtool`. It is not the adapter of a real tool.
//!
//! It maps four hook events of `testtool` to status reports. The data of
//! each event is a JSON object:
//!
//! | Event | Data | Report |
//! |---|---|---|
//! | `prompt` | `model`, `session`, `resume` | The Agent run works, with the details. The resume command is `resume`, or `testtool --resume <session>`. |
//! | `stop` | none | The Agent run is done. |
//! | `tool` | `tool` | The Agent run works (soft), with the name of the tool. |
//! | `task` | `id`, `parent`, `phase` (`start` or `end`), `title` | A Child work starts or ends. |
//!
//! After each event it sends the extension event `mapped` with the name of
//! the hook event. Its install plan adds one hook for each event to
//! `~/.testtool/settings.json`.

use nomux_extension::{Call, CallError, Extension, RunError};
use nomux_proto::wire::v1::{
    AgentDetails, AgentReport, AgentReportResult, AgentState, ChildWorkChange, ChildWorkReport,
    HookAnswer, HookEvent, HookInstallPlan, HookPlatform, SettingsEntry, SettingsPath,
};
use serde_json::Value;

/// The identifier of the extension. The manifest of the test package has
/// the same identifier.
const ID: &str = "nomux.test-adapter";
const SETTINGS: &str = "~/.testtool/settings.json";
const EVENTS: [&str; 4] = ["prompt", "tool", "task", "stop"];

/// The payload of the event `mapped`.
#[derive(Clone, PartialEq, prost::Message)]
struct Mapped {
    #[prost(string, tag = "1")]
    event: String,
}

fn main() -> Result<(), RunError> {
    Extension::new(ID, env!("CARGO_PKG_VERSION"))
        .method("hook", hook)
        .method("hooks.plan", |_: &Call, (): ()| Ok(plan()))
        .run()
}

fn hook(call: &Call, event: HookEvent) -> Result<HookAnswer, CallError> {
    let data: Value = serde_json::from_slice(&event.payload).unwrap_or_default();
    let text = |key: &str| data[key].as_str().unwrap_or_default().to_owned();
    let host = call.host();
    match event.event.as_str() {
        "prompt" => report(
            call,
            &AgentReport {
                session_id: event.session_id,
                state: AgentState::Working.into(),
                details: Some(AgentDetails {
                    model: text("model"),
                    provider_session_id: text("session"),
                    resume_command: match text("resume") {
                        resume if resume.is_empty() => {
                            format!("testtool --resume {}", text("session"))
                        }
                        resume => resume,
                    },
                }),
                ..AgentReport::default()
            },
        )?,
        "stop" => report(
            call,
            &AgentReport {
                session_id: event.session_id,
                state: AgentState::Done.into(),
                ..AgentReport::default()
            },
        )?,
        "tool" => report(
            call,
            &AgentReport {
                session_id: event.session_id,
                state: AgentState::Working.into(),
                soft: true,
                tool: text("tool"),
                ..AgentReport::default()
            },
        )?,
        "task" => {
            let ends = text("phase") == "end";
            let child = ChildWorkReport {
                session_id: event.session_id,
                change: if ends {
                    ChildWorkChange::End
                } else {
                    ChildWorkChange::Start
                }
                .into(),
                id: text("id"),
                parent_id: text("parent"),
                state: if ends {
                    AgentState::Done
                } else {
                    AgentState::Working
                }
                .into(),
                title: text("title"),
                ..ChildWorkReport::default()
            };
            host.call::<_, AgentReportResult>("agent.child", &child)?;
        }
        _ => {}
    }
    host.emit("mapped", &Mapped { event: event.event })?;
    Ok(HookAnswer::default())
}

fn report(call: &Call, report: &AgentReport) -> Result<(), CallError> {
    call.host()
        .call::<_, AgentReportResult>("agent.report", report)
        .map(drop)
}

fn plan() -> HookInstallPlan {
    let path = |platform: HookPlatform| SettingsPath {
        platform: platform.into(),
        path: SETTINGS.to_owned(),
    };
    HookInstallPlan {
        settings_paths: vec![path(HookPlatform::Linux), path(HookPlatform::Macos)],
        entries: EVENTS
            .iter()
            .map(|event| SettingsEntry {
                pointer: format!("/hooks/{event}"),
                value_json: format!(r#"{{"command":"nomux hook testtool {event}"}}"#),
            })
            .collect(),
    }
}
