//! A target: the text with which a user names one session. It is the
//! identifier, the name, or a unique prefix of one of the two. The protocol
//! specification has the rule (`spec/control.md`, "Names").

use crate::{ClientError, Session, SessionState};

/// The session that `target` names among `sessions`: the list of the host
/// with the ended sessions.
///
/// The first step that finds a session wins: (1) the identifier is the
/// target; (2) a session that is not ended has the target as its name;
/// (3) the identifier or the name of exactly one session that is not ended
/// starts with the target. An ended session is found only by its
/// identifier.
///
/// # Errors
///
/// Returns [`ClientError::NotFound`] if no session matches, and
/// [`ClientError::AmbiguousTarget`] with the candidates if step 3 finds
/// more than one session.
pub fn resolve_session(sessions: Vec<Session>, target: &str) -> Result<Session, ClientError> {
    let mut sessions = sessions;
    if let Some(index) = sessions.iter().position(|session| session.id == target) {
        return Ok(sessions.swap_remove(index));
    }
    sessions.retain(|session| session.state != SessionState::Ended);
    if let Some(index) = sessions
        .iter()
        .position(|session| session.name.as_deref() == Some(target))
    {
        return Ok(sessions.swap_remove(index));
    }
    sessions.retain(|session| {
        !target.is_empty()
            && (session.id.starts_with(target)
                || session
                    .name
                    .as_deref()
                    .is_some_and(|name| name.starts_with(target)))
    });
    match sessions.len() {
        0 => Err(ClientError::NotFound(format!(
            "no session has the identifier or the name {target}, and no session has a name or \
             an identifier that starts with it"
        ))),
        1 => Ok(sessions.remove(0)),
        _ => Err(ClientError::AmbiguousTarget {
            target: target.to_owned(),
            candidates: sessions,
        }),
    }
}

/// The session that a user means when the user names none: of the sessions
/// that are not ended, the one that a client attached to last, else the one
/// that started last. `None` if each session ended.
#[must_use]
pub fn most_recent_session(sessions: Vec<Session>) -> Option<Session> {
    sessions
        .into_iter()
        .filter(|session| session.state != SessionState::Ended)
        .max_by_key(|session| (session.last_attached_at, session.started_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, name: Option<&str>, state: SessionState) -> Session {
        Session {
            id: id.to_owned(),
            name: name.map(str::to_owned),
            state,
            pid: None,
            command: Vec::new(),
            working_directory: None,
            resolved_working_directory: None,
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
            last_attached_at: None,
            attached_clients: 0,
            hibernation: None,
            pinned: false,
        }
    }

    fn host() -> Vec<Session> {
        vec![
            session("k3xq2abc", Some("api"), SessionState::Running),
            session("k3xz7def", Some("api-v2"), SessionState::Dormant),
            session("m4ab5ghi", None, SessionState::Running),
            session("p5cd6jkl", Some("build"), SessionState::Ended),
            session("q6ef7mno", Some("m4ab"), SessionState::Ended),
        ]
    }

    fn resolved(target: &str) -> Result<String, String> {
        match resolve_session(host(), target) {
            Ok(session) => Ok(session.id),
            Err(ClientError::NotFound(_)) => Err("not found".to_owned()),
            Err(ClientError::AmbiguousTarget { candidates, .. }) => Err(candidates
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>()
                .join(",")),
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn the_most_recent_session_is_the_last_attached_then_the_last_started() {
        let at = |id: &str, attached: Option<u64>, started: u64, state| Session {
            last_attached_at: attached,
            started_at: Some(started),
            ..session(id, None, state)
        };
        let most_recent = |sessions| most_recent_session(sessions).map(|session| session.id);
        assert_eq!(
            most_recent(vec![
                at("old", Some(5), 1, SessionState::Running),
                at("new", None, 9, SessionState::Running),
                at("ended", Some(7), 1, SessionState::Ended),
            ]),
            Some("old".to_owned())
        );
        assert_eq!(
            most_recent(vec![
                at("a", None, 1, SessionState::Dormant),
                at("b", None, 3, SessionState::Running),
            ]),
            Some("b".to_owned())
        );
        assert_eq!(
            most_recent(vec![at("x", Some(1), 1, SessionState::Ended)]),
            None
        );
    }

    #[test]
    fn the_first_step_that_finds_a_session_wins() {
        let table: &[(&str, Result<&str, &str>)] = &[
            // An identifier, also of an ended session.
            ("k3xq2abc", Ok("k3xq2abc")),
            ("p5cd6jkl", Ok("p5cd6jkl")),
            // An exact name wins over a longer name with the same start.
            ("api", Ok("k3xq2abc")),
            ("api-v2", Ok("k3xz7def")),
            // A unique prefix of a name or of an identifier.
            ("api-", Ok("k3xz7def")),
            ("m4", Ok("m4ab5ghi")),
            ("k3xq", Ok("k3xq2abc")),
            // A prefix of two sessions: the candidates, nothing is chosen.
            ("k3x", Err("k3xq2abc,k3xz7def")),
            ("ap", Err("k3xq2abc,k3xz7def")),
            // An ended session is found by its identifier only: its name
            // and its prefix name nothing.
            ("build", Err("not found")),
            ("p5", Err("not found")),
            // The name of an ended session does not hide a prefix of a
            // session that is not ended.
            ("m4ab", Ok("m4ab5ghi")),
            ("zz", Err("not found")),
            ("", Err("not found")),
        ];
        for (target, expected) in table {
            let expected = expected.map(str::to_owned).map_err(str::to_owned);
            assert_eq!(resolved(target), expected, "target {target:?}");
        }
    }
}
