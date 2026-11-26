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
