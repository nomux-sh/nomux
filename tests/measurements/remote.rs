//! Measures a remote host with the real binaries and an SSH server on this
//! computer: the time of the first `nomux host add` with the upload of the
//! binaries, the time of a later command, and the time from a key to its
//! echo on the screen of `nomux --host <name> attach`, without and with a
//! delay on the connection.
//!
//! `tests/README.md` says how to run it. The numbers of a debug build have
//! no meaning.

use std::thread;
use std::time::{Duration, Instant};

use nomux_end_to_end::{Impairment, RemoteHost, TerminalClient, key_to_echo, latency_row};

/// The number of commands of the measurement of a later command.
const COMMANDS: usize = 20;
/// The number of keys of a measurement without a delay.
const KEYS: usize = 2000;
/// Each key waits for one round trip of the connection. With a delay, a
/// smaller number of keys keeps the measurement short.
const KEYS_WITH_DELAY: usize = 300;
/// 50 ms in each direction: a round trip of 100 ms.
const DELAY: Duration = Duration::from_millis(50);
/// The model of 2% lost packets: 2% of the chunks of bytes wait for the
/// shortest time after which a TCP connection sends a packet again.
const STALL_PROBABILITY: f64 = 0.02;
const STALL: Duration = Duration::from_millis(200);
/// Longer than a round trip with a stall.
const QUIET_TIME: Duration = Duration::from_secs(1);

/// Writes as fast as the holder reads.
const FLOOD_WITHOUT_LIMIT: &str = "yes 'a line of the flood of output'";

fn main() {
    if !installation_times() {
        return;
    }
    println!();
    println!("| Key to echo on `nomux --host vps attach` | Median | 99th percentile | Maximum |");
    println!("|---|---|---|---|");
    let delayed = Impairment {
        delay: DELAY,
        ..Impairment::default()
    };
    let delayed_with_stalls = Impairment {
        stall_probability: STALL_PROBABILITY,
        stall: STALL,
        ..delayed
    };
    for (name, impairment, keys) in [
        (
            "SSH to this computer, no delay",
            Impairment::default(),
            KEYS,
        ),
        ("Round trip of 100 ms", delayed, KEYS_WITH_DELAY),
        (
            "Round trip of 100 ms, and 2% of the chunks wait 200 ms more",
            delayed_with_stalls,
            KEYS_WITH_DELAY,
        ),
    ] {
        let Some(remote) = RemoteHost::start_with(&[], impairment) else {
            return;
        };
        remote.install_binaries();
        remote.add_as_vps();
        let mut typist = attach_to_new_session(&remote, "cat");
        typist.type_keys(b"ready\r");
        typist.wait_for_text("ready");
        wait_for_quiet(&mut typist);
        latency_row(name, &key_to_echo(&mut typist, None, keys));

        let mut flood = attach_to_new_session(&remote, FLOOD_WITHOUT_LIMIT);
        flood.wait_for_rows("the flood", |rows| !rows[0].is_empty());
        wait_for_quiet(&mut typist);
        latency_row(
            &format!("{name}, flood in a second attached session"),
            &key_to_echo(&mut typist, Some(&mut flood), keys),
        );
    }
}

/// Measures `host add` on a host without the binaries and a later command.
/// Returns `false` if this computer has no SSH server.
fn installation_times() -> bool {
    let Some(mut remote) = RemoteHost::start_with(&[], Impairment::default()) else {
        return false;
    };
    let releases = remote.release_fixture();
    remote
        .client
        .set_variable("NOMUX_RELEASE_DOWNLOAD_URL", releases);

    println!("| Measurement | Result |");
    println!("|---|---|");
    let started = Instant::now();
    let added = remote
        .nomux_ok(&[
            "host",
            "add",
            &remote.ssh_target(),
            "--name",
            "vps",
            "--json",
        ])
        .json();
    assert_eq!(added["host"]["installed"], true);
    println!(
        "| First `nomux host add`: connection, archive, upload, checksum, start of the daemon | {:.0} ms |",
        milliseconds(started.elapsed())
    );

    let mut times: Vec<Duration> = (0..COMMANDS)
        .map(|_| {
            let started = Instant::now();
            let _ = remote.nomux_ok(&["--host", "vps", "ls", "--quiet"]);
            started.elapsed()
        })
        .collect();
    times.sort_unstable();
    println!(
        "| Later command `nomux --host vps ls`: median, maximum of {COMMANDS} | {:.0} ms, {:.0} ms |",
        milliseconds(times[COMMANDS / 2]),
        milliseconds(times[COMMANDS - 1])
    );
    let started = Instant::now();
    let _ = remote.client.nomux_ok(&["ls", "--quiet"]);
    let first_local = started.elapsed();
    let started = Instant::now();
    let _ = remote.client.nomux_ok(&["ls", "--quiet"]);
    println!(
        "| For comparison, `nomux ls` on the local host: with the start of the daemon, later | {:.0} ms, {:.0} ms |",
        milliseconds(first_local),
        milliseconds(started.elapsed())
    );
    true
}

/// Waits until the output of the last key arrived. A measurement that
/// starts before that takes late output for the echo of its first key, and
/// each key after it is one echo behind.
fn wait_for_quiet(client: &mut TerminalClient) {
    thread::sleep(QUIET_TIME);
    client.read_pending_output();
}

fn attach_to_new_session(remote: &RemoteHost, command: &str) -> TerminalClient {
    let created = remote.nomux_ok(&["--host", "vps", "new", "--quiet", "--", "sh", "-c", command]);
    remote.nomux_in_terminal(&["--host", "vps", "attach", created.stdout.trim()])
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
