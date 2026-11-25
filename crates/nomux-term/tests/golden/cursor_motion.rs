//! Cursor motion, the control characters that move the cursor, and the wrap
//! at the last column.

use crate::snapshot::assert_screen;

#[test]
fn cursor_position_stops_at_the_last_row_and_the_last_column() {
    assert_screen(
        "\x1b[99;99HX",
        "
|                    |
|                    |
|                    |
|                    |
|                   X|
cursor 4,19
",
    );
}

#[test]
fn relative_moves_stop_at_the_edges_of_the_screen() {
    assert_screen(
        "\x1b[3;3H\x1b[9A\x1b[9DA\x1b[9B\x1b[99CB",
        "
|A                   |
|                    |
|                    |
|                    |
|                   B|
cursor 4,19
",
    );
}

#[test]
fn relative_move_without_a_parameter_moves_one_cell() {
    assert_screen(
        "\x1b[3;3H\x1b[A\x1b[CA\x1b[B\x1b[DB",
        "
|                    |
|   A                |
|   B                |
|                    |
|                    |
cursor 2,4
",
    );
}

#[test]
fn column_absolute_and_line_absolute_keep_the_other_coordinate() {
    assert_screen(
        "\x1b[3;5H\x1b[2GA\x1b[4dB",
        "
|                    |
|                    |
| A                  |
|  B                 |
|                    |
cursor 3,3
",
    );
}

#[test]
fn next_line_and_previous_line_go_to_the_first_column() {
    assert_screen(
        "ab\x1b[2Ec\x1b[1Fd",
        "
|ab                  |
|d                   |
|c                   |
|                    |
|                    |
cursor 1,1
",
    );
}

#[test]
fn backspace_and_carriage_return_erase_no_character() {
    assert_screen(
        "abc\x08\x08X\rY",
        "
|YXc                 |
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn line_feed_keeps_the_column() {
    assert_screen(
        "ab\ncd",
        "
|ab                  |
|  cd                |
|                    |
|                    |
|                    |
cursor 1,4
",
    );
}

#[test]
fn index_keeps_the_column_and_next_line_goes_to_the_first_column() {
    assert_screen(
        "a\x1bDb\x1bEc",
        "
|a                   |
| b                  |
|c                   |
|                    |
|                    |
cursor 2,1
",
    );
}

#[test]
fn reverse_index_in_the_top_row_scrolls_the_screen_down() {
    assert_screen(
        "1\r\n2\x1b[H\x1bM",
        "
|                    |
|1                   |
|2                   |
|                    |
|                    |
cursor 0,0
",
    );
}

#[test]
fn restore_cursor_gives_back_the_position_and_the_style() {
    assert_screen(
        "\x1b[2;3H\x1b[1;31m\x1b7\x1b[0m\x1b[5;5HA\x1b8B",
        "
|                    |
|  B                 |
|                    |
|                    |
|    A               |
cursor 1,3
style 1,2 bold fg=1
",
    );
}

#[test]
fn cursor_stays_in_the_last_column_after_a_full_row() {
    assert_screen(
        "abcdefghijklmnopqrst",
        "
|abcdefghijklmnopqrst|
|                    |
|                    |
|                    |
|                    |
cursor 0,19
",
    );
}

#[test]
fn character_after_a_full_row_goes_to_the_next_row() {
    assert_screen(
        "abcdefghijklmnopqrstu",
        "
|abcdefghijklmnopqrst|
|u                   |
|                    |
|                    |
|                    |
cursor 1,1
",
    );
}

#[test]
fn carriage_return_after_a_full_row_stays_in_the_row() {
    assert_screen(
        "abcdefghijklmnopqrst\rX",
        "
|Xbcdefghijklmnopqrst|
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn autowrap_off_writes_each_next_character_in_the_last_column() {
    assert_screen(
        "\x1b[?7labcdefghijklmnopqrstuvZ",
        "
|abcdefghijklmnopqrsZ|
|                    |
|                    |
|                    |
|                    |
cursor 0,19
modes -AUTOWRAP
",
    );
}
