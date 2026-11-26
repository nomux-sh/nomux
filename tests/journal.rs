//! The journal of a session through the daemon: a client that attaches
//! again gets the output that left the ring buffer of the holder, a client
//! learns that output is gone, and `nomux capture` reads the scrollback.

use std::fs;

use nomux_client::{
    AttachEnd, AttachOptions, CaptureRange, Client, ClientKind, OutputPosition, TerminalEvent,
};
use nomux_end_to_end::{Sandbox, wait_for};
use nomux_holder_protocol::journal::segment_files;
use nomux_holder_protocol::session_state::JOURNAL_DIRECTORY;

/// The ring buffer of a holder has 262144 bytes.
const RING_BYTES: usize = 262_144;

/// A shell command that writes the numbers from 1 to `last`, each on its
/// own line, to a terminal that changes no output byte and does not echo.
///
/// Without `-opost`, the terminal puts a carriage return before each line
/// feed, and the terminal of macOS sometimes puts two. `seq` of macOS
/// writes a large number with an exponent.
fn write_numbers(last: u32) -> String {
    format!("stty -echo -opost; awk 'BEGIN {{ for (i = 1; i <= {last}; i++) print i }}'")
}

/// What a terminal gets from [`write_numbers`].
fn numbers(last: u32) -> Vec<u8> {
    (1..=last)
        .flat_map(|number| format!("{number}\n").into_bytes())
        .collect()
}

/// Fails with the place of the first difference: the two byte sequences
/// have megabytes.
fn assert_same_bytes(received: &[u8], expected: &[u8]) {
    if received == expected {
        return;
    }
    let first = received
        .iter()
        .zip(expected)
        .position(|(received, expected)| received != expected)
        .unwrap_or(received.len().min(expected.len()));
    let around = |bytes: &[u8]| {
        let part = &bytes[first.saturating_sub(40)..bytes.len().min(first + 40)];
        String::from_utf8_lossy(part).into_owned()
    };
    panic!(
        "{} bytes and not the {} expected bytes. The first difference is at byte {first}: \
         received {:?}, expected {:?}",
        received.len(),
        expected.len(),
        around(received),
        around(expected)
    );
}

async fn connect(sandbox: &Sandbox) -> Client {
    Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
        .await
        .expect("the daemon answers")
}

fn new_session(sandbox: &Sandbox, script: &str) -> String {
    sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh", "-c", script])
        .stdout
        .trim()
        .to_owned()
}

/// Waits until the command of the session `id` wrote `bytes` output bytes.
async fn wait_for_output(sandbox: &Sandbox, id: &str, bytes: usize) {
    let mut client = connect(sandbox).await;
    let range = CaptureRange {
        max_bytes: Some(1),
        ..CaptureRange::default()
    };
    for _ in 0..2000 {
        let newest = client.capture(id, range).await.unwrap();
        if newest.seq + newest.bytes.len() as u64 >= bytes as u64 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the session did not write {bytes} bytes");
}

/// One event of an attach, with its bytes.
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Snapshot,
    Output(Vec<u8>),
    HistoryGap,
}

/// Attaches at `position` and reads until the session ends. Events of the
/// same kind that follow each other are one event.
async fn attach_until_the_end(
    sandbox: &Sandbox,
    id: &str,
    position: OutputPosition,
    input: &[u8],
) -> (Vec<Event>, Vec<OutputPosition>, AttachEnd) {
    let options = AttachOptions {
        size: None,
        resume: Some(position),
    };
    let (mut output, mut terminal_input) =
        connect(sandbox).await.attach(id, options).await.unwrap();
    terminal_input.send(input).await.unwrap();
    let mut events: Vec<Event> = Vec::new();
    // The position of the output after each snapshot.
    let mut snapshot_positions = Vec::new();
    loop {
        match output.next().await.expect("the attach continues") {
            TerminalEvent::Snapshot { starts, .. } => {
                if starts {
                    events.push(Event::Snapshot);
                }
            }
            TerminalEvent::Output(bytes) => {
                let bytes = bytes.to_vec();
                if events.last() == Some(&Event::Snapshot) {
                    let after = output.position().unwrap();
                    snapshot_positions.push(OutputPosition {
                        epoch: after.epoch,
                        seq: after.seq - bytes.len() as u64,
                    });
                }
                match events.last_mut() {
                    Some(Event::Output(all)) => all.extend_from_slice(&bytes),
                    _ => events.push(Event::Output(bytes)),
                }
            }
            TerminalEvent::HistoryGap => events.push(Event::HistoryGap),
            TerminalEvent::End(end) => return (events, snapshot_positions, end),
            event => panic!("an attach of one connection gave {event:?}"),
        }
    }
}

#[tokio::test]
async fn a_client_that_was_away_for_more_than_the_ring_buffer_gets_each_missing_byte() {
    let sandbox = Sandbox::new();
    // The command writes the numbers, waits for one line of input and
    // writes it again.
    let script = format!("{}; read line; echo \"after $line\"", write_numbers(80000));
    let id = new_session(&sandbox, &script);
    let written = numbers(80000);
    assert!(written.len() > RING_BYTES + 100_000);
    wait_for_output(&sandbox, &id, written.len()).await;

    // The client has the first 1000 bytes. The ring buffer has only the
    // last 262144 bytes: the bytes between come from the journal.
    let position = OutputPosition {
        epoch: 1,
        seq: 1000,
    };
    let (events, _, end) = attach_until_the_end(&sandbox, &id, position, b"input\n").await;

    let mut expected = written[1000..].to_vec();
    expected.extend_from_slice(b"after input\n");
    // One run of output: no snapshot, no gap and no repeat, from the
    // journal to the ring buffer to the output that the command writes
    // during the attach.
    let [Event::Output(received)] = events.as_slice() else {
        panic!("not one run of output: {} events", events.len());
    };
    assert_same_bytes(received, &expected);
    assert_eq!(end, AttachEnd::Exited { code: 0 });
}

#[tokio::test]
async fn a_client_learns_that_output_before_the_oldest_journal_segment_is_gone() {
    let sandbox = Sandbox::new();
    // Output that needs more than one journal segment: a segment has 2 MiB
    // of compressed output.
    let last = 2_500_000;
    let script = format!("{}; read line; echo \"after $line\"", write_numbers(last));
    let id = new_session(&sandbox, &script);
    let written = numbers(last);
    wait_for_output(&sandbox, &id, written.len()).await;

    let journal = sandbox.session_state_directory(&id).join(JOURNAL_DIRECTORY);
    let segments = wait_for("the journal has three segments", || {
        let segments = segment_files(&journal).unwrap();
        (segments.len() >= 3).then_some(segments)
    });
    // What the size limit of the journal does to the oldest output.
    fs::remove_file(&segments[0].path).unwrap();
    let oldest = usize::try_from(segments[1].name.start).unwrap();

    let position = OutputPosition {
        epoch: 1,
        seq: 1000,
    };
    let (events, snapshots, end) = attach_until_the_end(&sandbox, &id, position, b"input\n").await;

    // The client is told about the gap, and then it gets the oldest screen
    // that the journal has and each byte from there on.
    let [Event::HistoryGap, Event::Snapshot, Event::Output(received)] = events.as_slice() else {
        panic!("not a gap, a screen and output: {} events", events.len());
    };
    let [screen] = snapshots.as_slice() else {
        panic!("not one screen: {snapshots:?}");
    };
    let screen = usize::try_from(screen.seq).unwrap();
    assert!(screen >= oldest, "{screen} is before {oldest}");
    // The screen is from the oldest segment that has a checkpoint.
    assert!(
        screen <= usize::try_from(segments[2].name.start).unwrap(),
        "{screen} is not in the oldest segment"
    );
    let mut expected = written[screen..].to_vec();
    expected.extend_from_slice(b"after input\n");
    assert_same_bytes(received, &expected);
    assert_eq!(end, AttachEnd::Exited { code: 0 });
}

#[test]
fn capture_gives_the_last_lines_also_when_they_left_the_ring_buffer() {
    let sandbox = Sandbox::new();
    let id = new_session(&sandbox, &format!("{}; sleep 300", write_numbers(80000)));
    let written = numbers(80000);
    wait_for("the command wrote its output", || {
        let last_line = sandbox.nomux_ok(&["capture", &id, "--lines", "1"]).stdout;
        (last_line == "80000\n").then_some(())
    });

    let three = sandbox.nomux_ok(&["capture", &id, "--lines", "3"]).stdout;
    assert_eq!(three, "79998\n79999\n80000\n");

    // More lines than the ring buffer has: the lines from 10001 on have
    // 420000 bytes.
    let many = sandbox
        .nomux_ok(&["capture", &id, "--lines", "70000"])
        .stdout;
    assert!(many.len() > RING_BYTES);
    assert_eq!(many.as_bytes(), &written[written.len() - many.len()..]);
    assert!(many.starts_with("10001\n"), "{}", &many[..20]);

    // More lines than the session wrote: all of its output.
    let all = sandbox
        .nomux_ok(&["capture", &id, "--lines", "100000"])
        .stdout;
    assert!(all.as_bytes() == written.as_slice());

    let document = sandbox
        .nomux_ok(&["capture", &id, "--lines", "2", "--json"])
        .json();
    assert_eq!(document["capture"]["id"], id.as_str());
    assert_eq!(document["capture"]["output"], "79999\n80000\n");

    assert_eq!(sandbox.nomux(&["capture", "no-session"]).code, 2);
}
