//! The seccomp program of a sandbox without network: a classic BPF program
//! that the kernel runs for each system call of the command.
//!
//! - A system call of a different architecture ends the process: a 32-bit
//!   program on a 64-bit kernel has different numbers, which the program
//!   does not check.
//! - On x86-64, each system call of the x32 interface fails.
//! - `io_uring_setup` fails: a ring can open sockets without `socket`.
//! - `socket` fails for the internet protocols, raw packets and virtual
//!   machine sockets. A Unix socket and a netlink socket stay possible.
//!
//! A refused call fails with `EACCES`, as a Landlock denial does.

use libc::sock_filter;

/// The audit architecture of this build: `AUDIT_ARCH_X86_64` or
/// `AUDIT_ARCH_AARCH64` of `linux/audit.h`. `None` for an architecture
/// whose program nobody tested: such a host has no `network_off`.
#[cfg(target_arch = "x86_64")]
pub(super) const ARCHITECTURE: Option<u32> = Some(0xC000_003E);
#[cfg(target_arch = "aarch64")]
pub(super) const ARCHITECTURE: Option<u32> = Some(0xC000_00B7);
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(super) const ARCHITECTURE: Option<u32> = None;

/// `__X32_SYSCALL_BIT`: each system call number of the x32 interface has it.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: Option<u32> = Some(0x4000_0000);
#[cfg(not(target_arch = "x86_64"))]
const X32_SYSCALL_BIT: Option<u32> = None;

/// The offsets in `struct seccomp_data`: the number, the architecture, and
/// the low 32 bits of the first argument on a little-endian host.
const NUMBER: u32 = 0;
const ARCH: u32 = 4;
const FIRST_ARGUMENT: u32 = 16;

const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7fff_0000;

const LOAD_WORD: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
const JUMP_EQUAL: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
const JUMP_GREATER_EQUAL: u16 = 0x35; // BPF_JMP | BPF_JGE | BPF_K
const RETURN: u16 = 0x06; // BPF_RET | BPF_K

/// Where a jump goes. The three returns are the last instructions.
#[derive(Clone, Copy)]
enum Target {
    Next,
    Allow,
    Deny,
    Kill,
}

enum Step {
    Load(u32),
    Jump(u16, u32, Target, Target),
}

/// The program of a plan without network. Empty on an architecture without
/// a program: `prepare` refuses such a plan before.
pub(super) fn network_off() -> Vec<sock_filter> {
    let Some(architecture) = ARCHITECTURE else {
        return Vec::new();
    };
    let deny_if = |value| Step::Jump(JUMP_EQUAL, value, Target::Deny, Target::Next);
    let mut steps = vec![
        Step::Load(ARCH),
        Step::Jump(JUMP_EQUAL, architecture, Target::Next, Target::Kill),
        Step::Load(NUMBER),
    ];
    if let Some(bit) = X32_SYSCALL_BIT {
        steps.push(Step::Jump(
            JUMP_GREATER_EQUAL,
            bit,
            Target::Deny,
            Target::Next,
        ));
    }
    steps.push(deny_if(number(libc::SYS_io_uring_setup)));
    steps.push(Step::Jump(
        JUMP_EQUAL,
        number(libc::SYS_socket),
        Target::Next,
        Target::Allow,
    ));
    steps.push(Step::Load(FIRST_ARGUMENT));
    for family in [
        libc::AF_INET,
        libc::AF_INET6,
        libc::AF_PACKET,
        libc::AF_VSOCK,
    ] {
        steps.push(deny_if(family.unsigned_abs()));
    }
    assemble(&steps)
}

fn number(system_call: libc::c_long) -> u32 {
    u32::try_from(system_call).unwrap_or(u32::MAX)
}

/// The instructions of `steps`, then the returns: allow, deny, end the
/// process. The last step falls through to allow.
fn assemble(steps: &[Step]) -> Vec<sock_filter> {
    let allow = steps.len();
    let mut program: Vec<sock_filter> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let offset = |target| {
                let destination = match target {
                    Target::Next => index + 1,
                    Target::Allow => allow,
                    Target::Deny => allow + 1,
                    Target::Kill => allow + 2,
                };
                // A program of fewer than 256 instructions.
                u8::try_from(destination - index - 1).unwrap_or(u8::MAX)
            };
            match *step {
                Step::Load(field) => instruction(LOAD_WORD, 0, 0, field),
                Step::Jump(code, value, if_true, if_false) => {
                    instruction(code, offset(if_true), offset(if_false), value)
                }
            }
        })
        .collect();
    program.extend([
        instruction(RETURN, 0, 0, RET_ALLOW),
        instruction(RETURN, 0, 0, RET_ERRNO | libc::EACCES.unsigned_abs()),
        instruction(RETURN, 0, 0, RET_KILL_PROCESS),
    ]);
    program
}

fn instruction(code: u16, jt: u8, jf: u8, k: u32) -> sock_filter {
    sock_filter { code, jt, jf, k }
}
