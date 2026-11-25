# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

[git-cliff](https://git-cliff.org) writes this file from the commit subjects.
To see the changes that are not in a version, run `git cliff --unreleased`.

## [0.2.1] - 2026-11-25

### Fixed

- Build the musl release binaries with a C compiler for musl

### Documentation

- List the release build workflow in the agent rules
## [0.2.0] - 2026-11-25

### Added

- Add the journal format and the session metadata
- Write the journal in the holder and add its read request
- Add the extension host library
- Read the hosts of the global configuration file
- Add an SSH client to the transport layer
- Let a new daemon replace the daemon that runs
- Add remote hosts, the host commands and --host

### Documentation

- Describe the journal and the session state directory
- Describe the extension host
- Describe remote hosts
## [0.1.1] - 2026-11-25

### Fixed

- Refuse a terminal of fewer than 2 columns
- Keep the columns and the wrap of the redraw sequence correct
- Wrap the position arithmetic of the ring buffer

### Documentation

- List the terminal features and the known gaps
- Update the known gaps of the terminal
## [0.1.0] - 2026-11-25

### Added

- Add the install script for the release archives
- Add the holder protocol crate
- Run a session in the holder
- Add the terminal state API to nomux-term
- Add the Unix socket transport of the control channel
- Add the client library of the daemon protocol
- Serve sessions from the daemon
- Add the first CLI commands for sessions and the daemon
- Find the incomplete sequence at the end of the output
- Keep the screen state in the holder and add the redraw request
- Parse program status, prompt marks and notifications
- Keep the program reports in the holder and add their requests
- Add the framing of the terminal channels to the transport
- Attach clients to the terminal of a session in the daemon
- Add attach and the terminal requests to the client library
- Add the attach and detach commands and new --attach

### Fixed

- Accept the command of a new session after --

### Changed

- Reduce the shared version output to one small module
- Read the program name with the process start time

### Documentation

- Add architecture overview, directory notes and changelog
- Describe the current state and link the roadmap
- Describe the install steps and the release workflow
- Describe the holder and its protocol
- Describe nomux-term and the benchmark workspace
- Describe the daemon and the CLI
- Describe the screen state and the redraw request of the holder
- Describe the program reports of the holder
- Describe attach and add a quick start with it
