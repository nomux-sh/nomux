//! The plan of a clean: the items, with their bytes, refusals and warnings.
//! A plan changes nothing. A dry run shows it, and a clean runs the items
//! of the same plan, thus the two have one source.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nomux_proto::wire::v1::{
    Error, WorktreeCleanAction, WorktreeInfo, WorktreeMergedBy, WorktreePlanItem,
    WorktreePlanRequest, WorktreePlanResponse, WorktreePlanStep, WorktreeRefusal, WorktreeState,
};

use super::{Described, Detail, Sizes, Sources, Worktrees, lock, not_found};
use crate::clock::now;

/// A plan stays this long.
const PLAN_LIFETIME: Duration = Duration::from_mins(15);
/// The daemon keeps this many plans at most: the oldest goes first.
const MAX_PLANS: usize = 16;
/// The name of the trash place in the parent directory of a worktree.
pub(super) const TRASH_PLACE: &str = ".nomux-trash";

/// What a plan selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    BuildOutput,
    /// The build output of each finished worktree: a rule of the settings.
    BuildOutputOfFinished,
    Finished,
    Stale,
    Prune,
}

/// The request of a plan.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the options of `WorktreePlanRequest`, each one independent"
)]
pub(super) struct Selection {
    pub(super) project_id: String,
    pub(super) action: Action,
    pub(super) paths: Vec<PathBuf>,
    pub(super) delete_branch: bool,
    pub(super) allow_unpushed: bool,
    pub(super) include_ignored: bool,
    pub(super) now: bool,
    /// A rule of the settings: a dormant session refuses, and no option
    /// removes a refusal.
    pub(super) automatic: bool,
}

impl Selection {
    pub(super) fn of_request(request: &WorktreePlanRequest) -> Self {
        let action = match request.action() {
            WorktreeCleanAction::BuildOutput => Action::BuildOutput,
            WorktreeCleanAction::Stale => Action::Stale,
            WorktreeCleanAction::Prune => Action::Prune,
            WorktreeCleanAction::Finished | WorktreeCleanAction::Unspecified => Action::Finished,
        };
        Self {
            project_id: request.project_id.clone(),
            action,
            paths: request.worktree_paths.iter().map(PathBuf::from).collect(),
            delete_branch: request.delete_branch,
            allow_unpushed: request.allow_unpushed,
            include_ignored: request.include_ignored,
            now: request.now,
            automatic: false,
        }
    }

    /// A rule of the settings: none of the options.
    pub(super) fn automatic(action: Action) -> Self {
        Self {
            project_id: String::new(),
            action,
            paths: Vec::new(),
            delete_branch: false,
            allow_unpushed: false,
            include_ignored: false,
            now: false,
            automatic: true,
        }
    }
}

/// A plan that a clean can name.
#[derive(Debug, Clone)]
pub(super) struct StoredPlan {
    pub(super) selection: Selection,
    pub(super) items: Vec<WorktreePlanItem>,
    created_at_ms: u64,
}

#[derive(Default)]
pub(super) struct StoredPlans {
    plans: std::sync::Mutex<Vec<(String, StoredPlan)>>,
}

impl StoredPlans {
    /// Keeps a plan and returns its identifier.
    pub(super) fn keep(&self, selection: Selection, items: Vec<WorktreePlanItem>) -> String {
        let id = random_id();
        let mut plans = lock(&self.plans);
        let oldest_kept = now().0.saturating_sub(lifetime_ms());
        plans.retain(|(_, plan)| plan.created_at_ms >= oldest_kept);
        if plans.len() == MAX_PLANS {
            plans.remove(0);
        }
        plans.push((
            id.clone(),
            StoredPlan {
                selection,
                items,
                created_at_ms: now().0,
            },
        ));
        id
    }

    /// The plan `id`, if it is not older than its lifetime.
    pub(super) fn get(&self, id: &str) -> Option<StoredPlan> {
        let oldest_kept = now().0.saturating_sub(lifetime_ms());
        lock(&self.plans)
            .iter()
            .find(|(plan_id, plan)| plan_id == id && plan.created_at_ms >= oldest_kept)
            .map(|(_, plan)| plan.clone())
    }
}

fn lifetime_ms() -> u64 {
    u64::try_from(PLAN_LIFETIME.as_millis()).unwrap_or(u64::MAX)
}

/// Sixteen hexadecimal digits from the random source of the system.
pub(super) fn random_id() -> String {
    let mut bytes = [0_u8; 8];
    // Without random bytes the time keeps the identifiers apart.
    if getrandom::fill(&mut bytes).is_err() {
        bytes = now().0.to_be_bytes();
    }
    format!("{:016x}", u64::from_be_bytes(bytes))
}

impl Worktrees {
    /// `Worktree.Plan`.
    pub(crate) async fn plan(
        &self,
        request: &WorktreePlanRequest,
        sources: Sources<'_>,
    ) -> Result<WorktreePlanResponse, Error> {
        let selection = Selection::of_request(request);
        let projects = self
            .snapshot(&selection.project_id, &sources, Sizes::Wait)
            .await?;
        for path in &selection.paths {
            let path = path.display().to_string();
            if super::find_worktree(&projects, &path).is_none() {
                return Err(not_found(format!("{path} is no worktree of the Projects")));
            }
        }
        let items = items(&projects, &selection);
        let total_bytes = items
            .iter()
            .filter(|item| item.refusals.is_empty())
            .map(|item| item.bytes)
            .sum();
        let plan_id = self.plans.keep(selection, items.clone());
        Ok(WorktreePlanResponse {
            plan_id,
            items,
            total_bytes,
            grace_ms: u64::try_from(self.settings.grace.as_millis()).unwrap_or(u64::MAX),
        })
    }
}

/// The items of a plan of `projects`.
pub(super) fn items(projects: &[Described], selection: &Selection) -> Vec<WorktreePlanItem> {
    let named: HashSet<&Path> = selection.paths.iter().map(PathBuf::as_path).collect();
    let mut items = Vec::new();
    for project in projects {
        let main = project
            .wire
            .worktrees
            .iter()
            .find(|worktree| worktree.is_main && !worktree.is_bare)
            .map(|worktree| PathBuf::from(&worktree.path));
        let mut missing = Vec::new();
        for (worktree, detail) in project.wire.worktrees.iter().zip(&project.details) {
            let path = Path::new(&worktree.path);
            let is_named = named.contains(path);
            if worktree.is_bare || !worktree.trash_id.is_empty() || (!named.is_empty() && !is_named)
            {
                continue;
            }
            let item = Item {
                project_id: &project.wire.project_id,
                worktree,
                detail,
                selection,
                is_named,
            };
            if worktree
                .flags()
                .any(|flag| flag == nomux_proto::wire::v1::WorktreeFlag::Missing)
            {
                missing.push(worktree);
                continue;
            }
            items.extend(item.steps());
        }
        if matches!(selection.action, Action::Finished | Action::Prune) {
            items.extend(prune_steps(
                &project.wire.project_id,
                main.as_deref(),
                &missing,
            ));
        }
    }
    items
}

/// One worktree of a plan.
struct Item<'a> {
    project_id: &'a str,
    worktree: &'a WorktreeInfo,
    detail: &'a Detail,
    selection: &'a Selection,
    is_named: bool,
}

impl Item<'_> {
    fn steps(&self) -> Vec<WorktreePlanItem> {
        let linked_or_named = !self.worktree.is_main || self.is_named;
        let state = self.state();
        match self.selection.action {
            Action::BuildOutput if linked_or_named => {
                self.build_output_steps(&self.build_output_refusals())
            }
            Action::Stale if linked_or_named && state == WorktreeState::Stale => {
                self.build_output_steps(&self.build_output_refusals())
            }
            Action::BuildOutputOfFinished
                if !self.worktree.is_main && state == WorktreeState::Finished =>
            {
                self.build_output_steps(&self.build_output_refusals())
            }
            Action::Finished
                if (!self.worktree.is_main && state == WorktreeState::Finished)
                    || self.is_named =>
            {
                let refusals = self.removal_refusals();
                let mut steps = self.build_output_steps(&refusals);
                steps.push(self.removal_step(refusals));
                steps
            }
            _ => Vec::new(),
        }
    }

    /// The state for the plan: a dormant session makes a worktree active
    /// only for a rule of the settings.
    fn state(&self) -> WorktreeState {
        if self.only_dormant_sessions() && !self.selection.automatic {
            self.detail.state_without_sessions
        } else {
            self.worktree.state()
        }
    }

    fn only_dormant_sessions(&self) -> bool {
        self.detail.dormant_sessions > 0 && self.detail.live_sessions == 0
    }

    /// The refusals of the worktree that the plan keeps.
    fn refusals(&self, keep: impl Fn(WorktreeRefusal) -> bool) -> Vec<WorktreeRefusal> {
        let dormant_only = self.only_dormant_sessions() && !self.selection.automatic;
        let finished_without_sessions =
            self.detail.state_without_sessions == WorktreeState::Finished;
        self.worktree
            .refusals()
            .filter(|refusal| match refusal {
                WorktreeRefusal::ActiveSession => !dormant_only,
                WorktreeRefusal::NotFinished => !(dormant_only && finished_without_sessions),
                _ => true,
            })
            .filter(|refusal| keep(*refusal))
            .collect()
    }

    fn build_output_refusals(&self) -> Vec<WorktreeRefusal> {
        self.refusals(|refusal| {
            matches!(
                refusal,
                WorktreeRefusal::FactsUnknown
                    | WorktreeRefusal::ActiveSession
                    | WorktreeRefusal::ProcessRunning
                    | WorktreeRefusal::Kept
            )
        })
    }

    fn removal_refusals(&self) -> Vec<WorktreeRefusal> {
        let selection = self.selection;
        let options = !selection.automatic;
        self.refusals(|refusal| match refusal {
            WorktreeRefusal::UnpushedCommits => !(options && selection.allow_unpushed),
            WorktreeRefusal::IgnoredUserFiles => !(options && selection.include_ignored),
            WorktreeRefusal::OtherFileSystem | WorktreeRefusal::NoTrashPlace => {
                !(options && selection.now)
            }
            _ => true,
        })
    }

    fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.only_dormant_sessions() {
            warnings.push(format!(
                "{} dormant session(s) in the worktree: a restore of a session will find no \
                 directory",
                self.detail.dormant_sessions
            ));
        }
        if !self.worktree.tool.is_empty() {
            warnings.push(format!(
                "the tool {} may keep a record of this worktree",
                self.worktree.tool
            ));
        }
        warnings
    }

    fn base(&self, step: WorktreePlanStep) -> WorktreePlanItem {
        WorktreePlanItem {
            project_id: self.project_id.to_owned(),
            worktree_path: self.worktree.path.clone(),
            step: step.into(),
            branch: self.worktree.branch.clone(),
            head: self.worktree.head.clone(),
            ..WorktreePlanItem::default()
        }
    }

    fn build_output_steps(&self, refusals: &[WorktreeRefusal]) -> Vec<WorktreePlanItem> {
        let sizes: HashMap<&str, u64> = self
            .worktree
            .size
            .iter()
            .flat_map(|size| &size.build_outputs)
            .map(|output| (output.path.as_str(), output.bytes))
            .collect();
        self.detail
            .build_outputs
            .iter()
            .map(|(path, rule)| WorktreePlanItem {
                path: Path::new(&self.worktree.path)
                    .join(path)
                    .display()
                    .to_string(),
                rule: rule.clone(),
                bytes: sizes.get(path.as_str()).copied().unwrap_or(0),
                refusals: refusals.iter().copied().map(Into::into).collect(),
                warnings: self.warnings(),
                ..self.base(WorktreePlanStep::BuildOutput)
            })
            .collect()
    }

    fn removal_step(&self, refusals: Vec<WorktreeRefusal>) -> WorktreePlanItem {
        let selection = self.selection;
        let (step, path) = if selection.now && !selection.automatic {
            (WorktreePlanStep::Delete, String::new())
        } else {
            (WorktreePlanStep::Trash, trash_path(&self.worktree.path))
        };
        let bytes = self
            .worktree
            .size
            .as_ref()
            .map_or(0, |size| size.total_bytes - size.build_output_bytes);
        let merged_strictly = matches!(
            self.worktree.merged_by(),
            WorktreeMergedBy::Ancestor | WorktreeMergedBy::Tree
        );
        let deletes_branch =
            selection.delete_branch && merged_strictly && !self.worktree.branch.is_empty();
        let branch_kept_reason = match (selection.delete_branch, deletes_branch) {
            (true, false) if self.worktree.branch.is_empty() => {
                "the worktree has no branch".to_owned()
            }
            (true, false) => format!(
                "the base branch does not have each change of the branch {}",
                self.worktree.branch
            ),
            _ => String::new(),
        };
        WorktreePlanItem {
            path,
            bytes,
            refusals: refusals.into_iter().map(Into::into).collect(),
            warnings: self.warnings(),
            deletes_branch,
            branch_kept_reason,
            ignored_user_paths: self.detail.user_paths.clone(),
            ..self.base(step)
        }
    }
}

/// A path in the trash place of the worktree `path`: its parent directory,
/// `.nomux-trash`, its name and a random part.
fn trash_path(path: &str) -> String {
    let path = Path::new(path);
    let name = super::last_part(path);
    let parent = path.parent().unwrap_or(path);
    parent
        .join(TRASH_PLACE)
        .join(format!("{name}-{}", &random_id()[..8]))
        .display()
        .to_string()
}

/// The prune steps of the missing worktrees of one Project, or the repair
/// proposals when a missing worktree moved below the main worktree.
fn prune_steps(
    project_id: &str,
    main: Option<&Path>,
    missing: &[&WorktreeInfo],
) -> Vec<WorktreePlanItem> {
    let item = |worktree: &WorktreeInfo, step: WorktreePlanStep| WorktreePlanItem {
        project_id: project_id.to_owned(),
        worktree_path: worktree.path.clone(),
        step: step.into(),
        branch: worktree.branch.clone(),
        head: worktree.head.clone(),
        ..WorktreePlanItem::default()
    };
    let repairs: Vec<WorktreePlanItem> = missing
        .iter()
        .filter_map(|worktree| {
            let moved = main.and_then(|main| moved_directory(main, Path::new(&worktree.path)))?;
            Some(WorktreePlanItem {
                path: moved.display().to_string(),
                warnings: vec![format!(
                    "run `git worktree repair {}`: a prune would remove the record of this \
                     directory",
                    moved.display()
                )],
                ..item(worktree, WorktreePlanStep::Repair)
            })
        })
        .collect();
    if !repairs.is_empty() {
        return repairs;
    }
    missing
        .iter()
        .map(|worktree| {
            let locked = worktree
                .flags()
                .any(|flag| flag == nomux_proto::wire::v1::WorktreeFlag::Locked);
            WorktreePlanItem {
                refusals: if locked {
                    vec![WorktreeRefusal::Locked.into()]
                } else {
                    Vec::new()
                },
                ..item(worktree, WorktreePlanStep::Prune)
            }
        })
        .collect()
}

/// A directory below `main` at the same relative path as the end of the
/// missing `recorded` path, with the `.git` file of a linked worktree: the
/// directory of the Project moved, and the worktree with it.
fn moved_directory(main: &Path, recorded: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = recorded.components().collect();
    (1..parts.len()).find_map(|start| {
        let candidate = parts[start..]
            .iter()
            .fold(main.to_path_buf(), |path, part| path.join(part));
        let git_file = fs::read_to_string(candidate.join(".git")).ok()?;
        (git_file.starts_with("gitdir:") && git_file.contains("/worktrees/")).then_some(candidate)
    })
}

/// The key of an item: a clean compares the items of two plans with it.
/// The path of a trash step is random, thus not a part of the key.
pub(super) fn key(item: &WorktreePlanItem) -> (String, String, i32, String, String) {
    let path = match item.step() {
        WorktreePlanStep::Trash | WorktreePlanStep::Delete => String::new(),
        _ => item.path.clone(),
    };
    (
        item.project_id.clone(),
        item.worktree_path.clone(),
        item.step,
        path,
        item.head.clone(),
    )
}

#[cfg(test)]
mod tests {
    use nomux_proto::wire::v1::{ProjectWorktrees, WorktreeMergedBy};

    use super::*;

    /// One Project with one finished linked worktree with a `target`, and
    /// `refusals`.
    fn project(refusals: &[WorktreeRefusal]) -> Described {
        Described {
            directory: PathBuf::from("/p"),
            wire: ProjectWorktrees {
                project_id: "p".to_owned(),
                worktrees: vec![WorktreeInfo {
                    path: "/p-w".to_owned(),
                    branch: "w".to_owned(),
                    head: "h".to_owned(),
                    state: WorktreeState::Finished.into(),
                    merged_by: WorktreeMergedBy::UpstreamGone.into(),
                    refusals: refusals.iter().copied().map(Into::into).collect(),
                    ..WorktreeInfo::default()
                }],
                ..ProjectWorktrees::default()
            },
            targets: Vec::new(),
            target_of: vec![None],
            details: vec![Detail {
                build_outputs: vec![("target".to_owned(), "cargo-target".to_owned())],
                state_without_sessions: WorktreeState::Finished,
                ..Detail::default()
            }],
        }
    }

    fn refusals_of_the_trash_step(
        refusals: &[WorktreeRefusal],
        selection: &Selection,
    ) -> Vec<WorktreeRefusal> {
        let items = items(&[project(refusals)], selection);
        let step = items
            .iter()
            .find(|item| {
                matches!(
                    item.step(),
                    WorktreePlanStep::Trash | WorktreePlanStep::Delete
                )
            })
            .expect("a removal step");
        step.refusals().collect()
    }

    #[test]
    fn each_option_removes_only_its_own_refusal_and_a_rule_of_the_settings_has_none() {
        use WorktreeRefusal as R;
        let all = [
            R::UnpushedCommits,
            R::DetachedCommits,
            R::IgnoredUserFiles,
            R::OtherFileSystem,
            R::NoTrashPlace,
        ];
        let command = |change: fn(&mut Selection)| {
            let mut selection = Selection::automatic(Action::Finished);
            selection.automatic = false;
            change(&mut selection);
            selection
        };
        let cases: [(&str, Selection, Vec<R>); 5] = [
            ("no option", command(|_| {}), all.to_vec()),
            (
                "allow unpushed",
                command(|s| s.allow_unpushed = true),
                vec![
                    R::DetachedCommits,
                    R::IgnoredUserFiles,
                    R::OtherFileSystem,
                    R::NoTrashPlace,
                ],
            ),
            (
                "include ignored",
                command(|s| s.include_ignored = true),
                vec![
                    R::UnpushedCommits,
                    R::DetachedCommits,
                    R::OtherFileSystem,
                    R::NoTrashPlace,
                ],
            ),
            (
                "now",
                command(|s| s.now = true),
                vec![R::UnpushedCommits, R::DetachedCommits, R::IgnoredUserFiles],
            ),
            (
                "a rule of the settings with each option",
                {
                    let mut selection = Selection::automatic(Action::Finished);
                    selection.allow_unpushed = true;
                    selection.include_ignored = true;
                    selection.now = true;
                    selection
                },
                all.to_vec(),
            ),
        ];
        for (name, selection, expected) in cases {
            assert_eq!(
                refusals_of_the_trash_step(&all, &selection),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn the_branch_is_deleted_only_by_the_strict_merge_rule() {
        let selection = Selection {
            delete_branch: true,
            ..Selection::of_request(&WorktreePlanRequest::default())
        };
        let mut strict = project(&[]);
        strict.wire.worktrees[0].merged_by = WorktreeMergedBy::Tree.into();

        let gone = items(&[project(&[])], &selection);
        let tree = items(&[strict], &selection);

        let removal = |items: &[WorktreePlanItem]| {
            items
                .iter()
                .find(|item| item.step() == WorktreePlanStep::Trash)
                .cloned()
                .expect("a trash step")
        };
        assert!(!removal(&gone).deletes_branch);
        assert_ne!(removal(&gone).branch_kept_reason, "");
        assert!(removal(&tree).deletes_branch);
        assert_eq!(removal(&tree).branch_kept_reason, "");
    }
}
