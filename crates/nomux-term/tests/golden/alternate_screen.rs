//! The alternate screen (DEC mode 1049) and the modes that the VT library
//! does not know (DEC modes 47 and 1047).

use crate::snapshot::assert_screen;

#[test]
fn alternate_screen_starts_empty_with_the_cursor_of_the_primary_screen() {
    assert_screen(
        "main\x1b[?1049halt",
        "
|    alt             |
|                    |
|                    |
|                    |
|                    |
cursor 0,7
modes +ALTERNATE_SCREEN
",
    );
}

#[test]
fn leaving_the_alternate_screen_gives_back_the_screen_and_the_cursor() {
    assert_screen(
        "main\x1b[?1049h\x1b[3;3Halt\x1b[?1049l",
        "
|main                |
|                    |
|                    |
|                    |
|                    |
cursor 0,4
",
    );
}

#[test]
fn alternate_screen_is_empty_again_at_the_second_entry() {
    assert_screen(
        "\x1b[?1049h\x1b[3;3Hfirst\x1b[?1049l\x1b[?1049h",
        "
|                    |
|                    |
|                    |
|                    |
|                    |
cursor 0,0
modes +ALTERNATE_SCREEN
",
    );
}

#[test]
fn scroll_region_is_the_same_for_the_two_screens() {
    assert_screen(
        "L1\r\nL2\r\nL3\r\nL4\r\nL5\x1b[?1049h\x1b[2;4r\x1b[?1049l\x1b[5;1H\n",
        "
|L1                  |
|L2                  |
|L3                  |
|L4                  |
|L5                  |
cursor 4,0
",
    );
}

/// A known gap: the VT library has the alternate screen only for DEC mode
/// 1049. Programs that use the `xterm` terminal description use mode 1049.
#[test]
fn dec_mode_1047_does_not_select_the_alternate_screen() {
    assert_screen(
        "main\x1b[?1047h\x1b[Halt\x1b[?1047l",
        "
|altn                |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
",
    );
}

/// A known gap: see DEC mode 1047.
#[test]
fn dec_mode_47_does_not_select_the_alternate_screen() {
    assert_screen(
        "main\x1b[?47h\x1b[Halt\x1b[?47l",
        "
|altn                |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
",
    );
}
