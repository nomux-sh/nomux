//! The program reports: what the holder keeps from OSC 7501, OSC 133,
//! OSC 9 and OSC 777 in the output, and the two requests that give it to
//! the daemon.

use nomux_holder_protocol::program_reports::{
    BlockedKind, Notification, ProgramState, ProgramStatus, PromptMark,
};
use nomux_holder_protocol::wire::{Reply, Request};

use crate::fixture::{Client, Holder, runtime_directory};

/// Puts the name that the operating system has for the shell in `$app`.
/// `/bin/sh` is a different program on each host.
const OWN_NAME: &str = "app=$(ps -o ucomm= -p $$ | tr -d ' ');";

/// Reports a root record and a record below it with the name of the shell,
/// a record with the name of a different program, and a record that it
/// removes again. Then it waits for a line and reports the end of the work.
const REPORT_STATUS: &str = concat!(
    r#"printf '\033]7501;state=working:app=%s:msg=QnVpbGRpbmcgZG9jcw==\033\\' "$app";"#,
    r"printf '\033]7501;state=blocked:kind=permission:id=deploy/eu-west:",
    r"title=RVUgV2VzdA==:progress=40\007';",
    r"printf '\033]7501;state=blocked:kind=permission:app=claude-code:id=forged\007';",
    r"printf '\033]7501;state=blocked:kind=question:app=claude-code\007';",
    r"printf '\033]7501;state=done:id=temporary\007';",
    r"printf '\033]7501;state=clear:id=temporary\007';",
    "printf first-part; read line;",
    r#"printf '\033]7501;state=done:app=%s\007' "$app";"#,
    "printf second-part; read line",
);

/// Sends the query, reads the answer of the terminal as a line and compares
/// it with the answer of the specification. The shell computes the marker.
const QUERY: &str = concat!(
    "stty -echo;",
    r"printf '\033]7501;?\033\\';",
    "IFS= read -r reply;",
    r"expected=$(printf '\033]7501;?:states=idle,working,done,blocked,error:",
    r"kinds=permission,question,auth\033\\');",
    r#"[ "$reply" = "$expected" ] && echo "reply-$((6*7))";"#,
    "read line",
);
const END_OF_QUERY: &[u8] = b"\x1b]7501;?\x1b\\";

/// Writes one report in two parts, and waits for a line between them.
const REPORT_IN_PARTS: &str = concat!(
    "stty -echo;",
    r#"printf '\033]7501;state=working:app=%s:msg=QnVpbGRp' "$app"; read line;"#,
    r"printf 'bmcgZG9jcw==\007'; printf complete; read line",
);
const END_OF_FIRST_PART: &[u8] = b"msg=QnVpbGRp";

/// Writes the marks of a prompt, reads two lines, then writes the marks of
/// a command that ends with the exit code 3 and of the next prompt.
const PROMPT_MARKS: &str = concat!(
    "stty -echo;",
    r"printf '\033]133;A\007$ \033]133;B\007'; read first; read second;",
    r"printf '\033]133;C\007\033]133;D;3\007\033]133;A\007'; printf next-prompt; read line",
);
const END_OF_PROMPT: &[u8] = b"\x1b]133;B\x07";

/// Two notifications, and a progress report of `ConEmu` that is not one.
const NOTIFY: &str = concat!(
    r"printf '\033]9;Build finished\007';",
    r"printf '\033]777;notify;Deploy;eu-west is ready\033\\';",
    r"printf '\033]9;4;1;50\007'; printf notified; read line",
);

fn shell(script: &str) -> [String; 3] {
    [
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        format!("{OWN_NAME}{script}"),
    ]
}

fn start(session: &str, script: &str) -> (tempfile::TempDir, Holder) {
    let runtime = runtime_directory();
    let command = shell(script);
    let command: Vec<&str> = command.iter().map(String::as_str).collect();
    let holder = Holder::start(runtime.path(), session, &[], &command);
    (runtime, holder)
}

fn status<'a>(statuses: &'a [ProgramStatus], id: &str) -> &'a ProgramStatus {
    statuses
        .iter()
        .find(|status| status.id == id)
        .unwrap_or_else(|| panic!("no record {id:?} in {statuses:?}"))
}

#[test]
fn the_holder_keeps_the_reports_of_the_foreground_program_and_gives_them_to_each_daemon() {
    let (runtime, _holder) = start("status", REPORT_STATUS);
    let mut client = Client::connect(runtime.path());
    client.read_until("first-part");

    let reports = client.program_reports();

    let ids: Vec<&str> = reports
        .statuses
        .iter()
        .map(|status| status.id.as_str())
        .collect();
    assert_eq!(ids, ["", "deploy/eu-west"], "{:?}", reports.statuses);
    let root = status(&reports.statuses, "");
    assert_eq!(root.state, ProgramState::Working);
    assert!(root.app.is_some());
    assert_eq!(root.message.as_deref(), Some("Building docs"));
    let child = status(&reports.statuses, "deploy/eu-west");
    assert_eq!(child.state, ProgramState::Blocked);
    assert_eq!(child.kind, Some(BlockedKind::Permission));
    assert_eq!(child.progress, Some(40));
    assert_eq!(child.title.as_deref(), Some("EU West"));
    assert_eq!(child.app, None);
    assert!(root.revision < child.revision);

    // The state is in the holder: a daemon that connects later gets it.
    drop(client);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.program_reports(), reports);

    // The wait has no reply until a program reports again.
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    let wait = client.send(Request::WaitProgramReports {
        after: reports.revision,
    });
    client.type_line("");
    let Reply::ProgramReportsChanged { revision } = client.receive(wait) else {
        panic!("not a change of the program reports");
    };
    assert!(revision > reports.revision);
    client.read_until("second-part");
    let root = client.program_reports().statuses.remove(0);
    assert_eq!(root.state, ProgramState::Done);
    assert_eq!(root.message, None);
}

#[test]
fn a_program_that_sends_the_query_gets_the_answer_on_its_input() {
    let (runtime, _holder) = start("query", QUERY);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.read_until_end(END_OF_QUERY);

    // The answer has no end of line. This line gives it to the shell.
    client.type_line("");

    client.read_until("reply-42");
}

#[test]
fn a_report_that_arrives_in_two_reads_is_kept_when_it_is_complete() {
    let (runtime, _holder) = start("parts", REPORT_IN_PARTS);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.read_until_end(END_OF_FIRST_PART);
    assert_eq!(client.program_reports().statuses, []);

    client.type_line("");
    client.read_until("complete");

    let statuses = client.program_reports().statuses;
    assert_eq!(
        status(&statuses, "").message.as_deref(),
        Some("Building docs")
    );
}

#[test]
fn the_prompt_marks_and_the_input_of_the_daemon_give_the_input_line_state() {
    let (runtime, _holder) = start("marks", PROMPT_MARKS);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.read_until_end(END_OF_PROMPT);

    let empty_line = client.program_reports().input_line;
    assert_eq!(empty_line.mark, Some(PromptMark::InputStart));
    assert!(!empty_line.input_after_mark);
    assert_eq!(empty_line.exit_code, None);

    client.type_line("first");
    let typed_line = client.program_reports().input_line;
    assert_eq!(typed_line.mark, Some(PromptMark::InputStart));
    assert!(typed_line.input_after_mark);
    assert!(typed_line.revision > empty_line.revision);

    client.type_line("second");
    client.read_until("next-prompt");
    let next_prompt = client.program_reports().input_line;
    assert_eq!(next_prompt.mark, Some(PromptMark::PromptStart));
    assert!(!next_prompt.input_after_mark);
    assert_eq!(next_prompt.exit_code, Some(3));
}

#[test]
fn the_holder_keeps_the_notifications_in_the_sequence_of_their_arrival() {
    let (runtime, _holder) = start("notify", NOTIFY);
    let mut client = Client::connect(runtime.path());
    client.read_until("notified");

    let notifications = client.program_reports().notifications;

    let content: Vec<(Option<&str>, &str)> = notifications
        .iter()
        .map(|Notification { title, body, .. }| (title.as_deref(), body.as_str()))
        .collect();
    assert_eq!(
        content,
        [
            (None, "Build finished"),
            (Some("Deploy"), "eu-west is ready"),
        ]
    );
    assert!(notifications[0].revision < notifications[1].revision);
}
