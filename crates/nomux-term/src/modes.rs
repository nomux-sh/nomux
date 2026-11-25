//! The modes of a terminal.

use bitflags::bitflags;

bitflags! {
    /// The modes that a program can set. A client needs them to encode the
    /// keys, the mouse and the paste data, and to make the same terminal state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Modes: u32 {
        /// The alternate screen is visible (DEC mode 1049).
        const ALTERNATE_SCREEN = 1;
        /// The cursor keys send application sequences (DECCKM, DEC mode 1).
        const APPLICATION_CURSOR = 1 << 1;
        /// The keypad sends application sequences (DECKPAM).
        const APPLICATION_KEYPAD = 1 << 2;
        /// Paste data is put between two marks (DEC mode 2004).
        const BRACKETED_PASTE = 1 << 3;
        /// The program gets focus events (DEC mode 1004).
        const FOCUS_REPORTING = 1 << 4;
        /// The program gets mouse clicks (DEC mode 1000).
        const MOUSE_CLICK = 1 << 5;
        /// The program gets mouse clicks and drag motion (DEC mode 1002).
        const MOUSE_DRAG = 1 << 6;
        /// The program gets mouse clicks and all motion (DEC mode 1003).
        const MOUSE_MOTION = 1 << 7;
        /// Mouse events use the UTF-8 encoding (DEC mode 1005).
        const MOUSE_UTF8 = 1 << 8;
        /// Mouse events use the SGR encoding (DEC mode 1006).
        const MOUSE_SGR = 1 << 9;
        /// The mouse wheel sends cursor keys on the alternate screen (DEC mode 1007).
        const ALTERNATE_SCROLL = 1 << 10;
        /// A character after the last column goes to the next row (DECAWM, DEC mode 7).
        const AUTOWRAP = 1 << 11;
        /// Cursor positions are relative to the scroll region (DECOM, DEC mode 6).
        const ORIGIN = 1 << 12;
        /// A new character moves the characters at its right (IRM, mode 4).
        const INSERT = 1 << 13;
        /// A line feed also moves the cursor to the first column (LNM, mode 20).
        const LINE_FEED_NEW_LINE = 1 << 14;
        /// Kitty keyboard protocol: disambiguate escape codes (flag 1).
        const KEYBOARD_DISAMBIGUATE = 1 << 15;
        /// Kitty keyboard protocol: report event types (flag 2).
        const KEYBOARD_EVENT_TYPES = 1 << 16;
        /// Kitty keyboard protocol: report alternate keys (flag 4).
        const KEYBOARD_ALTERNATE_KEYS = 1 << 17;
        /// Kitty keyboard protocol: report all keys as escape codes (flag 8).
        const KEYBOARD_ALL_AS_ESCAPE_CODES = 1 << 18;
        /// Kitty keyboard protocol: report associated text (flag 16).
        const KEYBOARD_ASSOCIATED_TEXT = 1 << 19;
    }
}

impl Modes {
    /// The flags of the kitty keyboard protocol, in the encoding of the protocol.
    #[must_use]
    pub fn keyboard_flags(self) -> u8 {
        const FLAGS: [Modes; 5] = [
            Modes::KEYBOARD_DISAMBIGUATE,
            Modes::KEYBOARD_EVENT_TYPES,
            Modes::KEYBOARD_ALTERNATE_KEYS,
            Modes::KEYBOARD_ALL_AS_ESCAPE_CODES,
            Modes::KEYBOARD_ASSOCIATED_TEXT,
        ];
        FLAGS
            .iter()
            .zip(0..)
            .filter(|(mode, _)| self.contains(**mode))
            .fold(0, |flags, (_, bit)| flags | (1 << bit))
    }
}
