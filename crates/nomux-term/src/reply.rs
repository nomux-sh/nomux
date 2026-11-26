//! The answers of the terminal to the queries of a program: the bytes that
//! the terminal sends to the input of the program.
//!
//! The VT library answers most queries with an event. This module keeps
//! those events in order, gives the answers that need the colors, and
//! changes the identity of the library into the identity of nomux:
//!
//! | Query | Answer |
//! |---|---|
//! | Primary device attributes (DA1) | [`PRIMARY_DEVICE_ATTRIBUTES`] |
//! | Secondary device attributes (DA2) | `CSI > 1 ; Pv ; 0 c`, see [`secondary_device_attributes`] |
//! | Terminal name and version (XTVERSION) | `DCS > \| nomux(version) ST` |
//! | Setting request (DECRQSS) | `DCS 0 $ r ST`: the setting is not known |
//! | Color (OSC 4, 10, 11, 12) | The color that the program set, else the color of the [`Palette`] |
//! | Text area size in pixels | `CSI 4 ; 0 ; 0 t`: nomux knows no pixel size |
//! | The other queries | The answer of the library from the state of the terminal |

use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::Rgb as LibraryRgb;

use crate::palette::{Palette, Rgb};

/// The primary device attributes of nomux: a VT220 (62) with ANSI colors
/// (22). It does not say sixel (4) or `ReGIS` (3): nomux keeps no picture.
pub(crate) const PRIMARY_DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";

/// The answer to a setting request (DECRQSS): nomux does not report a
/// setting.
pub(crate) const SETTING_NOT_KNOWN: &[u8] = b"\x1bP0$r\x1b\\";

/// The answers that wait for the caller. A program that sends queries and
/// does not read the answers must not make them grow without a limit.
const MAX_REPLY_BYTES: usize = 4096;

/// The answers of the library that one call of the parser can make before
/// the terminal reads them.
const MAX_LIBRARY_EVENTS: usize = 256;

/// The primary device attributes of the VT library: a VT102.
const LIBRARY_PRIMARY_DEVICE_ATTRIBUTES: &str = "\x1b[?6c";
/// The start of the secondary device attributes of the VT library.
const LIBRARY_SECONDARY_DEVICE_ATTRIBUTES: &str = "\x1b[>0;";

/// An answer of the library that the terminal did not read yet.
enum LibraryReply {
    Text(String),
    Color(usize, Arc<dyn Fn(LibraryRgb) -> String + Send + Sync>),
    PixelSize(Arc<dyn Fn(WindowSize) -> String + Send + Sync>),
}

/// The event listener of the VT library. The library gives the listener
/// only `&self`, thus the listener and the terminal share the events.
#[derive(Clone, Default)]
pub(crate) struct ReplyListener {
    events: Arc<Mutex<Vec<LibraryReply>>>,
}

impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        let reply = match event {
            Event::PtyWrite(text) => LibraryReply::Text(text),
            Event::ColorRequest(index, format) => LibraryReply::Color(index, format),
            Event::TextAreaSizeRequest(format) => LibraryReply::PixelSize(format),
            // A program must not read the clipboard of the user (OSC 52).
            // The other events change nothing that a program reads.
            _ => return,
        };
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() < MAX_LIBRARY_EVENTS {
            events.push(reply);
        }
    }
}

/// The answers in the order of the queries.
pub(crate) struct Replies {
    listener: ReplyListener,
    bytes: Vec<u8>,
    pub(crate) palette: Palette,
}

impl Replies {
    pub(crate) fn new(listener: ReplyListener) -> Self {
        Self {
            listener,
            bytes: Vec::new(),
            palette: Palette::default(),
        }
    }

    /// Adds the answers that the library made since the last call.
    /// `colors` has the colors that the program set.
    pub(crate) fn collect(&mut self, colors: &Colors) {
        let events = {
            let mut events = self
                .listener
                .events
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if events.is_empty() {
                return;
            }
            std::mem::take(&mut *events)
        };
        for event in events {
            let text = match event {
                LibraryReply::Text(text) => nomux_identity(text),
                LibraryReply::Color(index, format) => {
                    let set = colors[index].map(|color| Rgb::new(color.r, color.g, color.b));
                    let color = set.unwrap_or_else(|| self.palette.color(index));
                    format(LibraryRgb {
                        r: color.red,
                        g: color.green,
                        b: color.blue,
                    })
                }
                LibraryReply::PixelSize(format) => format(WindowSize {
                    num_lines: 0,
                    num_cols: 0,
                    cell_width: 0,
                    cell_height: 0,
                }),
            };
            self.push(text.as_bytes());
        }
    }

    /// Adds an answer that this crate makes, after the answers of the
    /// library.
    pub(crate) fn push(&mut self, answer: &[u8]) {
        if self.bytes.len() + answer.len() <= MAX_REPLY_BYTES {
            self.bytes.extend_from_slice(answer);
        }
    }

    pub(crate) fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

/// Changes the device attributes of the library into those of nomux.
fn nomux_identity(text: String) -> String {
    if text == LIBRARY_PRIMARY_DEVICE_ATTRIBUTES {
        return String::from_utf8_lossy(PRIMARY_DEVICE_ATTRIBUTES).into_owned();
    }
    if text.starts_with(LIBRARY_SECONDARY_DEVICE_ATTRIBUTES) && text.ends_with('c') {
        return secondary_device_attributes();
    }
    text
}

/// `CSI > 1 ; Pv ; 0 c`: a VT220 (1), the version of nomux as one number
/// (`Pv`, major × 10000 + minor × 100 + patch), no ROM cartridge (0).
pub(crate) fn secondary_device_attributes() -> String {
    let mut parts = env!("CARGO_PKG_VERSION")
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0).min(99);
    let (major, minor, patch) = (next(), next(), next());
    let version = major * 10_000 + minor * 100 + patch;
    format!("\x1b[>1;{version};0c")
}

/// `DCS > | nomux(version) ST`.
pub(crate) fn terminal_version() -> String {
    format!("\x1bP>|nomux({})\x1b\\", env!("CARGO_PKG_VERSION"))
}
