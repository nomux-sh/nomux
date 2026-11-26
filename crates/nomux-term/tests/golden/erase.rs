//! Erase in line (EL), erase in display (ED) and erase characters (ECH).

use crate::snapshot::assert_screen;

const THREE_ROWS: &str = "one\r\ntwo\r\nthree";

#[test]
fn erase_in_line_to_the_end_starts_at_the_cursor() {
    assert_screen(
        "abcdef\x1b[3G\x1b[K",
        "
|ab                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn erase_in_line_to_the_start_includes_the_cursor() {
    assert_screen(
        "abcdef\x1b[3G\x1b[1K",
        "
|   def              |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn erase_in_line_of_the_full_row_keeps_the_cursor() {
    assert_screen(
        "abcdef\x1b[3G\x1b[2K",
        "
|                    |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn erase_in_display_below_starts_at_the_cursor() {
    assert_screen(
        &format!("{THREE_ROWS}\x1b[2;2H\x1b[J"),
        "
|one                 |
|t                   |
|                    |
|                    |
|                    |
cursor 1,1
",
    );
}

#[test]
fn erase_in_display_above_includes_the_cursor() {
    assert_screen(
        &format!("{THREE_ROWS}\x1b[3;2H\x1b[1J"),
        "
|                    |
|                    |
|  ree               |
|                    |
|                    |
cursor 2,1
",
    );
    assert_screen(
        &format!("{THREE_ROWS}\x1b[2;2H\x1b[44m\x1b[1J"),
        "
|                    |
|  o                 |
|three               |
|                    |
|                    |
cursor 1,1
style 0,0-19 bg=4
style 1,0-1 bg=4
",
    );
}

#[test]
fn erase_in_display_of_the_full_screen_keeps_the_cursor() {
    assert_screen(
        &format!("{THREE_ROWS}\x1b[2;2H\x1b[2JX"),
        "
|                    |
| X                  |
|                    |
|                    |
|                    |
cursor 1,2
",
    );
}

#[test]
fn erase_characters_keeps_the_cursor_and_the_rest_of_the_row() {
    assert_screen(
        "abcdef\x1b[1;2H\x1b[3X",
        "
|a   ef              |
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn erased_cells_get_the_current_background_color() {
    assert_screen(
        "abcdef\r\nabcdef\x1b[44m\x1b[1;5H\x1b[K\x1b[2;2H\x1b[2X",
        "
|abcd                |
|a  def              |
|                    |
|                    |
|                    |
cursor 1,1
style 0,4-19 bg=4
style 1,1-2 bg=4
",
    );
}
