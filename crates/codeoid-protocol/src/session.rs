//! Session metadata — the `SessionInfo` struct and its satellites.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Working,
    WaitingApproval,
    Error,
}

/// Execution mode — controls tool approval and autonomous budgeting.
///
/// * `Guarded` (daemon default) — Read/Grep/Glob/memory are auto-approved;
///   Write/Edit/Bash/Agent still ask. (≈ Claude Code's default mode.)
/// * `Interactive` — every tool call asks for approval, including reads.
/// * `Autonomous` — every tool auto-approved until the turn budget is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    Interactive,
    /// Serializes as `"guarded"`. `auto-allow` accepted as a backward-compat
    /// alias on the wire (the mode was renamed from `auto-allow`).
    #[serde(alias = "auto-allow")]
    Guarded,
    Autonomous,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::Interactive
    }
}

/// Top-level session metadata — broadcast on attach, list, and every
/// `session.info_update`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub workdir: String,
    pub status: SessionStatus,
    pub created_by: String,
    pub created_at: String,
    pub attached_clients: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SessionMode>,

    /// Remaining turns budget for autonomous mode. `None` = unbounded, `Some(0)` = exhausted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns_remaining: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_files: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_uri: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagents: Option<Vec<Subagent>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SessionUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationInfo>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_messages: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,

    /// Id of the provider backing this session (e.g. "claude", "pi").
    /// Absent on daemons that predate multi-provider sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Lineage for a session created via `session.fork`. Absent = not a fork.
    /// Rendered as a "⑃ from <name>·t<atTurn>" tag in the session title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<ForkedFrom>,

    /// Git worktree backing this session's workdir (fork isolation / bind).
    /// Absent = shares its workdir with no git isolation. Rendered as a
    /// "⎇ <branch>" tag in the session title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<SessionWorktree>,

    /// Collaboration this session orchestrates — goal + role→backend
    /// bindings — when it was created with the Collaborative toggle.
    /// Absent = a normal session. Persisted daemon-side, so it survives a
    /// restart the way `role`/`provider_id` already do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collaboration: Option<CollaborationConfig>,

    /// Set when this session is a role-CHILD of a collaborative session.
    /// Absent = not a collaboration child. Pairs with `collaboration` above,
    /// which marks the orchestrating parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collaboration_role: Option<CollaborationRoleRef>,
}

/// Where a forked session came from — the parent id, the parent's name at
/// fork time (a snapshot; the parent may be renamed or gone), and the branch
/// point (conversation rounds carried over).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkedFrom {
    pub session_id: String,
    pub name: String,
    pub at_turn: u32,
}

/// A git worktree backing a session's workdir. `created_by_codeoid` marks a
/// worktree codeoid created for fork isolation (removed on destroy) versus one
/// the user bound (never touched).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorktree {
    pub path: String,
    pub branch: String,
    pub created_by_codeoid: bool,
}

/// One role in a collaborative session — a `{backend, model}` binding chosen
/// per purpose.
///
/// `name` is a free-form string, not an enum: the role taxonomy is data, so
/// the daemon can add "security-reviewer" as a config change and this crate
/// keeps parsing it without a release.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationRole {
    pub name: String,
    /// Backend this role's children run on. The daemon fail-closes on an id
    /// it does not have registered.
    pub provider_id: String,
    /// Model within that backend. `None` = the backend's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How many children to fan out for this role. `None` = 1; >1 is what
    /// makes a review panel a panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// What this role is for; surfaced in the child's brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// Whether this role's children may modify the workspace.
    ///
    /// `None`/`false` = read-only, and that default is load-bearing: the
    /// daemon gives a read-only role a leaf identity with no write scope at
    /// all, so a reviewer provably cannot write rather than being asked not
    /// to. Write authority is opt-in per role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<bool>,
    /// Goal-blackboard artifact kinds this role may READ — `spec`, `research`,
    /// `adr`, `task-list`, `diff`, `findings`, or `extra/<key>`.
    ///
    /// `None` = the daemon's default profile for this role name; a role with no
    /// profile and no declaration reads nothing. This is what makes reviewer
    /// independence structural: `review` reads `diff`+`spec` and NOT
    /// `research` or its peers' `findings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reads: Option<Vec<String>>,
    /// Artifact kinds this role may WRITE. `None` = the default profile for
    /// this role name. A role writing a multi-writer kind (`findings`) writes
    /// into its own slot, chosen daemon-side, so one reviewer can never
    /// overwrite another's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writes: Option<Vec<String>>,
}

/// Set on a role-CHILD of a collaborative session: which collaboration it
/// belongs to and which role it plays.
///
/// The mirror of [`SessionInfo::collaboration`] (set on the orchestrating
/// parent), so a client can group a fleet without inferring it from names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationRoleRef {
    /// Session id of the orchestrating parent.
    pub parent_session_id: String,
    /// Role name from the parent's config (already lowercased by the daemon).
    pub role_name: String,
    /// 1-based index within this role's fan-out (`review` ×3 → 1, 2, 3).
    pub ordinal: u32,
    /// Whether this child's identity carries write authority.
    pub write: bool,
}

/// Collaborative-session config: one goal worked by several role-children on
/// possibly different backends. Sent on `session.create` and echoed back on
/// [`SessionInfo`].
///
/// Exactly one role must be named `orchestrator`, and in v1 it must sit on
/// the claude backend. The daemon enforces both and answers with a specific
/// error, so this crate carries only the shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationConfig {
    pub goal: String,
    pub roles: Vec<CollaborationRole>,
}

/// Rotation telemetry — how many times the backing Claude Code session has
/// been rolled over to avoid context compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationInfo {
    pub count: u32,
    /// Unix ms of last rotation, or null if never rotated.
    pub last_rotated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_code_session_id: Option<String>,
}

/// Cumulative usage totals for a session. Aggregated from each SDK `result` event.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub num_turns: u32,
    pub duration_ms: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_turns: Option<Vec<TurnUsage>>,

    /// Max PRIMARY-AGENT context size ever seen on a single turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_input_tokens: Option<u64>,

    /// Most recent turn's PRIMARY-AGENT context size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_input_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_output_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_cost_usd: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_cache_hit_rate: Option<f64>,
}

/// Per-turn usage record — one entry per SDK `result` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub turn_number: u32,
    pub created_at: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub duration_ms: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    pub total_input_tokens: u64,
    pub billable_input_tokens: u64,
    pub cache_hit_rate: f64,
}

/// Active sub-agent for the identity chain display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wimse_uri: Option<String>,
    pub agent_type: String,
    pub spawned_at: i64,
    pub active: bool,
}
