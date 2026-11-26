//! The git extension of the end-to-end tests: it has the identifier of the
//! real git extension, `nomux.git`, and only its method `worktree_facts`.
//! It does not read a repository: it gives the facts that a test wrote
//! before, as an encoded `WorktreeFactsResponse` in the file
//! `GIT_FACTS_FILE` of the home directory. The facts of a real repository are tested in the
//! `extensions` repository.

use std::path::PathBuf;
use std::{env, fs};

use nomux_end_to_end::GIT_FACTS_FILE;
use nomux_extension::{Call, CallError, ErrorCode, Extension, RunError};
use nomux_proto::wire::v1::{WorktreeFactsRequest, WorktreeFactsResponse};
use prost::Message;

fn main() -> Result<(), RunError> {
    Extension::new("nomux.git", env!("CARGO_PKG_VERSION"))
        .method("worktree_facts", facts)
        .run()
}

fn facts(_: &Call, _: WorktreeFactsRequest) -> Result<WorktreeFactsResponse, CallError> {
    let failed = |message: String| CallError::new(ErrorCode::Failed, message);
    let home = env::var_os("HOME").ok_or_else(|| failed("HOME is not set".to_owned()))?;
    let bytes = fs::read(PathBuf::from(home).join(GIT_FACTS_FILE))
        .map_err(|error| failed(format!("no facts: {error}")))?;
    WorktreeFactsResponse::decode(bytes.as_slice())
        .map_err(|error| failed(format!("the facts are not valid: {error}")))
}
