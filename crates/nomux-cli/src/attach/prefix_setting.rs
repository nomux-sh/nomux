//! The prefix keys of an attach, from the first place that has them: the
//! `--prefix` option, the `NOMUX_PREFIX` variable, `prefix` in the
//! `[attach]` table of the configuration file of this computer, or the
//! default `Ctrl-a` and `Ctrl-\`.

use std::env;

use clap::Args;
use nomux_client::ClientError;
use nomux_core::configuration::{ConfigurationFile, PrefixKeys, default_configuration_path};

const VARIABLE: &str = "NOMUX_PREFIX";

/// The option of the commands that attach.
#[derive(Debug, Default, Args)]
pub(crate) struct PrefixOption {
    /// A prefix key of this attach, for example C-a, C-b or C-\. Give the option two times for
    /// two keys. none: no key detaches [default: the variable `NOMUX_PREFIX`, then `prefix` in
    /// `[attach]` of the configuration file, then C-a and C-\]
    #[arg(long = "prefix", value_name = "KEY")]
    keys: Vec<String>,
}

impl PrefixOption {
    /// The prefix keys of the attach.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the setting that applies is not
    /// valid: the message names its place.
    pub(crate) fn prefix_keys(&self) -> Result<PrefixKeys, ClientError> {
        if !self.keys.is_empty() {
            return PrefixKeys::from_texts(self.keys.iter().map(String::as_str))
                .map_err(|error| ClientError::Failed(format!("--prefix: {error}")));
        }
        if let Some(text) = env::var_os(VARIABLE).filter(|text| !text.is_empty()) {
            return text
                .to_str()
                .ok_or_else(|| "it is not valid Unicode".to_owned())
                .and_then(str::parse)
                .map_err(|error| ClientError::Failed(format!("{VARIABLE}: {error}")));
        }
        let Some(path) = default_configuration_path() else {
            return Ok(PrefixKeys::default());
        };
        let configured = ConfigurationFile::load(&path)?.attach_prefix()?;
        Ok(configured.unwrap_or_default())
    }
}
