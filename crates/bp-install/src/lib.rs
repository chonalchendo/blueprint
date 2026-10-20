//! Installer: harness dialects, the embedded bundle, version comparison, precondition tests.

use bp_vault::Kind;

/// A node kind's body template, from `bundle/templates/`; `None` for a record kind, which has none. The binary
/// embeds them, so `create --template` needs no install, and `install` does not copy them (27, design change 7).
pub fn template(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Project => include_str!("../../../bundle/templates/project.md"),
        Kind::Design => include_str!("../../../bundle/templates/design.md"),
        Kind::Milestone => include_str!("../../../bundle/templates/milestone.md"),
        Kind::Issue => include_str!("../../../bundle/templates/issue.md"),
        Kind::Thread => include_str!("../../../bundle/templates/thread.md"),
        Kind::Decision => include_str!("../../../bundle/templates/decision.md"),
        Kind::Convention => include_str!("../../../bundle/templates/convention.md"),
        Kind::Learning => include_str!("../../../bundle/templates/learning.md"),
        Kind::Inbox => include_str!("../../../bundle/templates/inbox.md"),
        _ => return None,
    })
}
