//! The scroll region (DECSTBM) and the origin mode (DECOM).

use crate::snapshot::assert_screen;

const FIVE_ROWS: &str = "L1\r\nL2\r\nL3\r\nL4\r\nL5";
/// The rows 2 to 4 of the 5 rows.
const REGION: &str = "\x1b[2;4r";

fn assert_screen_after_five_rows(output: &str, expected: &str) {
    assert_screen(&format!("{FIVE_ROWS}{output}"), expected);
}

#[test]
fn set_scroll_region_moves_the_cursor_to_the_first_cell() {
    assert_screen(
        &format!("\x1b[3;3H{REGION}X"),
        "
|X                   |
|                    |
|                    |
|                    |
|                    |
cursor 0,1
",
    );
}

#[test]
fn line_feed_in_the_last_row_of_the_region_scrolls_the_region_only() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[4;1H\n"),
        "
|L1                  |
|L3                  |
|L4                  |
|                    |
|L5                  |
cursor 3,0
",
    );
}

#[test]
fn reverse_index_in_the_first_row_of_the_region_scrolls_the_region_only() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[2;1H\x1bM"),
        "
|L1                  |
|                    |
|L2                  |
|L3                  |
|L5                  |
cursor 1,0
",
    );
}

#[test]
fn line_feed_below_the_region_scrolls_nothing() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[5;1H\nX"),
        "
|L1                  |
|L2                  |
|L3                  |
|L4                  |
|X5                  |
cursor 4,1
",
    );
}

#[test]
fn scroll_up_moves_the_rows_of_the_region_only() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[2S"),
        "
|L1                  |
|L4                  |
|                    |
|                    |
|L5                  |
cursor 0,0
",
    );
}

#[test]
fn scroll_down_moves_the_rows_of_the_region_only() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[2T"),
        "
|L1                  |
|                    |
|                    |
|L2                  |
|L5                  |
cursor 0,0
",
    );
}

#[test]
fn insert_line_in_the_region_moves_no_row_below_the_region() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[3;1H\x1b[L"),
        "
|L1                  |
|L2                  |
|                    |
|L3                  |
|L5                  |
cursor 2,0
",
    );
}

#[test]
fn delete_line_in_the_region_moves_no_row_below_the_region() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[3;1H\x1b[M"),
        "
|L1                  |
|L2                  |
|L4                  |
|                    |
|L5                  |
cursor 2,0
",
    );
}

#[test]
fn scroll_region_without_parameters_is_the_full_screen() {
    assert_screen_after_five_rows(
        &format!("{REGION}\x1b[r\x1b[5;1H\n"),
        "
|L2                  |
|L3                  |
|L4                  |
|L5                  |
|                    |
cursor 4,0
",
    );
}

#[test]
fn origin_mode_counts_rows_from_the_region_and_keeps_the_cursor_in_it() {
    assert_screen(
        &format!("{REGION}\x1b[?6h\x1b[1;1HA\x1b[9;3HB"),
        "
|                    |
|A                   |
|                    |
|  B                 |
|                    |
cursor 3,3
modes +ORIGIN
",
    );
}
