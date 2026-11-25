//! The payload of `ext call`, as the command line gives it.

use std::fs;

use nomux_client::ClientError;

/// The sign before the path of a file with the payload.
const FILE_SIGN: char = '@';

/// The bytes of the payload: the text of the argument, or the content of the
/// file after `@`. Without an argument, the payload is empty.
pub(super) fn read(argument: Option<&str>) -> Result<Vec<u8>, ClientError> {
    let Some(argument) = argument else {
        return Ok(Vec::new());
    };
    let Some(file) = argument.strip_prefix(FILE_SIGN) else {
        return Ok(argument.as_bytes().to_vec());
    };
    fs::read(file).map_err(|error| {
        ClientError::Failed(format!("the payload file {file} cannot be read: {error}"))
    })
}
