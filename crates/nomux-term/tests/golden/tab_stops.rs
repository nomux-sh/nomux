//! Tab stops: the default stops, the stops that a program sets and clears,
//! and the moves to a stop.

use crate::snapshot::assert_screen;

#[test]
fn default_tab_stops_are_at_each_eighth_column() {
    assert_screen(
        "a\tb\tc",
        "
|a       b       c   |
|                    |
|                    |
|                    |
|                    |
cursor 0,17
",
    );
}

#[test]
fn tab_after_the_last_stop_goes_to_the_last_column_of_the_row() {
    assert_screen(
        "\t\t\t\tx",
        "
|                   x|
|                    |
|                    |
|                    |
|                    |
cursor 0,19
",
    );
}

#[test]
fn tab_erases_no_character() {
    assert_screen(
        "abcdefghij\r\tX",
        "
|abcdefghXj          |
|                    |
|                    |
|                    |
|                    |
cursor 0,9
",
    );
}

#[test]
fn set_tab_stop_adds_a_stop_at_the_cursor() {
    assert_screen(
        "\x1b[1;5H\x1bH\r\tx",
        "
|    x               |
|                    |
|                    |
|                    |
|                    |
cursor 0,5
",
    );
}

#[test]
fn clear_tab_stop_removes_the_stop_at_the_cursor_only() {
    assert_screen(
        "\x1b[1;9H\x1b[g\r\tx",
        "
|                x   |
|                    |
|                    |
|                    |
|                    |
cursor 0,17
",
    );
}

#[test]
fn clear_all_tab_stops_leaves_no_stop() {
    assert_screen(
        "\x1b[3g\tx",
        "
|                   x|
|                    |
|                    |
|                    |
|                    |
cursor 0,19
",
    );
}

#[test]
fn backward_tab_goes_to_the_stop_before_the_cursor() {
    assert_screen(
        "\x1b[1;20H\x1b[Zx\x1b[1;12H\x1b[2Zy",
        "
|y               x   |
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn forward_tab_with_a_count_goes_that_number_of_stops() {
    assert_screen(
        "\x1b[2Ix",
        "
|                x   |
|                    |
|                    |
|                    |
|                    |
cursor 0,17
",
    );
}

/// A known gap: after a character in the last column, the VT library keeps
/// the wrap when a backward tab moves the cursor. A VT terminal writes the
/// next character at the tab stop. `nomux-term` ends the wrap at the end of
/// a call of `Terminal::feed`, not in the call.
#[test]
fn backward_tab_after_the_last_column_keeps_the_wrap_in_the_same_call() {
    assert_screen(
        "aaaaaaaaaaaaaaaaaaaa\x1b[Zb",
        "
|aaaaaaaaaaaaaaaaaaaa|
|b                   |
|                    |
|                    |
|                    |
cursor 1,1
",
    );
}
