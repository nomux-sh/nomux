//! Arbitrary bytes go into the decoder of the program reports of a holder.
//!
//! The daemon reads the program reports from shared memory that a holder
//! wrote, and it does not trust the holder. `ProgramReports::decode` must
//! not panic and must not allocate more than the limit. Program reports that
//! decode must also be the same after `encode` and a second `decode`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_holder_protocol::program_reports::ProgramReports;

fuzz_target!(|input: &[u8]| {
    let Ok(reports) = ProgramReports::decode(input) else {
        return;
    };
    let mut encoded = Vec::new();
    reports.encode(&mut encoded);
    assert_eq!(ProgramReports::decode(&encoded), Ok(reports));
});
