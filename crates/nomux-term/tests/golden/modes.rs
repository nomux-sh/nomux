//! The modes that a client needs to encode the keys, the mouse and the paste
//! data, and the look of the cursor.

use crate::snapshot::{assert_cursor, assert_modes};

#[test]
fn bracketed_paste_is_set_and_reset() {
    assert_modes("\x1b[?2004h", "+BRACKETED_PASTE");
    assert_modes("\x1b[?2004h\x1b[?2004l", "");
}

#[test]
fn mouse_tracking_mode_replaces_the_tracking_mode_before_it() {
    assert_modes("\x1b[?1000h", "+MOUSE_CLICK");
    assert_modes("\x1b[?1000h\x1b[?1002h", "+MOUSE_DRAG");
    assert_modes("\x1b[?1000h\x1b[?1002h\x1b[?1003h", "+MOUSE_MOTION");
}

#[test]
fn mouse_encoding_is_set_together_with_a_tracking_mode() {
    assert_modes("\x1b[?1003h\x1b[?1006h", "+MOUSE_MOTION +MOUSE_SGR");
    assert_modes("\x1b[?1000h\x1b[?1005h", "+MOUSE_CLICK +MOUSE_UTF8");
}

#[test]
fn mouse_modes_are_reset() {
    assert_modes("\x1b[?1003h\x1b[?1006h\x1b[?1003l\x1b[?1006l", "");
}

#[test]
fn one_sequence_sets_each_mode_of_its_parameters() {
    assert_modes(
        "\x1b[?1002;1006;2004;1004h",
        "+BRACKETED_PASTE +FOCUS_REPORTING +MOUSE_DRAG +MOUSE_SGR",
    );
}

#[test]
fn application_cursor_keys_and_application_keypad_are_set_and_reset() {
    assert_modes("\x1b[?1h\x1b=", "+APPLICATION_CURSOR +APPLICATION_KEYPAD");
    assert_modes("\x1b[?1h\x1b=\x1b[?1l\x1b>", "");
}

#[test]
fn kitty_keyboard_push_sets_the_flags() {
    assert_modes(
        "\x1b[>5u",
        "+KEYBOARD_DISAMBIGUATE +KEYBOARD_ALTERNATE_KEYS",
    );
    assert_modes(
        "\x1b[>31u",
        "+KEYBOARD_DISAMBIGUATE +KEYBOARD_EVENT_TYPES +KEYBOARD_ALTERNATE_KEYS +KEYBOARD_ALL_AS_ESCAPE_CODES +KEYBOARD_ASSOCIATED_TEXT",
    );
}

#[test]
fn kitty_keyboard_pop_gives_back_the_flags_before_the_push() {
    assert_modes("\x1b[>1u\x1b[>2u\x1b[<u", "+KEYBOARD_DISAMBIGUATE");
    assert_modes("\x1b[>1u\x1b[>2u\x1b[<2u", "");
}

#[test]
fn kitty_keyboard_pop_of_more_entries_than_the_stack_has_leaves_no_flag() {
    assert_modes("\x1b[>1u\x1b[<9u", "");
}

#[test]
fn kitty_keyboard_set_replaces_adds_or_removes_flags() {
    assert_modes("\x1b[>1u\x1b[=8u", "+KEYBOARD_ALL_AS_ESCAPE_CODES");
    assert_modes(
        "\x1b[>1u\x1b[=8;2u",
        "+KEYBOARD_DISAMBIGUATE +KEYBOARD_ALL_AS_ESCAPE_CODES",
    );
    assert_modes(
        "\x1b[>15u\x1b[=8;3u",
        "+KEYBOARD_DISAMBIGUATE +KEYBOARD_EVENT_TYPES +KEYBOARD_ALTERNATE_KEYS",
    );
}

#[test]
fn alternate_screen_has_its_own_kitty_keyboard_flags() {
    assert_modes("\x1b[>1u\x1b[?1049h", "+ALTERNATE_SCREEN");
    assert_modes(
        "\x1b[>1u\x1b[?1049h\x1b[>2u\x1b[?1049l",
        "+KEYBOARD_DISAMBIGUATE",
    );
}

#[test]
fn full_reset_gives_the_modes_of_a_new_terminal() {
    assert_modes(
        "\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?7l\x1b[4h\x1b[>1u\x1bc",
        "",
    );
}

#[test]
fn cursor_style_parameter_selects_the_shape_and_the_blinking() {
    assert_cursor("\x1b[1 q", "0,0 blinking");
    assert_cursor("\x1b[2 q", "0,0");
    assert_cursor("\x1b[3 q", "0,0 underline blinking");
    assert_cursor("\x1b[4 q", "0,0 underline");
    assert_cursor("\x1b[5 q", "0,0 bar blinking");
    assert_cursor("\x1b[6 q", "0,0 bar");
    assert_cursor("\x1b[5 q\x1b[0 q", "0,0");
}

#[test]
fn cursor_is_hidden_and_shown() {
    assert_cursor("\x1b[?25l", "0,0 hidden");
    assert_cursor("\x1b[?25l\x1b[?25h", "0,0");
}
