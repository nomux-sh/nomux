//! The candidates of the benchmark.

/// What a candidate can do, which sets the measurements that apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// No VT library: the reference for size and memory.
    Baseline,
    /// A parser without screen state.
    ParserOnly,
    /// A parser with screen state.
    Terminal,
}

/// One probe package of this workspace.
#[derive(Debug)]
pub(crate) struct Candidate {
    pub(crate) package: &'static str,
    pub(crate) kind: Kind,
    /// A program that the build of the candidate needs in addition to Cargo.
    pub(crate) build_tool: Option<&'static str>,
}

pub(crate) const CANDIDATES: [Candidate; 7] = [
    Candidate {
        package: "probe-baseline",
        kind: Kind::Baseline,
        build_tool: None,
    },
    Candidate {
        package: "probe-vte",
        kind: Kind::ParserOnly,
        build_tool: None,
    },
    Candidate {
        package: "probe-vt100",
        kind: Kind::Terminal,
        build_tool: None,
    },
    Candidate {
        package: "probe-vt100-psmux",
        kind: Kind::Terminal,
        build_tool: None,
    },
    Candidate {
        package: "probe-alacritty-terminal",
        kind: Kind::Terminal,
        build_tool: None,
    },
    Candidate {
        package: "probe-nomux-term",
        kind: Kind::Terminal,
        build_tool: None,
    },
    Candidate {
        package: "probe-libghostty-vt",
        kind: Kind::Terminal,
        build_tool: Some("zig"),
    },
];
