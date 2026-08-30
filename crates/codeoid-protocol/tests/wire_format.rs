//! Wire-format invariants.
//!
//! The TS daemon expects camelCase everywhere except on `type:` discriminators
//! (snake_case) and `SessionMode` (kebab-case). A serde mistake on our side
//! causes silent field drop on receive and silent field drop on send — the
//! single worst class of bug we can ship. This test builds a sample of every
//! type we serialize, dumps it to JSON, and asserts no snake_case leaks.

use codeoid_protocol::{
    Attachment, CancelReason, ClientMessage, ConfirmedBy, ContentPart, DaemonMessage, ErrorCode,
    FleetDelta, FleetScope, FleetTask, FleetTaskKind, FleetTaskShape, FleetTaskStatus,
    IdentityType, MessageIdentity, MessageRole, SearchScope, SendPriority, SessionInfo,
    SessionMessage, SessionMessageDelta, SessionMode, SessionRole, SessionStatus, SessionUsage,
    ToolInfo, ToolState,
};
use serde::Serialize;
use serde_json::Value;

/// Keys allowed to contain `_` on the wire. Protocol-level type
/// discriminants carry dotted names (`session.message`) and a handful of
/// snake_case enum values (`waiting_approval`, `tool_call`). These are in
/// VALUES, not KEYS, and are checked separately.
fn is_snake_case_key(k: &str) -> bool {
    k.contains('_')
}

/// Walk a `serde_json::Value` and return every object *key* that contains
/// an underscore. Field names are what the TS daemon matches on — values
/// are allowed to be snake_case (they're enum discriminants).
fn collect_snake_keys(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if is_snake_case_key(k) {
                    out.push((path.to_string(), k.clone()));
                }
                let nested = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_snake_keys(v, &nested, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let nested = format!("{path}[{i}]");
                collect_snake_keys(v, &nested, out);
            }
        }
        _ => {}
    }
}

fn assert_no_snake_case_keys<T: Serialize>(sample: T, label: &str) {
    let value = serde_json::to_value(sample).expect("serializes");
    let mut offenders = Vec::new();
    collect_snake_keys(&value, "", &mut offenders);
    assert!(
        offenders.is_empty(),
        "snake_case field names leaked for {label}:\n  {offenders:#?}\n  serialized = {value:#?}"
    );
}

// -----------------------------------------------------------------------------
// Samples — keep these shaped the way the real daemon emits. If a field gets
// added to the protocol, add it here so the rename-audit covers it.
// -----------------------------------------------------------------------------

fn sample_identity() -> MessageIdentity {
    MessageIdentity {
        sub: "spiffe://x/y".into(),
        name: Some("Alice".into()),
        kind: IdentityType::Human,
    }
}

fn sample_session_info() -> SessionInfo {
    SessionInfo {
        id: "s".into(),
        name: "Demo".into(),
        workdir: "/tmp".into(),
        status: SessionStatus::Working,
        created_by: "me".into(),
        created_at: "2026-04-22T00:00:00Z".into(),
        attached_clients: 1,
        role: Some(SessionRole::Conductor),
        mode: Some(SessionMode::Interactive),
        turns_remaining: Some(10),
        pinned_files: Some(vec!["README.md".into()]),
        agent_uri: None,
        subagents: None,
        usage: Some(SessionUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_creation_tokens: 4,
            total_cost_usd: 0.05,
            num_turns: 6,
            duration_ms: 7,
            recent_turns: None,
            peak_input_tokens: Some(8),
            last_turn_input_tokens: Some(9),
            last_turn_output_tokens: Some(10),
            last_turn_cost_usd: Some(0.11),
            last_turn_cache_hit_rate: Some(0.12),
        }),
        rotation: None,
        queued_messages: Some(0),
        model: Some("claude-opus-4-7".into()),
        fallback_model: None,
        provider_id: None,
        forked_from: Some(codeoid_protocol::ForkedFrom {
            session_id: "parent-1".into(),
            name: "Sandbox".into(),
            at_turn: 12,
        }),
        worktree: Some(codeoid_protocol::SessionWorktree {
            path: "/repo-worktrees/fix-a1b2".into(),
            branch: "codeoid/fix-a1b2".into(),
            created_by_codeoid: true,
        }),
        collaboration: Some(codeoid_protocol::CollaborationConfig {
            goal: "Add rate limiting to the public API".into(),
            roles: vec![
                codeoid_protocol::CollaborationRole {
                    name: "orchestrator".into(),
                    provider_id: "claude".into(),
                    model: None,
                    count: None,
                    purpose: None,
                    write: None,
                    reads: None,
                    writes: None,
                },
                codeoid_protocol::CollaborationRole {
                    name: "review".into(),
                    provider_id: "gemini".into(),
                    model: Some("gemini-2.5-pro".into()),
                    count: Some(3),
                    purpose: Some("independent critique".into()),
                    write: Some(false),
                    reads: Some(vec!["spec".into(), "diff".into()]),
                    writes: Some(vec!["findings".into()]),
                },
            ],
        }),
        // A real session is either a parent or a child, never both; populated
        // here anyway so the camelCase walker visits this struct's keys too.
        collaboration_role: Some(codeoid_protocol::CollaborationRoleRef {
            parent_session_id: "parent-1".into(),
            role_name: "review".into(),
            ordinal: 2,
            write: false,
        }),
    }
}

fn sample_session_message() -> SessionMessage {
    SessionMessage {
        session_id: "s".into(),
        message_id: "m".into(),
        role: MessageRole::ToolCall,
        content: String::new(),
        parts: Some(vec![
            ContentPart::Text {
                text: "hi".into(),
                markdown: Some(false),
            },
            ContentPart::Code {
                code: "fn x() {}".into(),
                language: Some("rust".into()),
                file_path: Some("lib.rs".into()),
            },
            ContentPart::Diff {
                path: "x".into(),
                added: 1,
                removed: 0,
                original_path: Some("old_x".into()),
            },
            ContentPart::Progress {
                message: "running".into(),
                percent: Some(42),
                elapsed_ms: Some(3000),
            },
        ]),
        identity: sample_identity(),
        tool: Some(ToolInfo {
            tool_id: "t".into(),
            name: "Bash".into(),
            state: ToolState::WaitingConfirmation {
                input: serde_json::json!({"command": "ls"}),
                description: "run ls".into(),
                approval_id: "a1".into(),
            },
        }),
        metadata: None,
        timestamp: "2026-04-22T00:00:00Z".into(),
    }
}

// -----------------------------------------------------------------------------
// Actual tests — one per top-level protocol type. If a new variant lands in
// `ClientMessage` or `DaemonMessage`, add it here.
// -----------------------------------------------------------------------------

#[test]
fn client_messages_are_camel_case_on_wire() {
    let samples: Vec<(&str, ClientMessage)> = vec![
        (
            "SessionCreate",
            ClientMessage::SessionCreate {
                id: "1".into(),
                name: "n".into(),
                workdir: "/".into(),
                provider_id: Some("pi".into()),
                collaboration: None,
            },
        ),
        ("SessionList", ClientMessage::SessionList { id: "1".into() }),
        (
            "SessionAttach",
            ClientMessage::SessionAttach {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionDetach",
            ClientMessage::SessionDetach {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionSend",
            ClientMessage::SessionSend {
                id: "1".into(),
                session_id: "s".into(),
                text: "hi".into(),
                attachments: Some(vec![Attachment {
                    path: "README.md".into(),
                    content: Some("x".into()),
                    mime_type: Some("text/plain".into()),
                    data: None,
                }]),
                priority: Some(SendPriority::Now),
            },
        ),
        (
            "SessionInterrupt",
            ClientMessage::SessionInterrupt {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionApprove",
            ClientMessage::SessionApprove {
                id: "1".into(),
                session_id: "s".into(),
                approval_id: "a".into(),
                approved: true,
                updated_input: None,
            },
        ),
        (
            "SessionUiResponse",
            ClientMessage::SessionUiResponse {
                id: "1".into(),
                session_id: "s".into(),
                request_id: "u".into(),
                value: Some("Allow".into()),
                confirmed: Some(true),
                cancelled: None,
            },
        ),
        (
            "SessionPartAction",
            ClientMessage::SessionPartAction {
                id: "1".into(),
                session_id: "s".into(),
                message_id: "m".into(),
                action: "deploy".into(),
                data: Some(serde_json::json!({"envName": "dev"})),
            },
        ),
        (
            "SessionCommands",
            ClientMessage::SessionCommands {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionDestroy",
            ClientMessage::SessionDestroy {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionSetMode",
            ClientMessage::SessionSetMode {
                id: "1".into(),
                session_id: "s".into(),
                mode: SessionMode::Autonomous,
                max_turns: Some(50),
            },
        ),
        (
            "SessionPin",
            ClientMessage::SessionPin {
                id: "1".into(),
                session_id: "s".into(),
                path: "README.md".into(),
            },
        ),
        (
            "SessionUnpin",
            ClientMessage::SessionUnpin {
                id: "1".into(),
                session_id: "s".into(),
                path: "README.md".into(),
            },
        ),
        (
            "SessionRotate",
            ClientMessage::SessionRotate {
                id: "1".into(),
                session_id: "s".into(),
            },
        ),
        (
            "SessionSearch",
            ClientMessage::SessionSearch {
                id: "1".into(),
                query: "bug".into(),
                scope: Some(SearchScope::Workspace),
                workdir: Some("/".into()),
                limit: Some(5),
            },
        ),
        (
            "SessionSetProvider",
            ClientMessage::SessionSetProvider {
                id: "1".into(),
                session_id: "s".into(),
                provider_id: "pi".into(),
            },
        ),
        (
            "SessionSetModel",
            ClientMessage::SessionSetModel {
                id: "1".into(),
                session_id: "s".into(),
                model: "opus".into(),
                fallback_model: Some(Some("sonnet".into())),
            },
        ),
        (
            "SessionRename",
            ClientMessage::SessionRename {
                id: "1".into(),
                session_id: "s".into(),
                name: "renamed".into(),
            },
        ),
    ];

    for (label, msg) in samples {
        assert_no_snake_case_keys(msg, label);
    }
}

#[test]
fn daemon_messages_are_camel_case_on_wire() {
    use codeoid_protocol::AuthOkMsg;
    let samples: Vec<(&str, DaemonMessage)> = vec![
        (
            "AuthOk",
            DaemonMessage::AuthOk(AuthOkMsg {
                identity: sample_identity(),
                scopes: vec!["session:list".into()],
                protocol_version: Some(1),
                capabilities: Some(vec!["commands.dynamic".into(), "ui.dialogs".into()]),
                providers: None,
            }),
        ),
        (
            "SessionUiRequest",
            DaemonMessage::SessionUiRequest(codeoid_protocol::SessionUiRequestMsg {
                session_id: "s".into(),
                request_id: "u".into(),
                method: codeoid_protocol::UiRequestMethod::Select,
                title: "Pick one".into(),
                message: Some("The extension wants an answer.".into()),
                options: Some(vec!["a".into(), "b".into()]),
                placeholder: Some("type here".into()),
                prefill: Some("draft".into()),
                timeout_ms: Some(30_000),
                timestamp: "2026-04-22T00:00:00Z".into(),
            }),
        ),
        (
            "SessionUiResolved",
            DaemonMessage::SessionUiResolved {
                session_id: "s".into(),
                request_id: "u".into(),
                reason: codeoid_protocol::UiResolvedReason::Answered,
                timestamp: "2026-04-22T00:00:00Z".into(),
            },
        ),
        (
            "SessionCommandsResult",
            DaemonMessage::SessionCommandsResult {
                request_id: "r".into(),
                session_id: "s".into(),
                provider_id: "pi".into(),
                commands: vec![codeoid_protocol::ProviderCommand {
                    name: "review".into(),
                    description: Some("Review the diff".into()),
                    source: Some("extension".into()),
                    argument_hint: Some("<pattern>".into()),
                }],
            },
        ),
        (
            "ResponseOk",
            DaemonMessage::ResponseOk {
                request_id: "r".into(),
                data: Some(serde_json::json!({"okKey": true})),
            },
        ),
        (
            "ResponseError",
            DaemonMessage::ResponseError {
                request_id: "r".into(),
                error: "nope".into(),
                code: ErrorCode::Forbidden,
            },
        ),
        (
            "SessionListResult",
            DaemonMessage::SessionListResult {
                request_id: "r".into(),
                sessions: vec![sample_session_info()],
            },
        ),
        (
            "SessionStatusChange",
            DaemonMessage::SessionStatusChange {
                session_id: "s".into(),
                status: SessionStatus::WaitingApproval,
                timestamp: "t".into(),
            },
        ),
        (
            "SessionInfoUpdate",
            DaemonMessage::SessionInfoUpdate {
                session: sample_session_info(),
                timestamp: "t".into(),
            },
        ),
        (
            "ScrollbackReplay",
            DaemonMessage::ScrollbackReplay {
                session_id: "s".into(),
                messages: vec![sample_session_message()],
                tail: Some(true),
                has_more: Some(false),
            },
        ),
        (
            "ScrollbackPageResult",
            DaemonMessage::ScrollbackPageResult {
                request_id: "r".into(),
                session_id: "s".into(),
                messages: vec![sample_session_message()],
                has_more: true,
                source: "buffer".into(),
            },
        ),
        (
            "SessionMessage",
            DaemonMessage::SessionMessage(sample_session_message()),
        ),
        (
            "SessionMessageDelta",
            DaemonMessage::SessionMessageDelta(SessionMessageDelta {
                session_id: "s".into(),
                message_id: "m".into(),
                content_append: Some("x".into()),
                parts_append: None,
                parts_update: None,
                tool_state_update: Some(ToolState::Completed {
                    success: true,
                    output: Some("done".into()),
                    elapsed_ms: Some(200),
                    confirmed_by: Some(ConfirmedBy::User),
                }),
                timestamp: "t".into(),
            }),
        ),
    ];

    for (label, msg) in samples {
        assert_no_snake_case_keys(msg, label);
    }
}

#[test]
fn tool_state_cancelled_has_camel_case_fields() {
    let state = ToolState::Cancelled {
        reason: CancelReason::Denied,
        message: Some("no".into()),
    };
    assert_no_snake_case_keys(state, "ToolState::Cancelled");
}

#[test]
fn waiting_confirmation_roundtrips_approval_id() {
    // Regression test for the bug where `approval_id` serialized as
    // snake_case and incoming `approvalId` from the daemon failed to
    // deserialize — swallowing every approval gate.
    let raw = r#"{
        "phase": "waiting_confirmation",
        "input": {"command": "ls"},
        "description": "list files",
        "approvalId": "a-42"
    }"#;
    let state: ToolState = serde_json::from_str(raw).expect("parses");
    match state {
        ToolState::WaitingConfirmation { approval_id, .. } => {
            assert_eq!(approval_id, "a-42");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn tool_executing_roundtrips_elapsed_ms() {
    let raw = r#"{ "phase": "executing", "elapsedMs": 1234 }"#;
    let state: ToolState = serde_json::from_str(raw).unwrap();
    match state {
        ToolState::Executing { elapsed_ms, .. } => {
            assert_eq!(elapsed_ms, Some(1234));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn tool_completed_roundtrips_confirmed_by() {
    let raw = r#"{
        "phase": "completed",
        "success": true,
        "elapsedMs": 500,
        "confirmedBy": "auto"
    }"#;
    let state: ToolState = serde_json::from_str(raw).unwrap();
    match state {
        ToolState::Completed {
            elapsed_ms,
            confirmed_by,
            ..
        } => {
            assert_eq!(elapsed_ms, Some(500));
            assert_eq!(confirmed_by, Some(ConfirmedBy::Auto));
        }
        _ => panic!("wrong variant"),
    }
}

// ── Fleet board: cross-language fixtures ────────────────────────────────────
//
// The JSON below was CAPTURED from the real TS daemon (SessionManager handling
// `fleet.subscribe`, then broadcasting a delta), not hand-written. That is the
// whole point: this crate is a hand-maintained mirror of a TS contract, and the
// failure mode is silent field drop on one side. If the daemon's projection
// changes shape, these stop deserializing.

#[test]
fn fleet_snapshot_result_parses_real_daemon_json() {
    let raw = r#"{
      "type": "fleet.snapshot.result",
      "requestId": "1",
      "fleet": {
        "workers": [],
        "tasks": [{
          "id": "1aa8e2c8-6e3c-4b85-9ffc-924ae9eda6f4",
          "kind": "spawn",
          "shape": "scout",
          "status": "queued",
          "attempts": 0,
          "createdAt": 1786667059557,
          "createdBy": "wimse://conductor/acc"
        }],
        "events": [],
        "agg": {
          "activeTasks": 1, "blockedTasks": 0,
          "inputTokens": 0, "outputTokens": 0, "totalCostUsd": 0
        }
      }
    }"#;

    let msg: DaemonMessage = serde_json::from_str(raw).expect("daemon JSON must deserialize");
    let DaemonMessage::FleetSnapshotResult { request_id, fleet } = msg else {
        panic!("expected FleetSnapshotResult, got {msg:?}");
    };
    assert_eq!(request_id, "1");
    assert!(
        fleet.conductor.is_none(),
        "absent conductor is a valid board"
    );
    assert_eq!(fleet.tasks.len(), 1);
    assert_eq!(fleet.tasks[0].kind, FleetTaskKind::Spawn);
    assert_eq!(fleet.tasks[0].shape, FleetTaskShape::Scout);
    assert_eq!(fleet.tasks[0].status, FleetTaskStatus::Queued);
    assert_eq!(fleet.agg.active_tasks, 1);
    // The daemon must never put the dispatch prompt on the wire, so there is
    // no field here to hold one.
    assert!(!raw.contains("\"prompt\""));
}

#[test]
fn fleet_update_delta_parses_real_daemon_json() {
    let raw = r#"{
      "type": "fleet.update",
      "delta": {
        "kind": "task",
        "task": {
          "id": "4c2e690f-a7ce-40e2-baee-ad7d4998f892",
          "kind": "send",
          "shape": "ship",
          "status": "queued",
          "attempts": 0,
          "createdAt": 1786667059558,
          "targetSession": "sess-1",
          "createdBy": "wimse://conductor/acc"
        },
        "agg": {
          "activeTasks": 2, "blockedTasks": 0,
          "inputTokens": 0, "outputTokens": 0, "totalCostUsd": 0
        }
      }
    }"#;

    let msg: DaemonMessage = serde_json::from_str(raw).expect("daemon JSON must deserialize");
    let DaemonMessage::FleetUpdate { delta } = msg else {
        panic!("expected FleetUpdate, got {msg:?}");
    };
    let FleetDelta::Task { task, agg } = delta else {
        panic!("expected a task delta");
    };
    assert_eq!(task.kind, FleetTaskKind::Send);
    assert_eq!(task.target_session.as_deref(), Some("sess-1"));
    assert_eq!(agg.active_tasks, 2);
}

#[test]
fn unknown_enum_values_degrade_instead_of_failing_the_board() {
    // A client WILL meet a newer daemon. A role/status/shape it has never heard
    // of must land as Unknown, not poison the whole SessionInfo or task row —
    // otherwise one new enum value blanks the entire fleet view.
    let task: FleetTask = serde_json::from_str(
        r#"{"id":"t","kind":"teleport","shape":"warp","status":"vibing",
            "attempts":0,"createdAt":1,"createdBy":"c"}"#,
    )
    .expect("unknown enum values must still deserialize");
    assert_eq!(task.kind, FleetTaskKind::Unknown);
    assert_eq!(task.shape, FleetTaskShape::Unknown);
    assert_eq!(task.status, FleetTaskStatus::Unknown);

    let role: SessionRole = serde_json::from_str("\"sub-conductor\"").expect("unknown role");
    assert_eq!(role, SessionRole::Unknown);
}

#[test]
fn fleet_subscribe_serializes_the_way_the_daemon_schema_demands() {
    // The daemon's zod schema pins `scope` to the literal "tenant" and rejects
    // anything else, so this must not serialize as, say, "Tenant".
    let json = serde_json::to_value(ClientMessage::FleetSubscribe {
        id: "r1".into(),
        scope: FleetScope::Tenant,
    })
    .unwrap();
    assert_eq!(json["type"], "fleet.subscribe");
    assert_eq!(json["scope"], "tenant");
    assert_eq!(json["id"], "r1");
}
