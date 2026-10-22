//! The tool's half of the role check (21, rule 6; 27, design change 3): a verb outside the row of `BP_ROLE` is refused
//! before it runs. The role hook denies the same verbs earlier, but only on Claude Code and only for a call it parses.

use bp_operation::READ_VERBS;
use bp_vault::{Code, Refusal};

use crate::Verb;

/// `Ok` for a read verb, the human, a verb in the role's row, or one of the human's acts: `stamp`, `land` and a
/// `transition` to `approved` or `active` keep `role_forbidden`, which a code never stops meaning (15). Their own check
/// names the act, and no row holds them. `BP_ROLE` is a label an agent can forge, so this stops a slip, not an attack.
pub(crate) fn check(verb: &Verb) -> Result<(), Refusal> {
    let name = match verb {
        Verb::Where => "where",
        Verb::Show(_) => "show",
        Verb::List(_) => "list",
        Verb::Search(_) => "search",
        Verb::Walk(_) => "walk",
        Verb::Set(_) => "set",
        Verb::Create(_) => "create",
        Verb::Link(_) => "link",
        // The human's acts keep `role_forbidden`: `set::human` names the act before any row is read.
        Verb::Transition(t) if matches!(t.status.as_str(), "approved" | "active") => return Ok(()),
        Verb::Transition(_) => "transition",
        Verb::Record(_) => "record",
        Verb::Rename(_) => "rename",
        Verb::Cut(_) => "cut",
        Verb::Release(_) => "release",
        Verb::Pr(_) => "pr",
        Verb::Check(_) => "check",
        Verb::Install(_) => "install",
        Verb::Diagram(_) => "diagram",
        Verb::Land(_) | Verb::Stamp(_) => return Ok(()),
    };
    if READ_VERBS.contains(&name) {
        return Ok(());
    }
    let role = crate::set::role()?;
    let Some(row) = role.verbs() else { return Ok(()) };
    if row.contains(&name) {
        return Ok(());
    }
    let allowed = if row.is_empty() { "reads only".to_string() } else { row.join(", ") };
    Err(Refusal::new(
        Code::VerbOutsideRole,
        format!("{name} under BP_ROLE={role}"),
        "BP_ROLE names an agent role, and the verb is outside that role's row of 21; the read verbs are open to every role",
        format!(
            "`bp {name}` is outside the {role} role's verbs ({allowed}); ask the human to run it from a shell with BP_ROLE unset"
        ),
    ))
}
