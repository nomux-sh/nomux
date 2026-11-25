//! The result of an `ext` command that did not fail.

use nomux_client::Extension;

use super::manifest_check::ManifestCheck;
use crate::exit_code;
use crate::report::Report;

#[derive(Debug)]
pub(crate) enum ExtensionReport {
    Listed(Vec<Extension>),
    Installed(Extension),
    Removed {
        id: String,
    },
    Enabled(Extension),
    Disabled(Extension),
    Called {
        id: String,
        method: String,
        /// The payload of the result, in the encoding of the method.
        payload: Vec<u8>,
    },
    ManifestChecked(ManifestCheck),
}

impl ExtensionReport {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::ManifestChecked(ManifestCheck::NotValid(_)) => exit_code::ERROR,
            _ => exit_code::SUCCESS,
        }
    }
}

impl From<ExtensionReport> for Report {
    fn from(report: ExtensionReport) -> Self {
        Self::Extension(report)
    }
}
