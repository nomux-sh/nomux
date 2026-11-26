//! The rules of the hibernation: which agent session can hibernate, and
//! which ones a trigger hibernates. Pure code: the caller gives the facts,
//! the time and the measured memory. `docs/memory.md` has the table.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_status::{NeedsYouKind, State, Timestamp};

/// What the daemon knows about one live agent session: a session with a
/// holder whose adapter gave details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) id: SessionId,
    /// The most urgent state of the Agent run and of its Child work.
    pub(crate) state: State,
    /// When the daemon saw that state for the first time.
    pub(crate) since: Timestamp,
    /// The adapter gave a resume command.
    pub(crate) can_resume: bool,
    /// A client shows the session now.
    pub(crate) attached: bool,
    /// The user excluded the session from automatic hibernation.
    pub(crate) pinned: bool,
    /// When a client last wrote input to the session.
    pub(crate) last_input: Option<Timestamp>,
    /// When the last client detached from the session.
    pub(crate) last_detach: Option<Timestamp>,
}

impl Candidate {
    /// The start of the time without a change that the user made or saw:
    /// a client that detached a moment ago looked at the session.
    fn idle_since(&self) -> Timestamp {
        self.since.max(self.last_detach.unwrap_or(self.since))
    }
}

/// Why a session cannot hibernate now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Block {
    /// The adapter gave no resume command: a stop loses the conversation.
    NoResumeCommand,
    /// Work runs, in the Agent run or in its Child work.
    Works,
    /// The work waits for the user. A resume loses what it waits for, for
    /// example the permission request.
    NeedsYou(Option<NeedsYouKind>),
    /// The work failed: the user must see the failure in the process.
    Failed,
    /// A client shows the session.
    Attached,
    /// A client typed after the last change of the state: the input line
    /// can have text that the agent did not get.
    UnsentInput,
    /// The user excluded the session from automatic hibernation.
    Pinned,
    /// The session is not idle long enough: it can hibernate from this
    /// time on.
    IdleUntil(Timestamp),
}

impl fmt::Display for Block {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoResumeCommand => formatter.write_str(
                "its adapter gave no resume command, thus a stop loses the conversation",
            ),
            Self::Works => formatter.write_str("its agent works"),
            Self::NeedsYou(Some(NeedsYouKind::Permission)) => {
                formatter.write_str("its agent waits for a permission answer")
            }
            Self::NeedsYou(Some(NeedsYouKind::Auth)) => {
                formatter.write_str("its agent waits for a login")
            }
            Self::NeedsYou(_) => formatter.write_str("its agent waits for an answer of the user"),
            Self::Failed => formatter.write_str("its agent reported an error"),
            Self::Attached => formatter.write_str("a client shows it: detach the client first"),
            Self::UnsentInput => formatter
                .write_str("input was typed after the last change of its status and can be unsent"),
            Self::Pinned => formatter.write_str("it is pinned"),
            Self::IdleUntil(_) => formatter.write_str("it is not idle long enough"),
        }
    }
}

/// Why a trigger hibernates a session, with its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// More agent sessions were live than the limit.
    Count { live: usize, max_live: usize },
    /// The processes of the session used more memory than the limit of one
    /// session, which the operating system does not apply.
    SessionMemory { bytes: u64, limit: u64 },
    /// The processes of all sessions used more memory than the limit.
    TotalMemory { bytes: u64, limit: u64 },
    /// The operating system reported memory pressure.
    MemoryPressure(Pressure),
    /// A client asked for it.
    Request,
}

/// The pressure that the operating system reports: processes waited
/// `stall` for memory in `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pressure {
    pub(crate) stall: Duration,
    pub(crate) window: Duration,
}

/// One session that a trigger hibernates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) id: SessionId,
    pub(crate) trigger: Trigger,
}

/// The memory of the process trees of the commands of the live sessions,
/// in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Measurement {
    pub(crate) sessions: HashMap<SessionId, u64>,
    pub(crate) total: u64,
}

/// The candidates that can hibernate now, oldest first, and when the next
/// one can.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Review<'candidates> {
    pub(crate) eligible: Vec<&'candidates Candidate>,
    /// When the daemon must look again, without an event: the time at which
    /// a session is idle long enough and a trigger can apply to it. `None`
    /// if nothing can change without an event.
    pub(crate) next_look: Option<Timestamp>,
}

/// The settings of the triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    pub(crate) hibernate_after: Duration,
    /// 0 turns the count trigger off.
    pub(crate) max_live: usize,
    /// The memory limit of one session as a threshold. `None` if the
    /// configuration has none, or if the operating system applies it.
    pub(crate) session_threshold: Option<u64>,
    pub(crate) total_threshold: Option<u64>,
}

/// The conditions that apply to each hibernation: also to one that a
/// client asks for.
pub(crate) fn safety(candidate: &Candidate) -> Result<(), Block> {
    if !candidate.can_resume {
        return Err(Block::NoResumeCommand);
    }
    match candidate.state {
        State::Idle | State::Done | State::NeedsYou(Some(NeedsYouKind::Prompt)) => {}
        State::Working => return Err(Block::Works),
        State::NeedsYou(kind) => return Err(Block::NeedsYou(kind)),
        State::Error => return Err(Block::Failed),
    }
    if candidate.attached {
        return Err(Block::Attached);
    }
    if candidate
        .last_input
        .is_some_and(|input| input >= candidate.since)
    {
        return Err(Block::UnsentInput);
    }
    Ok(())
}

impl Policy {
    /// If a trigger of the configuration can hibernate `candidate` at
    /// `now`.
    pub(crate) fn eligibility(&self, candidate: &Candidate, now: Timestamp) -> Result<(), Block> {
        safety(candidate)?;
        if candidate.pinned {
            return Err(Block::Pinned);
        }
        let until = Timestamp(
            candidate
                .idle_since()
                .0
                .saturating_add(duration_ms(self.hibernate_after)),
        );
        if now < until {
            return Err(Block::IdleUntil(until));
        }
        Ok(())
    }

    /// The candidates that can hibernate at `now`, the session that is idle
    /// for the longest time first, and the time of the next look.
    pub(crate) fn review<'candidates>(
        &self,
        candidates: &'candidates [Candidate],
        now: Timestamp,
    ) -> Review<'candidates> {
        let mut eligible = Vec::new();
        let mut next_look: Option<Timestamp> = None;
        for candidate in candidates {
            match self.eligibility(candidate, now) {
                Ok(()) => eligible.push(candidate),
                Err(Block::IdleUntil(until)) => {
                    next_look = Some(next_look.map_or(until, |look| look.min(until)));
                }
                Err(_) => {}
            }
        }
        eligible.sort_by(|first, second| {
            (first.idle_since(), first.id.as_str()).cmp(&(second.idle_since(), second.id.as_str()))
        });
        // Without a trigger that can apply later, a session that becomes
        // idle long enough changes nothing: no timer.
        let count_exceeded = self.max_live > 0 && candidates.len() > self.max_live;
        if !count_exceeded && !self.needs_memory() {
            next_look = None;
        }
        Review {
            eligible,
            next_look,
        }
    }

    /// `true` if a trigger needs the memory of the sessions.
    pub(crate) fn needs_memory(&self) -> bool {
        self.session_threshold.is_some() || self.total_threshold.is_some()
    }

    /// The sessions of `eligible` that the triggers hibernate. `live` is
    /// the number of live agent sessions, `memory` the measured memory if a
    /// trigger needs it, and `pressure` the pressure that the operating
    /// system reported now. Each session is chosen one time at most.
    pub(crate) fn choose(
        &self,
        eligible: &[&Candidate],
        live: usize,
        memory: Option<&Measurement>,
        pressure: Option<Pressure>,
    ) -> Vec<Choice> {
        let mut choices: Vec<Choice> = Vec::new();
        let mut remaining = eligible.iter().map(|candidate| &candidate.id);
        if self.max_live > 0 {
            let excess = live.saturating_sub(self.max_live);
            for id in remaining.by_ref().take(excess) {
                let trigger = Trigger::Count {
                    live,
                    max_live: self.max_live,
                };
                choices.push(choice(id, trigger));
            }
        }
        let mut remaining: Vec<&SessionId> = remaining.collect();
        if let (Some(limit), Some(memory)) = (self.session_threshold, memory) {
            remaining.retain(|id| match memory.sessions.get(*id) {
                Some(&bytes) if bytes > limit => {
                    choices.push(choice(id, Trigger::SessionMemory { bytes, limit }));
                    false
                }
                _ => true,
            });
        }
        if let (Some(limit), Some(memory)) = (self.total_threshold, memory) {
            let freed: u64 = choices
                .iter()
                .filter_map(|choice| memory.sessions.get(&choice.id))
                .sum();
            let mut total = memory.total.saturating_sub(freed);
            let bytes = memory.total;
            while total > limit && !remaining.is_empty() {
                let id = remaining.remove(0);
                total = total.saturating_sub(memory.sessions.get(id).copied().unwrap_or(0));
                choices.push(choice(id, Trigger::TotalMemory { bytes, limit }));
            }
        }
        if let (Some(pressure), Some(id)) = (pressure, remaining.first())
            && choices.is_empty()
        {
            choices.push(choice(id, Trigger::MemoryPressure(pressure)));
        }
        choices
    }
}

fn choice(id: &SessionId, trigger: Trigger) -> Choice {
    Choice {
        id: id.clone(),
        trigger,
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;
    /// The time of each check: ten minutes after the start of the clock.
    const NOW: Timestamp = Timestamp(10 * MINUTE);

    fn policy() -> Policy {
        Policy {
            hibernate_after: Duration::from_mins(1),
            max_live: 2,
            session_threshold: None,
            total_threshold: None,
        }
    }

    /// An agent session that can hibernate: idle for five minutes, with a
    /// resume command, not shown, not pinned, without input.
    fn idle(id: &str) -> Candidate {
        Candidate {
            id: id.parse().unwrap(),
            state: State::Idle,
            since: Timestamp(5 * MINUTE),
            can_resume: true,
            attached: false,
            pinned: false,
            last_input: None,
            last_detach: None,
        }
    }

    /// A name, a change of a candidate that can hibernate, and the block
    /// that the change gives.
    type Case = (&'static str, fn(&mut Candidate), Block);

    fn ids(choices: &[Choice]) -> Vec<&str> {
        choices.iter().map(|choice| choice.id.as_str()).collect()
    }

    #[test]
    fn each_safety_condition_blocks_a_hibernation() {
        let blocked = |change: fn(&mut Candidate)| {
            let mut candidate = idle("a");
            change(&mut candidate);
            policy().eligibility(&candidate, NOW).unwrap_err()
        };
        let cases: [Case; 11] = [
            (
                "no resume command",
                |c| c.can_resume = false,
                Block::NoResumeCommand,
            ),
            ("work runs", |c| c.state = State::Working, Block::Works),
            (
                "a permission request waits",
                |c| c.state = State::NeedsYou(Some(NeedsYouKind::Permission)),
                Block::NeedsYou(Some(NeedsYouKind::Permission)),
            ),
            (
                "a question waits",
                |c| c.state = State::NeedsYou(Some(NeedsYouKind::Question)),
                Block::NeedsYou(Some(NeedsYouKind::Question)),
            ),
            (
                "a login waits",
                |c| c.state = State::NeedsYou(Some(NeedsYouKind::Auth)),
                Block::NeedsYou(Some(NeedsYouKind::Auth)),
            ),
            (
                "the source does not say what waits",
                |c| c.state = State::NeedsYou(None),
                Block::NeedsYou(None),
            ),
            ("the work failed", |c| c.state = State::Error, Block::Failed),
            ("a client shows it", |c| c.attached = true, Block::Attached),
            (
                "input after the last change",
                |c| c.last_input = Some(Timestamp(6 * MINUTE)),
                Block::UnsentInput,
            ),
            ("pinned", |c| c.pinned = true, Block::Pinned),
            (
                "a detach a moment ago",
                |c| c.last_detach = Some(Timestamp(NOW.0 - 1000)),
                Block::IdleUntil(Timestamp(NOW.0 - 1000 + MINUTE)),
            ),
        ];
        for (case, change, block) in cases {
            assert_eq!(blocked(change), block, "{case}");
        }
        assert_eq!(
            policy().eligibility(&idle("a"), Timestamp(5 * MINUTE + 1000)),
            Err(Block::IdleUntil(Timestamp(6 * MINUTE))),
            "a change a second ago"
        );
    }

    #[test]
    fn idle_done_and_a_wait_at_the_prompt_can_hibernate_and_input_before_the_change_does_not_block()
    {
        for state in [
            State::Idle,
            State::Done,
            State::NeedsYou(Some(NeedsYouKind::Prompt)),
        ] {
            let candidate = Candidate {
                state,
                // The input that started the work that is done now.
                last_input: Some(Timestamp(4 * MINUTE)),
                ..idle("a")
            };
            assert_eq!(policy().eligibility(&candidate, NOW), Ok(()), "{state:?}");
        }
    }

    #[test]
    fn a_request_of_a_client_skips_the_idle_time_and_the_pin_and_no_safety_condition() {
        let recent_and_pinned = Candidate {
            since: Timestamp(NOW.0 - 1),
            pinned: true,
            ..idle("a")
        };
        assert_eq!(safety(&recent_and_pinned), Ok(()));
        let attached = Candidate {
            attached: true,
            ..recent_and_pinned
        };
        assert_eq!(safety(&attached), Err(Block::Attached));
    }

    #[test]
    fn the_count_hibernates_the_sessions_that_are_idle_for_the_longest_time() {
        let candidates = [
            Candidate {
                since: Timestamp(3 * MINUTE),
                ..idle("c")
            },
            Candidate {
                since: Timestamp(MINUTE),
                ..idle("b")
            },
            Candidate {
                state: State::Working,
                ..idle("a")
            },
            Candidate {
                since: Timestamp(2 * MINUTE),
                ..idle("d")
            },
        ];
        let review = policy().review(&candidates, NOW);
        let chosen = policy().choose(&review.eligible, candidates.len(), None, None);

        assert_eq!(ids(&chosen), ["b", "d"], "4 live, 2 at most");
        assert_eq!(
            chosen[0].trigger,
            Trigger::Count {
                live: 4,
                max_live: 2
            }
        );
        let off = Policy {
            max_live: 0,
            ..policy()
        };
        assert_eq!(off.choose(&review.eligible, 4, None, None), []);
    }

    #[test]
    fn the_next_look_is_the_time_when_a_session_is_idle_long_enough_only_if_a_trigger_can_apply() {
        let soon = Candidate {
            since: Timestamp(NOW.0 - 1000),
            ..idle("a")
        };
        let candidates = [soon.clone(), idle("b"), idle("c")];
        let review = policy().review(&candidates, NOW);
        assert_eq!(review.next_look, Some(Timestamp(NOW.0 - 1000 + MINUTE)));

        // Two live agents, two at most, and no memory threshold: no timer.
        let under_the_count = [soon, idle("b")];
        assert_eq!(policy().review(&under_the_count, NOW).next_look, None);
        let with_threshold = Policy {
            total_threshold: Some(1),
            ..policy()
        };
        assert_eq!(
            with_threshold.review(&under_the_count, NOW).next_look,
            Some(Timestamp(NOW.0 - 1000 + MINUTE))
        );
    }

    #[test]
    fn the_memory_thresholds_hibernate_until_the_total_is_below_its_limit() {
        let policy = Policy {
            max_live: 0,
            session_threshold: Some(500),
            total_threshold: Some(1000),
            ..policy()
        };
        let candidates = [
            Candidate {
                since: Timestamp(MINUTE),
                ..idle("a")
            },
            Candidate {
                since: Timestamp(2 * MINUTE),
                ..idle("b")
            },
            Candidate {
                since: Timestamp(3 * MINUTE),
                ..idle("c")
            },
            idle("d"),
        ];
        let memory = Measurement {
            sessions: [("a", 100), ("b", 300), ("c", 600), ("d", 200)]
                .into_iter()
                .map(|(id, bytes)| (id.parse().unwrap(), bytes))
                .collect(),
            // A session that is not an agent session has 300.
            total: 1500,
        };
        let review = policy.review(&candidates, NOW);
        let chosen = policy.choose(&review.eligible, 4, Some(&memory), None);

        // c is above the limit of one session. Without c, the total is 900.
        assert_eq!(ids(&chosen), ["c"]);
        assert_eq!(
            chosen[0].trigger,
            Trigger::SessionMemory {
                bytes: 600,
                limit: 500
            }
        );

        let lower_total = Policy {
            session_threshold: None,
            total_threshold: Some(700),
            ..policy
        };
        let chosen = lower_total.choose(&review.eligible, 4, Some(&memory), None);
        // The oldest first, until 1500 - 100 - 300 - 600 = 500 is below 700.
        assert_eq!(ids(&chosen), ["a", "b", "c"]);
        assert_eq!(
            chosen[0].trigger,
            Trigger::TotalMemory {
                bytes: 1500,
                limit: 700
            }
        );
    }

    #[test]
    fn a_pressure_report_hibernates_one_session_if_no_other_trigger_does() {
        let pressure = Pressure {
            stall: Duration::from_millis(200),
            window: Duration::from_secs(2),
        };
        let candidates = [
            Candidate {
                since: Timestamp(2 * MINUTE),
                ..idle("b")
            },
            Candidate {
                since: Timestamp(MINUTE),
                ..idle("a")
            },
        ];
        let review = policy().review(&candidates, NOW);
        let chosen = policy().choose(&review.eligible, 2, None, Some(pressure));
        assert_eq!(ids(&chosen), ["a"]);
        assert_eq!(chosen[0].trigger, Trigger::MemoryPressure(pressure));

        let chosen = policy().choose(&review.eligible, 3, None, Some(pressure));
        assert_eq!(ids(&chosen), ["a"], "the count already frees memory");
        assert!(matches!(chosen[0].trigger, Trigger::Count { .. }));
    }
}
