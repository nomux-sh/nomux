# nomux-term

The terminal state of nomux: a thin abstraction over one VT library, `alacritty_terminal` 0.26.0.
The holder gives the output of a program to a `Terminal`, and reads the screen, the redraw sequence for a client that attaches, and the OSC sequences that the library does not handle.

The documentation of the crate (`cargo doc -p nomux-term --open`) has the interface.
[ADR-0003](https://github.com/nomux-sh/docs/blob/main/adr/0003-vt-library.md) has the selection of the VT library.

## Supported terminal features and known gaps

Each line of this section has a test that fails if the line is not true any more:

- **G** is a golden test in [`tests/golden`](tests/golden): a text snapshot of the screen after one sequence.
- **R** is a test of the redraw sequence in [`tests/redraw.rs`](tests/redraw.rs).
- **O** is a test of the OSC sequences in [`tests/unhandled_osc.rs`](tests/unhandled_osc.rs).
- **C** is a conformance case of the [VT library benchmark](../../bench/README.md), which `nomux-term` passes: 45 of 46 cases.

The fuzz targets in [`fuzz`](../../fuzz/README.md) give arbitrary output to the same code.

### Supported

| Feature | Sequences | Test |
|---|---|---|
| Colors | The 16 palette colors, the bright colors (SGR 90 to 97, 100 to 107), 256 colors, true color with `;` and with `:` | G `sgr`, R |
| Attributes | Bold, dim, italic, inverse, hidden, strikethrough, and the parameter that turns off each one | G `sgr`, R |
| Underline | Single, double, curly, dotted, dashed, and the underline color (SGR 58, 59) | G `sgr`, R |
| Cursor motion | CUP, CUU, CUD, CUF, CUB, CHA, VPA, CNL, CPL, IND, NEL, RI, backspace, carriage return, line feed | G `cursor_motion`, C |
| Saved cursor | DECSC and DECRC, with the position and the style | G `cursor_motion`, C |
| Wrap | The wrap after the last column, and autowrap off (DECAWM) | G `cursor_motion`, C |
| Erase | EL, ED and ECH. Erased cells get the current background color | G `erase`, C |
| Scroll region | DECSTBM, SU, SD, and the origin mode (DECOM) | G `scroll_region`, C |
| Insert and delete | ICH, DCH, IL, DL, REP, and the insert mode (IRM) | G `insert_and_delete`, C |
| Alternate screen | DEC mode 1049: an empty screen at each entry, and the screen and the cursor come back | G `alternate_screen`, C |
| Line drawing | The DEC special graphics character set in G0 and in G1, with shift out and shift in | G `line_drawing`, C |
| Wide characters | Two cells for a wide character and for an emoji, and the wrap of a wide character. One cell of a wide character without the other cell is an empty cell | G `wide_and_combining`, R, C |
| Combining marks | A combining character goes to the cell before the cursor | G `wide_and_combining`, C |
| Tab stops | The default stops, HTS, TBC, CBT and CHT | G `tab_stops`, C |
| Bracketed paste | DEC mode 2004 | G `modes` |
| Mouse | The tracking modes (DEC modes 1000, 1002, 1003) and the encodings (DEC modes 1005, 1006) | G `modes` |
| Focus reporting | DEC mode 1004 | G `modes` |
| Kitty keyboard protocol | Push, pop and set of the five flags, and a separate stack for the alternate screen | G `modes` |
| Key modes | Application cursor keys (DECCKM) and the application keypad | G `modes` |
| Cursor look | Visible or hidden (DECTCEM), and the shape and the blinking (DECSCUSR) | G `modes`, R |
| Full reset | RIS gives the modes of a new terminal | G `modes`, C |
| OSC for the caller | The caller gets each OSC sequence that the library does not handle, for example OSC 9, 133, 777 and 7501 | O |
| Program reports | `Osc::record` parses the program status (OSC 7501), the prompt marks (OSC 133) and the notifications (OSC 9, OSC 777) | The unit tests of `src/osc` |
| Redraw sequence | The cells, the cursor, the modes, the kitty keyboard flags and the style of the next character. With the alternate screen, also the screen behind it | R |

The library also handles the OSC sequences for the title, the palette colors, the hyperlinks (OSC 8) and the clipboard (OSC 52). `nomux-term` has no interface for them yet.

### Known gaps

| Gap | Result | Reference |
|---|---|---|
| DEC modes 47 and 1047 | The library has the alternate screen only for DEC mode 1049. The text of the program goes to the primary screen. Programs that use the `xterm` terminal description use mode 1049 | G `alternate_screen`, C |
| Kitty graphics | The library ignores the sequences. No picture is kept | ADR-0003 |
| Sixel | The library ignores the sequences. No picture is kept | ADR-0003 |
| Grapheme clusters | An emoji that a zero width joiner connects to a second emoji uses two cells for each emoji. The emoji variation selector does not make a character wide | G `wide_and_combining` |
| Erase in display above the cursor | From the second row, ED 1 keeps the first row. The cause is in the library. A correction in `nomux-term` needs a second parser for the CSI parameters in the path of each byte, thus the gap stays until a release of the library corrects it | G `erase`, [#29](https://github.com/nomux-sh/nomux/issues/29) |
| A terminal of 1 column | `Size::new` refuses fewer than 2 columns: the library cannot keep a wide character in 1 column. The holder refuses such a window | [`tests/size.rs`](tests/size.rs) |
| The wrap after a backward tab | After a character in the last column, a backward tab (CBT) keeps the wrap of the library: a next character in the same call of `Terminal::feed` goes to the next row. `nomux-term` ends the wrap at the end of each call | G `tab_stops`, R |
| Synchronized updates | `nomux-term` turns off DEC mode 2026: each byte changes the screen immediately | ADR-0003 |

The redraw sequence does not make these parts of the state, because the library has no read access to them. A client that attaches has the default value until the program sets the value again:

| Not in the redraw sequence | Result for a client that attaches |
|---|---|
| Scroll region | The region is the full screen. A program that set a region and then scrolls moves all rows of the client |
| Tab stops | The stops are at each eighth column |
| Saved cursor | DECRC moves the cursor to the first cell |
| Character sets | G0 and G1 are ASCII. Line drawing that the program selected before the attach shows letters for the next characters |
| Hyperlinks, changed palette colors, marks of wrapped lines | The text has no link, the palette has the colors of the client, and a resize of the client does not join wrapped lines |

## Tests

```sh
cargo nextest run -p nomux-term
```

A golden test prints the snapshot and the screen when they are different. If the new screen is correct, copy it into the test.
The snapshots are text in the test files, without a snapshot library: the screens have 5 rows, thus the two printed screens are a readable difference, and the sequence and its screen are in one place.
