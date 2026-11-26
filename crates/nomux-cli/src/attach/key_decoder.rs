//! The keys of nomux in the bytes that the user types during an attach: a
//! prefix key, and then the key that says what to do.
//!
//! A terminal sends a key in one of three forms: its legacy byte (`Ctrl-a`
//! is 0x01), the form of the kitty keyboard protocol (`CSI 97 ; 5 u`), or
//! the form of `modifyOtherKeys` (`CSI 27 ; 5 ; 97 ~`). A program of the
//! session can turn on one of the two newer forms, and the terminal of the
//! user then sends it. The decoder knows the three forms. Each byte that is
//! not a key of nomux goes to the session as it came: the decoder never
//! encodes a key again.
//!
//! The text of a bracketed paste (`CSI 200 ~` to `CSI 201 ~`) is text: it
//! has no key of nomux.

use nomux_core::configuration::{PrefixKey, PrefixKeys};

const ESCAPE: u8 = 0x1b;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
/// The bit of `Ctrl` in the modifiers of a key, after 1 is subtracted.
const CONTROL: u32 = 4;
/// The bits of `Caps Lock` and `Num Lock`, which do not change a key.
const LOCKS: u32 = 64 | 128;
/// The kitty event type of a release.
const RELEASE: u32 = 3;
/// The kitty codes of the modifier keys alone: `Shift` to `Meta`, left and
/// right, and the lock keys.
const MODIFIER_KEYS: std::ops::RangeInclusive<u32> = 57358..=57452;

/// What the user asked for with a key after the prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Detach,
    ShowKeys,
}

/// Finds the keys of nomux in the bytes that the user types.
#[derive(Debug)]
pub(super) struct KeyDecoder {
    prefixes: PrefixKeys,
    /// The bytes of the prefix that the user pressed, while the next key
    /// decides what it means. They go to the session if that key is not a
    /// key of nomux.
    after_prefix: Option<Prefix>,
    /// The start of an escape sequence at the end of a read.
    partial: Vec<u8>,
    in_paste: bool,
    /// The keys that nomux took: their release events go nowhere.
    taken: Vec<u32>,
}

#[derive(Debug)]
struct Prefix {
    key: PrefixKey,
    pressed: Key,
    bytes: Vec<u8>,
    /// The release of the prefix came: it is in `bytes`.
    released: bool,
}

/// One key or one other unit of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    /// The Unicode code point of the key, or its legacy byte.
    code: u32,
    /// The modifiers without the lock keys, as kitty gives them minus 1.
    modifiers: u32,
    release: bool,
    /// The key came as its legacy byte.
    legacy: bool,
}

impl KeyDecoder {
    pub(super) fn new(prefixes: PrefixKeys) -> Self {
        Self {
            prefixes,
            after_prefix: None,
            partial: Vec::new(),
            in_paste: false,
            taken: Vec::new(),
        }
    }

    /// Appends to `forward` the bytes of `typed` that go to the session.
    /// Returns the command of the user, if `typed` has one: the bytes after
    /// it go nowhere.
    pub(super) fn decode(&mut self, typed: &[u8], forward: &mut Vec<u8>) -> Option<Command> {
        let mut input = std::mem::take(&mut self.partial);
        input.extend_from_slice(typed);
        let mut position = 0;
        while position < input.len() {
            let rest = &input[position..];
            let Some(length) = self.unit_length(rest) else {
                // The start of a sequence: the next read has its end.
                self.partial = rest.to_vec();
                break;
            };
            let unit = &rest[..length];
            position += length;
            if let Some(command) = self.take(unit, forward) {
                return Some(command);
            }
        }
        None
    }

    /// `true` if the decoder keeps the start of an escape sequence. When no
    /// more bytes come soon, [`flush`](Self::flush) sends it.
    pub(super) fn has_partial(&self) -> bool {
        !self.partial.is_empty()
    }

    /// Sends the start of an escape sequence that did not end: it was not a
    /// key of nomux.
    pub(super) fn flush(&mut self, forward: &mut Vec<u8>) {
        let partial = std::mem::take(&mut self.partial);
        self.forward(&partial, forward);
    }

    /// The length of the unit at the start of `bytes`: an escape sequence,
    /// the text of a paste up to its end, or one byte. `None` if `bytes` is
    /// the start of a unit that is not complete.
    fn unit_length(&self, bytes: &[u8]) -> Option<usize> {
        if self.in_paste {
            return match find(bytes, PASTE_END) {
                Some(start) if start > 0 => Some(start),
                Some(_) => Some(PASTE_END.len()),
                // The end marker can start in this read.
                None => match partial_suffix(bytes, PASTE_END) {
                    0 => Some(bytes.len()),
                    suffix if suffix == bytes.len() => None,
                    suffix => Some(bytes.len() - suffix),
                },
            };
        }
        match bytes {
            [ESCAPE, b'[', parameters @ ..] => {
                let end = parameters
                    .iter()
                    .position(|byte| (0x40..=0x7e).contains(byte))?;
                Some(end + 3)
            }
            // A lone Escape key goes at once.
            _ => Some(1),
        }
    }

    /// Handles one unit. Returns the command of the user.
    fn take(&mut self, unit: &[u8], forward: &mut Vec<u8>) -> Option<Command> {
        if self.in_paste || self.prefixes.keys().is_empty() {
            self.in_paste &= unit != PASTE_END;
            forward.extend_from_slice(unit);
            return None;
        }
        if unit == PASTE_START {
            self.in_paste = true;
            self.forward(unit, forward);
            return None;
        }
        let key = Key::of(unit);
        let Some(mut prefix) = self.after_prefix.take() else {
            self.take_without_prefix(key, unit, forward);
            return None;
        };
        let Some(key) = key.filter(|key| !key.release && !MODIFIER_KEYS.contains(&key.code)) else {
            // A release or a modifier key alone does not decide what the
            // prefix means: it goes with the prefix.
            prefix.released |= key.is_some_and(|key| key.release && key.is_same(prefix.pressed));
            prefix.bytes.extend_from_slice(unit);
            self.after_prefix = Some(prefix);
            return None;
        };
        let command = match command_after(prefix.key, key) {
            AfterPrefix::Detach => Some(Command::Detach),
            AfterPrefix::ShowKeys => Some(Command::ShowKeys),
            AfterPrefix::SendPrefix => {
                forward.extend_from_slice(&prefix.bytes);
                None
            }
            AfterPrefix::Other => {
                forward.extend_from_slice(&prefix.bytes);
                forward.extend_from_slice(unit);
                return None;
            }
        };
        // The key after the prefix went nowhere, and the prefix too for a
        // command: their releases go nowhere either.
        self.remember_taken(key);
        if command.is_some() && !prefix.released {
            self.remember_taken(prefix.pressed);
        }
        command
    }

    /// Handles one unit when no prefix waits for the next key.
    fn take_without_prefix(&mut self, key: Option<Key>, unit: &[u8], forward: &mut Vec<u8>) {
        let Some(key) = key else {
            forward.extend_from_slice(unit);
            return;
        };
        if key.release {
            match self.taken.iter().position(|code| *code == key.code) {
                Some(index) => {
                    self.taken.swap_remove(index);
                }
                None => forward.extend_from_slice(unit),
            }
            return;
        }
        match self.prefix_of(key) {
            Some(prefix_key) => {
                self.after_prefix = Some(Prefix {
                    key: prefix_key,
                    pressed: key,
                    bytes: unit.to_vec(),
                    released: false,
                });
            }
            None => forward.extend_from_slice(unit),
        }
    }

    /// Sends `bytes`, with the prefix before them if the user pressed it.
    fn forward(&mut self, bytes: &[u8], forward: &mut Vec<u8>) {
        if let Some(prefix) = self.after_prefix.take() {
            forward.extend_from_slice(&prefix.bytes);
        }
        forward.extend_from_slice(bytes);
    }

    fn prefix_of(&self, key: Key) -> Option<PrefixKey> {
        self.prefixes
            .keys()
            .iter()
            .copied()
            .find(|prefix| key.is_control(prefix.byte()))
    }

    /// A key in the newer forms has a release event later: it goes nowhere.
    fn remember_taken(&mut self, key: Key) {
        const MAX_TAKEN: usize = 8;
        if key.legacy {
            return;
        }
        if self.taken.len() == MAX_TAKEN {
            self.taken.remove(0);
        }
        self.taken.push(key.code);
    }
}

/// What `key` after `prefix` means.
fn command_after(prefix: PrefixKey, key: Key) -> AfterPrefix {
    if key.is_plain('d') || key.is_control(0x04) {
        AfterPrefix::Detach
    } else if key.is_plain('?') {
        AfterPrefix::ShowKeys
    } else if key.is_control(prefix.byte())
        || prefix.letter().is_some_and(|letter| key.is_plain(letter))
    {
        AfterPrefix::SendPrefix
    } else {
        AfterPrefix::Other
    }
}

/// What a key after the prefix means.
enum AfterPrefix {
    Detach,
    ShowKeys,
    SendPrefix,
    Other,
}

impl Key {
    /// The key of `unit`, if it is a key in one of the three forms.
    fn of(unit: &[u8]) -> Option<Self> {
        match unit {
            [byte] => Some(Self {
                code: u32::from(*byte),
                modifiers: 0,
                release: false,
                legacy: true,
            }),
            [ESCAPE, b'[', sequence @ .., b'u'] => Self::of_kitty(sequence),
            [ESCAPE, b'[', sequence @ .., b'~'] => Self::of_modify_other_keys(sequence),
            _ => None,
        }
    }

    /// `code[:alternates][;modifiers[:event][;text]]`.
    fn of_kitty(sequence: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(sequence).ok()?;
        let mut fields = text.split(';');
        let code = fields.next()?.split(':').next()?.parse().ok()?;
        let mut modifier_field = fields.next().unwrap_or("1").split(':');
        let modifiers = modifier_field.next().filter(|text| !text.is_empty());
        let modifiers: u32 = modifiers.map_or(Some(1), |text| text.parse().ok())?;
        let event: u32 = modifier_field
            .next()
            .map_or(Some(1), |text| text.parse().ok())?;
        Some(Self {
            code,
            modifiers: modifiers.saturating_sub(1) & !LOCKS,
            release: event == RELEASE,
            legacy: false,
        })
    }

    /// `27;modifiers;code`.
    fn of_modify_other_keys(sequence: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(sequence).ok()?;
        let mut fields = text.split(';');
        if fields.next()? != "27" {
            return None;
        }
        let modifiers: u32 = fields.next()?.parse().ok()?;
        let code = fields.next()?.parse().ok()?;
        Some(Self {
            code,
            modifiers: modifiers.saturating_sub(1) & !LOCKS,
            release: false,
            legacy: false,
        })
    }

    /// `true` if the key is the control key with the legacy byte `byte`.
    fn is_control(self, byte: u8) -> bool {
        if self.legacy {
            return self.code == u32::from(byte);
        }
        let codes: &[u32] = match byte {
            0x00 => &[32, 64],
            0x01..=0x1a => &[u32::from(byte) + 96],
            0x1c..=0x1f => &[u32::from(byte) + 64],
            _ => &[],
        };
        self.modifiers == CONTROL && codes.contains(&self.code)
    }

    /// `true` if `self` is the release of `pressed`, or the same key.
    fn is_same(self, pressed: Self) -> bool {
        self.code == pressed.code
    }

    /// `true` if the key is `character` without `Ctrl`, `Alt` or `Super`.
    fn is_plain(self, character: char) -> bool {
        const SHIFT: u32 = 1;
        self.code == u32::from(character) && self.modifiers & !SHIFT == 0
    }
}

/// The position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The length of the longest end of `bytes` that is a start of `marker`.
fn partial_suffix(bytes: &[u8], marker: &[u8]) -> usize {
    (1..marker.len().min(bytes.len() + 1))
        .rev()
        .find(|&length| bytes.ends_with(&marker[..length]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gives each read to one decoder with the default prefixes. Returns
    /// the bytes for the session and the command, if any.
    fn typed(reads: &[&[u8]]) -> (Vec<u8>, Option<Command>) {
        typed_with(PrefixKeys::default(), reads)
    }

    fn typed_with(prefixes: PrefixKeys, reads: &[&[u8]]) -> (Vec<u8>, Option<Command>) {
        let mut decoder = KeyDecoder::new(prefixes);
        let mut forward = Vec::new();
        for read in reads {
            if let Some(command) = decoder.decode(read, &mut forward) {
                return (forward, Some(command));
            }
        }
        decoder.flush(&mut forward);
        (forward, None)
    }

    #[test]
    fn each_prefix_and_d_detaches_in_the_legacy_form() {
        assert_eq!(
            typed(&[b"ls\r\x01d"]),
            (b"ls\r".to_vec(), Some(Command::Detach))
        );
        assert_eq!(
            typed(&[b"ls\r\x1cd"]),
            (b"ls\r".to_vec(), Some(Command::Detach))
        );
        // Ctrl-d after the prefix, with Ctrl still held.
        assert_eq!(typed(&[b"\x01\x04"]), (Vec::new(), Some(Command::Detach)));
        assert_eq!(typed(&[b"\x01?"]), (Vec::new(), Some(Command::ShowKeys)));
    }

    #[test]
    fn a_prefix_that_comes_in_two_reads_still_detaches() {
        assert_eq!(
            typed(&[b"x\x01", b"dmore"]),
            (b"x".to_vec(), Some(Command::Detach))
        );
        // A kitty sequence that the read divides.
        assert_eq!(
            typed(&[b"x\x1b[97;", b"5u", b"\x1b[100u"]),
            (b"x".to_vec(), Some(Command::Detach))
        );
    }

    #[test]
    fn the_prefix_and_d_detach_in_the_kitty_form() {
        // Flag 1: the prefix is a sequence, d is its byte.
        assert_eq!(
            typed(&[b"\x1b[97;5ud"]),
            (Vec::new(), Some(Command::Detach))
        );
        assert_eq!(
            typed(&[b"\x1b[92;5ud"]),
            (Vec::new(), Some(Command::Detach))
        );
        // Flag 8: d is a sequence too. Caps Lock does not change the key.
        assert_eq!(
            typed(&[b"\x1b[97;69u\x1b[100;65u"]),
            (Vec::new(), Some(Command::Detach))
        );
        // Flags 2 and 8: the release of the prefix and the Ctrl key alone
        // between the two keys do not count as the next key.
        assert_eq!(
            typed(&[b"\x1b[57442;5u\x1b[97;5u\x1b[97;5:3u\x1b[57442;1:3u\x1b[100;1u"]),
            (b"\x1b[57442;5u".to_vec(), Some(Command::Detach))
        );
        // modifyOtherKeys.
        assert_eq!(
            typed(&[b"\x1b[27;5;97~d"]),
            (Vec::new(), Some(Command::Detach))
        );
    }

    #[test]
    fn the_release_of_a_key_that_nomux_took_goes_nowhere() {
        // Ctrl-a Ctrl-a with release events: one press and one release of
        // Ctrl-a reach the program.
        assert_eq!(
            typed(&[b"\x1b[97;5u\x1b[97;5:3u\x1b[97;5u\x1b[97;5:3u"]),
            (b"\x1b[97;5u\x1b[97;5:3u".to_vec(), None)
        );
    }

    #[test]
    fn the_prefix_reaches_the_session_when_no_key_of_nomux_follows() {
        // The prefix two times, or Ctrl-a and a, is one prefix.
        assert_eq!(typed(&[b"a\x01\x01d"]), (b"a\x01d".to_vec(), None));
        assert_eq!(typed(&[b"\x01a"]), (b"\x01".to_vec(), None));
        assert_eq!(typed(&[b"\x1c\x1c"]), (b"\x1c".to_vec(), None));
        // A different key after the prefix: the two keys, as they came.
        assert_eq!(typed(&[b"\x01", b"\x0b"]), (b"\x01\x0b".to_vec(), None));
        assert_eq!(
            typed(&[b"\x1b[97;5u\x1b[107;5u"]),
            (b"\x1b[97;5u\x1b[107;5u".to_vec(), None)
        );
        // A d without the prefix, and keys with Alt, are normal keys.
        assert_eq!(typed(&[b"dd\x1b[97;7u"]), (b"dd\x1b[97;7u".to_vec(), None));
        // A lone Escape goes at once.
        let mut decoder = KeyDecoder::new(PrefixKeys::default());
        let mut forward = Vec::new();
        assert_eq!(decoder.decode(b"\x1b", &mut forward), None);
        assert_eq!(forward, b"\x1b");
    }

    #[test]
    fn a_paste_has_no_keys_of_nomux() {
        let pasted: &[u8] = b"\x1b[200~a\x01d\x1b[97;5ud\x1b[201~";
        assert_eq!(typed(&[pasted]), (pasted.to_vec(), None));
        // The end marker in two reads ends the paste.
        assert_eq!(
            typed(&[b"\x1b[200~\x01d\x1b[20", b"1~\x01d"]),
            (b"\x1b[200~\x01d\x1b[201~".to_vec(), Some(Command::Detach))
        );
    }

    #[test]
    fn the_setting_chooses_the_prefixes() {
        let control_b = "C-b".parse().unwrap();
        assert_eq!(
            typed_with(control_b, &[b"\x01d\x02d"]),
            (b"\x01d".to_vec(), Some(Command::Detach))
        );
        let none = "none".parse().unwrap();
        assert_eq!(
            typed_with(none, &[b"\x01d\x1cd"]),
            (b"\x01d\x1cd".to_vec(), None)
        );
    }
}
