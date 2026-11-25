//! The input formats of the fuzz targets of the nomux core that are more
//! than bytes for one function.
//!
//! Each target is one file in `fuzz_targets`. `README.md` has the list.

pub mod forged_record;
pub mod journal_script;
pub mod terminal_script;
pub mod written_journal;
