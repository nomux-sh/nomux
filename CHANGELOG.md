# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release process generates each release section from the commit messages
with [git-cliff](https://git-cliff.org) and `cliff.toml`.

## [Unreleased]

### Added

- Cargo workspace with the nine crates of the core, the release profile and the lint policy.
- `cargo xtask ci`, which runs the format, lint and test checks.
- `cargo deny` configuration for licenses, advisories and sources.
- CI that calls the reusable workflows of the organization.
