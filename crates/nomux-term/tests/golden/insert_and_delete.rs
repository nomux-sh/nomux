//! Insert and delete of characters and of lines, the insert mode and the
//! repeat of a character.

use crate::snapshot::assert_screen;

const FIVE_ROWS: &str = "L1\r\nL2\r\nL3\r\nL4\r\nL5";

#[test]
fn insert_characters_moves_the_rest_right_and_drops_the_end_of_the_row() {
    assert_screen(
        "abcdefghijklmnopqrst\x1b[1;3H\x1b[2@",
        "
|ab  cdefghijklmnopqr|
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn delete_characters_moves_the_rest_left() {
    assert_screen(
        "abcdefghijklmnopqrst\x1b[1;3H\x1b[2P",
        "
|abefghijklmnopqrst  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn insert_mode_moves_the_rest_right_for_each_character() {
    assert_screen(
        "abc\x1b[1;1H\x1b[4hXY",
        "
|XYabc               |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
modes +INSERT
",
    );
}

#[test]
fn character_replaces_the_cell_after_insert_mode_is_reset() {
    assert_screen(
        "abc\x1b[1;1H\x1b[4hX\x1b[4lY",
        "
|XYbc                |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn insert_line_moves_the_rows_down_and_drops_the_last_row() {
    assert_screen(
        &format!("{FIVE_ROWS}\x1b[2;1H\x1b[2L"),
        "
|L1                  |
|                    |
|                    |
|L2                  |
|L3                  |
cursor 1,0
",
    );
}

#[test]
fn delete_line_moves_the_rows_up_and_adds_empty_rows_at_the_end() {
    assert_screen(
        &format!("{FIVE_ROWS}\x1b[2;1H\x1b[2M"),
        "
|L1                  |
|L4                  |
|L5                  |
|                    |
|                    |
cursor 1,0
",
    );
}

#[test]
fn repeat_writes_the_character_before_it_again() {
    assert_screen(
        "ab\x1b[3b",
        "
|abbbb               |
|                    |
|                    |
|                    |
|                    |
cursor 0,5
",
    );
}
