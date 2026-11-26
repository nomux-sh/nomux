# nomux-term

The terminal state of nomux: a thin abstraction over one VT library, `alacritty_terminal` 0.26.0.
The holder gives the output of a program to a `Terminal`, and reads the screen, the redraw sequence for a client that attaches, and the OSC sequences that the library does not handle.

The documentation of the crate (`cargo doc -p nomux-term --open`) has the interface.
[ADR-0003](https://github.com/nomux-sh/docs/blob/main/adr/0003-vt-library.md) has the selection of the VT library.

## Supported terminal features and known gaps

Each line of this section has a test that fails if the line is not true any more:

- **G** is a golden test in [`tests/golden`](tests/golden): a text snapshot of the screen after one sequence.
- **R** is a test of the redraw sequence in [`tests/redraw.rs`](tests/redraw.rs).
- **H** is a test of the history in [`tests/history.rs`](tests/history.rs).
- **O** is a test of the OSC sequences in [`tests/unhandled_osc.rs`](tests/unhandled_osc.rs).
- **K** is a conformance case in [`tests/golden/cases`](tests/golden/cases), in the style of `vttest` and `esctest`: the sequence and the screen of xterm. The name of a file is the group. A case of a known gap also has the screen of `nomux-term` now.
- **C** is a conformance case of the [VT library benchmark](../../bench/README.md), which `nomux-term` passes: 45 of 46 cases.

The fuzz targets in [`fuzz`](../../fuzz/README.md) give arbitrary output to the same code.

### Supported

| Feature | Sequences | Test |
|---|---|---|
| Colors | The 16 palette colors, the bright colors (SGR 90 to 97, 100 to 107), 256 colors, true color with `;` and with `:`, also without the color space parameter (`38:2:r:g:b`) | G `sgr`, K `sgr`, R |
| Attributes | Bold, dim, italic, inverse, hidden, strikethrough, and the parameter that turns off each one | G `sgr`, R |
| Underline | Single, double, curly, dotted, dashed, and the underline color (SGR 58, 59), also from the palette (`58:5:n`). `4:0` turns off the underline | G `sgr`, K `sgr`, R |
| Cursor motion | CUP, CUU, CUD, CUF, CUB, CHA, VPA, HPA, HPR, VPR, CNL, CPL, IND, NEL, RI, backspace, carriage return, line feed. CUU and CUD above or below the scroll region stop at the edge of the screen | G `cursor_motion`, K `cursor_and_margins`, C |
| Saved cursor | DECSC and DECRC, with the position and the style. DECRC without DECSC goes to the first cell | G `cursor_motion`, K `cursor_and_margins`, C |
| Wrap | The wrap after the last column, and autowrap off (DECAWM). Backspace, CUP and DECAWM end the pending wrap. A combining mark after a character in the last column goes to that character | G `cursor_motion`, K `autowrap`, C |
| Erase | EL, ED and ECH. ED 3 keeps the screen. DECALN fills the screen with `E` | G `erase`, K `erase`, C |
| Scroll region | DECSTBM, SU, SD, and the origin mode (DECOM). DECSTBM without a row between the margins is ignored. Set DECOM moves the cursor to the first cell of the region. IL, DL and RI outside the region move no row | G `scroll_region`, K `scroll_region`, C |
| Insert and delete | ICH, DCH, IL, DL, REP, and the insert mode (IRM). ICH and ECH with a count larger than the row stop at the end of the row | G `insert_and_delete`, K `insert_and_delete`, C |
| Alternate screen | DEC mode 1049: an empty screen at each entry, and the screen and the cursor come back, with the style of the cursor. DECSC on the alternate screen does not change the cursor of the primary screen | G `alternate_screen`, K `alternate_screen`, C |
| Line drawing | The DEC special graphics character set in G0 and in G1, with shift out and shift in | G `line_drawing`, C |
| Wide characters | Two cells for a wide character and for an emoji, and the wrap of a wide character. One cell of a wide character without the other cell is an empty cell | G `wide_and_combining`, K `insert_and_delete`, R, C |
| Background color erase | Cells that EL, ECH, ICH and DCH make empty, rows that the scroll region scrolls in, and a new alternate screen get the current background color | G `erase`, K `insert_and_delete`, K `scroll_region`, K `alternate_screen` |
| Combining marks | A combining character goes to the cell before the cursor | G `wide_and_combining`, C |
| Tab stops | The default stops, HTS, TBC, CBT and CHT. The two screens have one set of stops | G `tab_stops`, K `tab_stops`, C |
| Bracketed paste | DEC mode 2004, also on the alternate screen | G `modes`, K `modes` |
| Mouse | The tracking modes (DEC modes 1000, 1002, 1003) and the encodings (DEC modes 1005, 1006). An encoding is a mode of its own, also without a tracking mode | G `modes`, K `modes` |
| Focus reporting | DEC mode 1004. RIS resets it | G `modes`, K `modes` |
| Kitty keyboard protocol | Push, pop and set of the five flags, and a separate stack for the alternate screen | G `modes` |
| Key modes | Application cursor keys (DECCKM) and the application keypad | G `modes` |
| Cursor look | Visible or hidden (DECTCEM), and the shape and the blinking (DECSCUSR) | G `modes`, R |
| Full reset | RIS gives the modes of a new terminal | G `modes`, C |
| OSC for the caller | The caller gets each OSC sequence that the library does not handle, for example OSC 9, 133, 777 and 7501 | O |
| Program reports | `Osc::record` parses the program status (OSC 7501), the prompt marks (OSC 133) and the notifications (OSC 9, OSC 777) | The unit tests of `src/osc` |
| History | `Terminal::with_history` keeps the rows that scroll out above the primary screen, up to a limit, with the cells and the styles. `Terminal::new` keeps no history. A resize reflows the primary screen and the history when the number of columns changes, and moves rows between the screen and the history when the number of rows changes. ED 3 clears the history. The alternate screen has no history | H |
| Redraw sequence | The cells, the cursor, the modes, the kitty keyboard flags and the style of the next character. With the alternate screen, also the screen behind it | R |

The library also handles the OSC sequences for the title, the palette colors, the hyperlinks (OSC 8) and the clipboard (OSC 52). `nomux-term` has no interface for them yet.

### Known gaps

| Gap | Result | Reference |
|---|---|---|
| DEC modes 47 and 1047 | The library has the alternate screen only for DEC mode 1049. The text of the program goes to the primary screen. Programs that use the `xterm` terminal description use mode 1049 | G `alternate_screen`, C |
| Kitty graphics | The library ignores the sequences. No picture is kept | ADR-0003 |
| Sixel | The library ignores the sequences. No picture is kept | ADR-0003 |
| Grapheme clusters | An emoji that a zero width joiner connects to a second emoji uses two cells for each emoji. The emoji variation selector does not make a character wide | G `wide_and_combining` |
| A terminal of 1 column | `Size::new` refuses fewer than 2 columns: the library cannot keep a wide character in 1 column. The holder refuses such a window | [`tests/size.rs`](tests/size.rs) |
| The wrap after a backward tab | After a character in the last column, a backward tab (CBT) keeps the wrap of the library: a next character in the same call of `Terminal::feed` goes to the next row. `nomux-term` ends the wrap at the end of each call | G `tab_stops`, R |
| Synchronized updates | `nomux-term` turns off DEC mode 2026: each byte changes the screen immediately | ADR-0003, K `modes` |
| CUU and CUD in the scroll region | The cursor stops at the first or the last row of the screen, not at the margin | K `cursor_and_margins` |
| Line feed after a character in the last column | LF keeps the pending wrap: the next character goes to the first column of the row after the next row, not to the last column of the next row. A carriage return before the line feed ends the pending wrap | K `autowrap`, G `cursor_motion` |
| DCH with a large count | A count larger than the cells from the cursor to the end of the row also erases the cells before the cursor | K `insert_and_delete` |
| Reset of DECOM, DECALN | The cursor stays. xterm moves it to the first cell | K `scroll_region`, K `erase` |
| DECSC and DECOM | DECSC does not save the origin mode, thus DECRC does not restore it | K `cursor_and_margins` |
| DEC mode 1048 | Not handled: the cursor is not saved and not restored | K `cursor_and_margins` |
| DECSEL, DECSED | Not handled: the text stays | K `erase` |
| DECSTR | Not handled: the modes stay | K `modes` |
| DECSCNM | Not handled: no reverse video of the screen | K `modes` |
| SGR 21, 5 and 53 | Not kept: double underline with SGR 21 (SGR `4:2` works), blink and overline | K `sgr` |
| Mouse modes 9 and 1015 | Not handled: a client sends no X10 mouse report and no urxvt encoding | K `modes` |
| Answers to queries | `nomux-term` answers no query: device attributes (DA1, DA2), DECRQM, the device status and the cursor position (DSR), the kitty keyboard flags (`CSI ? u`), the colors (OSC 10, 11, 12), the text area size (`CSI 18 t`, `CSI 14 t`), XTVERSION and DECRQSS. A program that waits for an answer waits until its timeout | K `queries`, [#89](https://github.com/nomux-sh/nomux/issues/89) |

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

The conformance cases are data: [`tests/golden/conformance.rs`](tests/golden/conformance.rs) has the format and runs each case two times, with all the output in one call of `Terminal::feed` and with one byte in each call. The test prints each case that fails. A case of a known gap fails also when the screen of `nomux-term` becomes the screen of xterm: then remove the `gap` and `library` lines of the case and the line of the gap in this file.
