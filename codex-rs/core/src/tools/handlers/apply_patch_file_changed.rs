use crate::hook_runtime::run_file_changed_hooks;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::protocol::FileChange;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) async fn run_apply_patch_file_changed_hooks(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    call_id: String,
    changes: HashMap<PathBuf, FileChange>,
) {
    run_file_changed_hooks(session, turn, call_id, "apply_patch".to_string(), changes).await;
}
