//! Select graphic rendition (SGR): the colors and the attributes of the text.

use crate::snapshot::assert_screen;

#[test]
fn true_color_with_colons_is_the_color_with_semicolons() {
    assert_screen(
        "\x1b[38:2::10:20:30;48:2::1:2:3mA\x1b[0;38;2;10;20;30;48;2;1;2;3mB",
        "
|AB                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
style 0,0-1 fg=#0a141e bg=#010203
",
    );
}

#[test]
fn bright_color_parameters_are_the_palette_entries_8_to_15() {
    assert_screen(
        "\x1b[90mA\x1b[97mB\x1b[100mC\x1b[107mD",
        "
|ABCD                |
|                    |
|                    |
|                    |
|                    |
cursor 0,4
style 0,0 fg=8
style 0,1 fg=15
style 0,2 fg=15 bg=8
style 0,3 fg=15 bg=15
",
    );
}

#[test]
fn each_reset_parameter_turns_off_its_attributes_only() {
    assert_screen(
        "\x1b[1;2;3;4;7;8;9mA\x1b[22mB\x1b[23mC\x1b[24mD\x1b[27mE\x1b[28mF\x1b[29mG",
        "
|ABCDEFG             |
|                    |
|                    |
|                    |
|                    |
cursor 0,7
style 0,0 bold dim italic inverse hidden strikethrough underline=single
style 0,1 italic inverse hidden strikethrough underline=single
style 0,2 inverse hidden strikethrough underline=single
style 0,3 inverse hidden strikethrough
style 0,4 hidden strikethrough
style 0,5 strikethrough
",
    );
}

#[test]
fn default_color_parameters_keep_the_attributes() {
    assert_screen(
        "\x1b[1;31;42mA\x1b[39mB\x1b[49mC",
        "
|ABC                 |
|                    |
|                    |
|                    |
|                    |
cursor 0,3
style 0,0 bold fg=1 bg=2
style 0,1 bold bg=2
style 0,2 bold
",
    );
}

#[test]
fn default_underline_color_parameter_keeps_the_underline() {
    assert_screen(
        "\x1b[4;58;5;9mA\x1b[59mB",
        "
|AB                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
style 0,0 underline=single ul=9
style 0,1 underline=single
",
    );
}

#[test]
fn sgr_without_a_parameter_resets_the_style() {
    assert_screen(
        "\x1b[1;4;31;42mA\x1b[mB",
        "
|AB                  |
|                    |
|                    |
|                    |
|                    |
cursor 0,2
style 0,0 bold underline=single fg=1 bg=2
",
    );
}
