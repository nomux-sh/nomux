//! The session state directory: the session metadata that the restore after
//! a restart of the host reads without the holder.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use nomux_holder_protocol::session_state::{
    JOURNAL_DIRECTORY, METADATA_FILE, RestorePolicy, SessionMetadata,
};
use nomux_holder_protocol::wire::{ChildStatus, Reply, Request, WindowSize};

use crate::fixture::{Client, holder_command, runtime_directory, state_directory, wait_for};

const SECRET_NAME: &str = "NOMUX_TEST_SECRET";
const SECRET_VALUE: &str = "value-that-must-stay-in-memory";

fn mode(path: &Path) -> u32 {
    let metadata = fs::metadata(path).expect("the path exists");
    metadata.permissions().mode() & 0o777
}

fn read_metadata(path: &Path) -> (String, SessionMetadata) {
    let text = fs::read_to_string(path).expect("the metadata file exists");
    let metadata = SessionMetadata::parse(&text).expect("the metadata is valid");
    (text, metadata)
}

fn now() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    now.expect("the clock is after 1970").as_secs()
}

#[test]
fn the_metadata_file_has_what_starts_the_command_again_and_no_environment() {
    let runtime = runtime_directory();
    let work = runtime.path().join("work directory");
    fs::create_dir(&work).expect("the test helper works");
    let script = "printf started; read line; exit 3";
    let work_text = work.to_str().expect("the path is text");
    let options = [
        "--rows",
        "30",
        "--columns",
        "90",
        "--working-directory",
        work_text,
    ];
    let command = ["/bin/sh", "-c", script, "argument\nwith two lines"];
    let before_start = now();
    let mut holder = holder_command(runtime.path(), "meta", &options, &command)
        .env(SECRET_NAME, SECRET_VALUE)
        .spawn()
        .expect("the test helper works");
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.read_until("started");

    let session_directory = state_directory(runtime.path()).join("sessions/meta");
    let file = session_directory.join(METADATA_FILE);
    let (text, metadata) = read_metadata(&file);
    assert_eq!(metadata.epoch, 1);
    assert_eq!(metadata.epoch, client.hello.epoch);
    assert!((before_start..=now()).contains(&metadata.started));
    assert_eq!(
        metadata.size,
        WindowSize {
            rows: 30,
            columns: 90
        }
    );
    assert_eq!(metadata.restore_policy, RestorePolicy::OnFocus);
    assert_eq!(metadata.profile, "");
    assert_eq!(metadata.working_directory, work);
    assert_eq!(metadata.command, "/bin/sh");
    assert_eq!(
        metadata.arguments,
        ["-c", script, "argument\nwith two lines"]
    );
    assert_eq!(
        (metadata.status, metadata.ended),
        (ChildStatus::Running, None)
    );
    assert!(
        !text.contains(SECRET_NAME) && !text.contains(SECRET_VALUE),
        "{text}"
    );
    // Only the user can read what the session wrote.
    assert_eq!(mode(&session_directory), 0o700);
    assert_eq!(mode(&session_directory.join(JOURNAL_DIRECTORY)), 0o700);
    assert_eq!(mode(&file), 0o600);
    for segment in fs::read_dir(session_directory.join(JOURNAL_DIRECTORY)).expect("it exists") {
        assert_eq!(mode(&segment.expect("the entry can be read").path()), 0o600);
    }

    let resized = WindowSize {
        rows: 50,
        columns: 120,
    };
    assert_eq!(client.request(Request::Resize(resized)), Reply::Done);
    assert_eq!(read_metadata(&file).1.size, resized);

    client.type_line("");
    assert_eq!(
        client.wait_for_exit_status(),
        ChildStatus::Exited { code: 3 }
    );
    wait_for("the holder exits", || {
        holder.try_wait().expect("the test helper works")
    });
    // The directory stays after the holder ended.
    let (text, metadata) = read_metadata(&file);
    assert_eq!(metadata.status, ChildStatus::Exited { code: 3 });
    assert!(
        metadata
            .ended
            .is_some_and(|ended| ended >= metadata.started)
    );
    assert_eq!(metadata.size, resized);
    assert!(
        !text.contains(SECRET_NAME) && !text.contains(SECRET_VALUE),
        "{text}"
    );
}
