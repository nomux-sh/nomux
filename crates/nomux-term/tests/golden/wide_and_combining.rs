//! Characters that use two cells, and characters that use no cell of their
//! own.

use crate::snapshot::assert_screen;

#[test]
fn wide_character_uses_two_cells() {
    assert_screen(
        "日本x",
        "
|日本x               |
|                    |
|                    |
|                    |
|                    |
cursor 0,5
",
    );
}

#[test]
fn wide_character_that_does_not_fit_in_the_row_goes_to_the_next_row() {
    assert_screen(
        "abcdefghijklmnopqrs日",
        "
|abcdefghijklmnopqrs |
|日                  |
|                    |
|                    |
|                    |
cursor 1,2
",
    );
}

#[test]
fn character_on_the_first_cell_of_a_wide_character_erases_the_second_cell() {
    assert_screen(
        "日\x1b[1;1Hx",
        "
|x                   |
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn character_on_the_second_cell_of_a_wide_character_erases_the_first_cell() {
    assert_screen(
        "日\x1b[1;2Hx",
        "
| x                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
",
    );
}

#[test]
fn combining_mark_goes_to_the_cell_before_the_cursor() {
    assert_screen(
        "e\u{301}\u{323}x",
        "
|ex                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
combining 0,0 U+0301 U+0323
",
    );
}

#[test]
fn combining_mark_after_a_wide_character_goes_to_its_first_cell() {
    assert_screen(
        "日\u{301}x",
        "
|日x                 |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
combining 0,0 U+0301
",
    );
}

#[test]
fn emoji_uses_two_cells() {
    assert_screen(
        "🚀x",
        "
|🚀x                 |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
",
    );
}

/// A known gap: the VT library has no grapheme clusters. A terminal with
/// grapheme clusters shows one emoji of two cells.
#[test]
fn emoji_that_a_zero_width_joiner_connects_use_two_cells_each() {
    assert_screen(
        "👨\u{200d}👩x",
        "
|👨👩x               |
|                    |
|                    |
|                    |
|                    |
cursor 0,5
combining 0,0 U+200D
",
    );
}

/// A known gap: see the zero width joiner. A terminal with grapheme clusters
/// shows the heart as an emoji of two cells.
#[test]
fn emoji_variation_selector_does_not_make_a_character_wide() {
    assert_screen(
        "\u{2764}\u{fe0f}x",
        "
|❤x                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
combining 0,0 U+FE0F
",
    );
}
