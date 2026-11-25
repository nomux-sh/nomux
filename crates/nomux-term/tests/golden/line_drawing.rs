//! The DEC special graphics character set: the line drawing characters of
//! ncurses programs.

use crate::snapshot::assert_screen;

#[test]
fn line_drawing_in_g0_draws_a_box() {
    assert_screen(
        "\x1b(0lqqk\r\nx  x\r\nmqqj\x1b(B",
        "
|┌──┐                |
|│  │                |
|└──┘                |
|                    |
|                    |
cursor 2,4
",
    );
}

#[test]
fn ascii_in_g0_after_line_drawing_writes_letters_again() {
    assert_screen(
        "\x1b(0q\x1b(Bq",
        "
|─q                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn shift_out_selects_line_drawing_in_g1_and_shift_in_selects_g0() {
    assert_screen(
        "\x1b)0q\x0eq\x0fq",
        "
|q─q                 |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
",
    );
}

#[test]
fn line_drawing_changes_each_character_from_the_grave_accent_to_the_tilde() {
    assert_screen(
        "\x1b(0_`abcdefghijklmnopqrstuvwxyz{|}~",
        "
| ◆▒␉␌␍␊°±␤␋┘┐┌└┼⎺⎻─⎼|
|⎽├┤┴┬│≤≥π≠£·        |
|                    |
|                    |
|                    |
cursor 1,12
",
    );
}

#[test]
fn line_drawing_changes_no_uppercase_letter_and_no_digit() {
    assert_screen(
        "\x1b(0AZ09",
        "
|AZ09                |
|                    |
|                    |
|                    |
|                    |
cursor 0,4
",
    );
}
