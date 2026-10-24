//! The envelope for every non-zero exit (15): a closed code, the subject, the rule and a runnable fix.

use serde::Serialize;

crate::closed_enum! {
    /// The closed refusal codes of `analysis/contract.json`. A new meaning gets a new code; a code never changes.
    pub enum Code {
        SchemaInvalid = "schema_invalid",
        UnresolvedLink = "unresolved_link",
        UnknownValue = "unknown_value",
        IllegalTransition = "illegal_transition",
        DesignNotDelta = "design_not_delta",
        DeltaPending = "delta_pending",
        InboundLinks = "inbound_links",
        CheckUnnamed = "check_unnamed",
        CheckEmpty = "check_empty",
        VaultLocked = "vault_locked",
        AlreadyClaimed = "already_claimed",
        DependencyUnpushed = "dependency_unpushed",
        DependencySplit = "dependency_split",
        StackTooDeep = "stack_too_deep",
        ReviewMissing = "review_missing",
        ChecksPending = "checks_pending",
        DiffChanged = "diff_changed",
        BaseUnmerged = "base_unmerged",
        BaseMismatch = "base_mismatch",
        BundleMismatch = "bundle_mismatch",
        BundleInvalid = "bundle_invalid",
        ForgePrecondition = "forge_precondition",
        SchemaNewer = "schema_newer",
        NoVault = "no_vault",
        NoClone = "no_clone",
        NoNode = "no_node",
        NoSection = "no_section",
        ContextIncomplete = "context_incomplete",
        CouldNotRun = "could_not_run",
        Usage = "usage",
        NotALabel = "not_a_label",
        PathDirty = "path_dirty",
        IdTaken = "id_taken",
        CheckDuplicate = "check_duplicate",
        CheckFailed = "check_failed",
        IssuesOpen = "issues_open",
        EvidenceStale = "evidence_stale",
        ReviewerInvalid = "reviewer_invalid",
        ChecksChanged = "checks_changed",
        RoleForbidden = "role_forbidden",
        DiagramInvalid = "diagram_invalid",
        BaseMoved = "base_moved",
        NotEditorial = "not_editorial",
        VerbOutsideRole = "verb_outside_role",
        FindingInvalid = "finding_invalid",
    }
}

/// One envelope: exit 2 for a refusal, 1 when the tool could not run, 4 for a usage error.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {rule}")]
pub struct Refusal {
    /// The closed code.
    pub code: Code,
    /// The id or path the rule refused.
    pub subject: String,
    /// The rule, in words, as the design states it.
    pub rule: &'static str,
    /// The command or edit that resolves it.
    pub fix: String,
    /// The process exit code.
    pub exit: u8,
}

impl Refusal {
    /// A rule refused: exit 2.
    pub fn new(code: Code, subject: impl Into<String>, rule: &'static str, fix: impl Into<String>) -> Self {
        Self { code, subject: subject.into(), rule, fix: fix.into(), exit: 2 }
    }

    /// The tool could not run, and no rule refused: exit 1.
    pub fn failed(subject: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: Code::CouldNotRun,
            subject: subject.into(),
            rule: "the tool could not run",
            fix: fix.into(),
            exit: 1,
        }
    }

    /// The command line does not parse: exit 4. The subject names the bad argument; the fix is the verb's help.
    pub fn usage(subject: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: Code::Usage,
            subject: subject.into(),
            rule: "the verb, its arguments and its flags are as `bp <verb> --help` lists them",
            fix: fix.into(),
            exit: 4,
        }
    }

    /// A file does not parse. No verb rewrites a file it cannot parse, so the one fix, whatever the verb, is a human's
    /// edit (25, issue 8).
    pub fn schema(id: &str, broken: &crate::Broken) -> Self {
        let what = if broken.rule { "breaks its kind's schema" } else { "does not parse" };
        Self::new(
            Code::SchemaInvalid,
            id,
            "a file holds its kind's schema (12, the kind schemas)",
            format!("the file {what} ({}); edit {id}.md by hand, then git commit it", broken.why),
        )
    }
}
