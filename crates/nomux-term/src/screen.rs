//! Read access to the visible screen of a terminal.

use std::fmt;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;

use crate::cell::{Cell, CellWidth, Style};
use crate::conversion;
use crate::cursor::{Cursor, Position};
use crate::modes::Modes;
use crate::size::Size;

/// The visible screen of a [`Terminal`](crate::Terminal): the cells, the
/// cursor and the modes.
#[derive(Clone, Copy)]
pub struct Screen<'a> {
    term: &'a Term<VoidListener>,
    size: Size,
}

impl<'a> Screen<'a> {
    pub(crate) fn new(term: &'a Term<VoidListener>, size: Size) -> Self {
        Self { term, size }
    }

    /// The size of the screen.
    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    /// The cell at `position`, or `None` if the position is not on the screen.
    ///
    /// The VT library can keep one cell of a wide character without the
    /// other cell, for example after a delete character on the first cell.
    /// No terminal can show a half of a character, thus such a cell is an
    /// empty cell that keeps its style.
    #[must_use]
    pub fn cell(&self, position: Position) -> Option<Cell<'a>> {
        let cell = self.library_cell(position)?;
        let other_half = match cell.width {
            CellWidth::Single => return Some(cell),
            CellWidth::Double => (position.column.checked_add(1), CellWidth::Continuation),
            CellWidth::Continuation => (position.column.checked_sub(1), CellWidth::Double),
        };
        let (column, width) = other_half;
        let other_cell = column.and_then(|column| {
            self.library_cell(Position {
                row: position.row,
                column,
            })
        });
        if other_cell.is_some_and(|other_cell| other_cell.width == width) {
            Some(cell)
        } else {
            Some(cell.without_character())
        }
    }

    /// The cell as the VT library keeps it.
    fn library_cell(&self, position: Position) -> Option<Cell<'a>> {
        if position.row >= self.size.rows() || position.column >= self.size.columns() {
            return None;
        }
        let row = &self.term.grid()[Line(i32::from(position.row))];
        Some(conversion::cell(&row[Column(usize::from(position.column))]))
    }

    /// The cursor.
    #[must_use]
    pub fn cursor(&self) -> Cursor {
        let point = self.term.grid().cursor.point;
        let style = self.term.cursor_style();
        Cursor {
            position: Position {
                row: u16::try_from(point.line.0).unwrap_or(0),
                column: u16::try_from(point.column.0).unwrap_or(0),
            },
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR),
            shape: conversion::cursor_shape(style.shape),
            blinking: style.blinking,
        }
    }

    /// The modes that are set.
    #[must_use]
    pub fn modes(&self) -> Modes {
        conversion::modes(*self.term.mode())
    }

    /// The style that the next character gets.
    pub(crate) fn pen(&self) -> Style {
        conversion::style(&self.term.grid().cursor.template)
    }

    /// `true` if the cursor is in the last column after a character was
    /// written there: the next character goes to the next row.
    pub(crate) fn wrap_pending(&self) -> bool {
        self.term.grid().cursor.input_needs_wrap
    }
}

impl fmt::Debug for Screen<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Screen")
            .field("size", &self.size)
            .field("cursor", &self.cursor())
            .field("modes", &self.modes())
            .finish_non_exhaustive()
    }
}
