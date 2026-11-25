//! The description of the machine and the tools of a benchmark run.

use std::fs;
use std::process::Command;

/// Prints the facts that a reader needs to compare two runs.
pub(crate) fn print(target: &str) {
    println!("- Host: {}", output_of("uname", &["-srm"]));
    println!("- CPU: {}", cpu_model());
    println!("- Compiler: {}", output_of("rustc", &["--version"]));
    println!("- Zig: {}", output_of("zig", &["version"]));
    println!("- Target of the probes: `{target}`");
}

fn output_of(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(|| "not found".to_owned(), |text| text.trim().to_owned())
}

/// Linux has the model in `/proc/cpuinfo`. macOS has it in `sysctl`.
fn cpu_model() -> String {
    let from_proc = fs::read_to_string("/proc/cpuinfo").ok().and_then(|text| {
        let line = text.lines().find(|line| line.starts_with("model name"))?;
        Some(line.split_once(':')?.1.trim().to_owned())
    });
    from_proc.unwrap_or_else(|| output_of("sysctl", &["-n", "machdep.cpu.brand_string"]))
}
