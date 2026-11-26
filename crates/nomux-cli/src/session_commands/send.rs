//! `send`: text or keys for a session, as if the user typed them, without
//! an attach. The bytes go in one `Session.Input` request, thus they stay
//! together.

use nomux_client::{ClientError, Host};

use super::SendArguments;
use super::key_names::key_bytes;
use crate::commands::KIND;
use crate::report::Report;

const ENTER: u8 = b'\r';

pub(crate) async fn send(host: &Host, arguments: &SendArguments) -> Result<Report, ClientError> {
    let Some((target, words)) = arguments.target_and_words() else {
        return Err(ClientError::Failed(
            "send needs the session and the text: `nomux send <target> <text>`".to_owned(),
        ));
    };
    let bytes = if arguments.keys {
        words
            .iter()
            .map(|word| key_bytes(word))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ClientError::Failed)?
            .concat()
    } else {
        text_bytes(&words.join(" "), !arguments.no_enter)?
    };
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target).await?;
    client.send_input(&session.id, &bytes).await?;
    Ok(Report::Sent {
        session,
        bytes: bytes.len(),
    })
}

/// The bytes of `text`, and Enter after them with `enter`.
fn text_bytes(text: &str, enter: bool) -> Result<Vec<u8>, ClientError> {
    if text.contains(['\n', '\r']) {
        return Err(ClientError::Failed(
            "the text has a line break: send one line at a time, or each key with -k".to_owned(),
        ));
    }
    let mut bytes = text.as_bytes().to_vec();
    if enter {
        bytes.push(ENTER);
    }
    Ok(bytes)
}
