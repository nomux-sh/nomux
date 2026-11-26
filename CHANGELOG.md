# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

[git-cliff](https://git-cliff.org) writes this file from the commit subjects.
To see the changes that are not in a version, run `git cliff --unreleased`.

## [0.5.0] - 2026-11-26

### Added

- Keep groups of projects in the daemon and the CLI
- Find the project of a session by its resolved directory
- Keep the rows above the screen as history in nomux-term
- Choose the prefix keys of an attach and read the kitty key forms
- Give sessions names and find a session by its name
- Send keys to a session, follow its output and end all sessions
- Hibernate idle agent sessions and limit their memory

### Fixed

- Erase the first row for ED 1 from the second row
- Show the configuration file of the daemon and its macOS path
- Fit each table of the CLI to a terminal of 60 columns
- Change the directory of a pooled shell before its first prompt
- Wait for a replacement daemon when the earlier daemon closes
- Correct the documentation link, CI triggers and Dependabot rules
- Keep the format of a settings file at a hooks install
- Leave the message and location of a holder panic
## [0.4.0] - 2026-11-26

### Added

- Show the memory of the processes of each session
- Give each session its identifier, the daemon socket and a token
- Add the agent status engine
- Keep the status of each session and serve its events
- Add the status, agent explain and events commands
- Add hook events, the adapter source and extension events
- Let a pooled holder get its session with a new request
- Keep a pool of holders with a started shell for new sessions
- Keep the last agent run of an ended session

### Fixed

- Refuse --cmd together with -- in nomux new
- Fit the tables of the text output to the terminal width
- Keep the socket paths under the limit for a long home directory
- Give each session a terminal type and a UTF-8 character set

### Documentation

- Describe the agent status in the overview documents
- Describe the agent adapters and the hook commands
## [0.3.0] - 2026-11-26

### Added

- Let the extension host inspect and remove a package
- Serve the Ext service and run extensions in the daemon
- Add the extension requests to the client library
- Add the ext commands to the CLI
- Read the restore policy and the retention time from config.toml
- Restore sessions on demand and serve their journal

### Fixed

- Keep the size of a resize when a program writes an old size

### Documentation

- Describe the extensions in the daemon and the ext commands
- Describe the journal and OSC record fuzz targets
- Describe dormant sessions, the restore and the daemon at login
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
