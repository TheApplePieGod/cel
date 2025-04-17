use std::collections::VecDeque;

use either::Either;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{CellContent, Cursor, Margin, ScreenBuffer, ScreenBufferElement, ScreenBufferLine, SignedCursor, StyleState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferCursor(pub [usize; 2]);

#[derive(Clone)]
pub struct TerminalGrid {
    pub screen_buffer: ScreenBuffer,
    pub cursor: Cursor,
    pub width: usize,
    pub height: usize,
    pub margin: Margin,
    pub autowrap: bool,
    pub wants_wrap: bool,
    pub max_scrollback: usize,
}

impl TerminalGrid {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            screen_buffer: Default::default(),
            cursor: [0, 0],
            width,
            height,
            margin: Margin::from_dimensions(width, height),
            autowrap: true,
            wants_wrap: false,
            max_scrollback: usize::MAX
        }
    }

    pub fn clear(&mut self) {
        self.screen_buffer.clear();
        self.cursor = [0, 0];
    }

    pub fn resize(&mut self, width: usize, height: usize, reflow: bool, fill_height: bool) {
        let buf_cursor = self.get_buffer_cursor(&self.cursor);

        let old_width = self.width;
        self.width = width.max(1);
        self.height = height.max(1);

        // TODO: relative margins. For now, reset
        self.margin = Margin::from_dimensions(self.width, self.height);

        if reflow && self.width != old_width {
            self.reflow(buf_cursor);
        }

        if fill_height {
            self.ensure_cursor_line(&self.get_buffer_cursor(&[0, self.height - 1]));
        }
    }

    /// Prints a character at the current cursor position. Performs complex logic
    /// and may mutate cursor.
    pub fn print_char(&mut self, c: char, style: &StyleState) {
        // Ensure wrap is not set if autowrap is disabled
        self.wants_wrap &= self.autowrap;

        // Handle wrapping only when we place a character
        if self.wants_wrap && self.autowrap {
            self.wants_wrap = false;
            self.cursor[0] = 0;
            self.push_cursor_vertically(true);
            self.get_cell(self.cursor).is_wrap = true;
        }

        // Put char at the current position and advance if necessary
        let num_advances = self.put_char_at_cursor(c, style);
        for adv in 0..num_advances {
            // Handle wrapping only when we place a character
            if self.wants_wrap && self.autowrap {
                self.wants_wrap = false;
                self.cursor[0] = 0;
                self.push_cursor_vertically(true);
                self.get_cell(self.cursor).is_wrap = true;
            }

            // Check for wrap. If we want to wrap, update the state accordingly. Otherwise,
            // update the cursor directly
            let wrap = self.cursor[0] + 1 >= self.width;
            if wrap {
                // Even if autowrap is disabled, don't advance the cursor past the screen
                self.wants_wrap = self.autowrap;
            } else {
                // Advance the cursor
                self.cursor[0] += 1;
            }

            log::trace!(
                "Print {:?} (adv {}) {}", c,
                adv,
                match wrap && self.autowrap {
                    true => "<NEXT WRAP>",
                    false => ""
                },
            );
        }

        if num_advances == 0 {
            log::trace!("Print {:?} <APPEND>", c);
        }
    }

    /// Moves the cursor backward by one, accounting for backwrapping
    pub fn move_backward(&mut self) {
        if self.wants_wrap {
            self.wants_wrap = false;
        } else {
            if self.cursor[0] > 0 {
                self.cursor[0] -= 1;
            } else if self.cursor[1] > 0 {
                self.cursor[0] = self.width - 1;
                self.cursor[1] -= 1;
            }
        }
    }

    /// Gets the buffer line at the provided cursor, appending empty lines if necessary
    pub fn get_line(&mut self, cursor: Cursor) -> &mut ScreenBufferLine {
        let cursor = self.get_buffer_cursor(&cursor);
        self.ensure_cursor_line(&cursor);
        &mut self.screen_buffer[cursor.0[1]]
    }

    /// Gets the buffer line at the provided cursor, if it exists
    pub fn get_line_opt(&self, cursor: Cursor) -> Option<&ScreenBufferLine> {
        let cursor = self.get_buffer_cursor(&cursor);
        if self.line_exists(&cursor) {
            Some(&self.screen_buffer[cursor.0[1]])
        } else {
            None
        }
    }

    /// Gets the cell at the provided cursor, appending empty lines and cells if necessary
    pub fn get_cell(&mut self, cursor: Cursor) -> &mut ScreenBufferElement {
        let buf_cursor = self.get_buffer_cursor(&cursor);
        self.ensure_cursor_cell(&buf_cursor);
        let line = self.get_line(cursor);
        &mut line[buf_cursor.0[0]]
    }

    /// Gets the cell at the provided cursor, if it exists
    pub fn get_cell_opt(&self, cursor: Cursor) -> Option<&ScreenBufferElement> {
        let buf_cursor = self.get_buffer_cursor(&cursor);
        let line = self.get_line_opt(cursor)?;
        if self.cell_exists(&buf_cursor) {
            Some(&line[buf_cursor.0[0]])
        } else {
            None
        }
    }

    /// Get the cursor from a position relative to the current cursor, clamped
    pub fn get_cursor_relative(&self, cursor: Cursor, offset: SignedCursor) -> Cursor {
        let relative = [
            (cursor[0] as isize + offset[0]).max(0) as usize,
            (cursor[1] as isize + offset[1]).max(0) as usize
        ];

        self.clamp_cursor(relative)
    }

    /// Get the maximal cursor based on the current dimensions
    pub fn get_cursor_max(&self) -> Cursor {
        [self.width - 1, self.height - 1]
    }

    /// Gets the first buffer line index of the current screen
    pub fn get_top_index(&self) -> usize {
        self.screen_buffer.len().saturating_sub(self.height)
    }

    /// Get the buffer-indexed cursor for the provided cursor 
    pub fn get_buffer_cursor(&self, cursor: &Cursor) -> BufferCursor {
        let cursor = self.clamp_cursor(*cursor);
        BufferCursor([cursor[0], cursor[1] + self.get_top_index()])
    }

    /// Gets the length of the entire screen buffer
    pub fn get_buffer_len(&self) -> usize {
        self.screen_buffer.len()
    }

    /// Gets the length of the entire screen buffer, including the cursor if it falls
    /// below the buffer
    pub fn get_buffer_len_with_cursor(&self) -> usize {
        let buf_cursor = self.get_buffer_cursor(&self.cursor);
        self.get_buffer_len().max(buf_cursor.0[1] + 1)
    }

    /// Set the current cursor position
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
        self.wants_wrap = false;
    }

    /// Set the current cursor position given a buffer cursor, potentially extending the top index
    pub fn set_buf_cursor(&mut self, cursor: BufferCursor) {
        self.ensure_cursor_line(&cursor);
        self.cursor[0] = cursor.0[0];
        self.cursor[1] = cursor.0[1].saturating_sub(self.get_top_index());
    }

    /// Set the current margins
    pub fn set_margin(&mut self, margin: Margin) {
        self.margin = margin;
    }

    /// Set the current horizontal margins
    pub fn set_horizontal_margin(&mut self, left: usize, right: usize) {
        self.margin.left = left;
        self.margin.right = right;
    }

    /// Set the current vertical margins
    pub fn set_vertical_margin(&mut self, top: usize, bottom: usize) {
        self.margin.top = top;
        self.margin.bottom = bottom;
    }

    pub fn set_max_scrollback(&mut self, max_scrollback: usize) {
        self.max_scrollback = max_scrollback;
    }

    /// Test whether the supplied cursor is within currently set margins
    pub fn is_in_margins(&self, cursor: Cursor) -> bool {
        return cursor[0] >= self.margin.left
               && cursor[0] <= self.margin.right
               && cursor[1] >= self.margin.top
               && cursor[1] <= self.margin.bottom;
    }

    /// Computes the cursor for the start of the current line.
    /// Does not respect margins.
    pub fn get_cursor_sol(&self, cursor: Cursor) -> Cursor {
        [0, cursor[1]]
    }

    /// Computes the cursor for the end of the current line.
    /// Does not respect margins.
    pub fn get_cursor_eol(&self, cursor: Cursor) -> Cursor {
        [self.width - 1, cursor[1]]
    }

    /// Computes the cursor directly above the supplied cursor
    pub fn get_cursor_prev_line(&self, cursor: Cursor) -> Cursor {
        [cursor[0], cursor[1].saturating_sub(1)]
    }

    /// Computes the cursor directly below the supplied cursor
    pub fn get_cursor_next_line(&self, cursor: Cursor) -> Cursor {
        [cursor[0], (cursor[1] + 1).min(self.height - 1)]
    }

    /// Clamp cursor to dimensions
    pub fn clamp_cursor(&self, cursor: Cursor) -> Cursor {
        [
            cursor[0].min(self.width - 1),
            cursor[1].min(self.height - 1)
        ]
    }

    /// Check if the cursor is in bounds of the existing buffer data
    pub fn line_exists(&self, buf_cursor: &BufferCursor) -> bool {
        buf_cursor.0[1] < self.screen_buffer.len()
    }

    /// Check if the cursor is in bounds of the existing buffer data
    pub fn cell_exists(&self, buf_cursor: &BufferCursor) -> bool {
        self.line_exists(buf_cursor) && buf_cursor.0[0] < self.screen_buffer[buf_cursor.0[1]].len()
    }

    /// Ensures there are enough lines in the buffer to support the provided cursor position
    pub fn ensure_cursor_line(&mut self, buf_cursor: &BufferCursor) {
        if buf_cursor.0[1] >= self.screen_buffer.len() {
            self.screen_buffer.resize(buf_cursor.0[1] + 1, vec![]);
        }
    }

    /// Ensures there are enough lines and chars in the buffer to support the provided cursor position
    pub fn ensure_cursor_cell(&mut self, buf_cursor: &BufferCursor) {
        self.ensure_cursor_line(buf_cursor);
        let line = &mut self.screen_buffer[buf_cursor.0[1]];
        if buf_cursor.0[0] >= line.len() {
            line.resize(buf_cursor.0[0] + 1, Default::default());
        }
    }

    /// Physically insert/remove 'amount' from into the screen buffer at the specified cursor,
    /// going downward
    pub fn insert_or_remove_lines(&mut self, cursor: Cursor, amount: u32, remove: bool) {
        if !self.is_in_margins(cursor) {
            return;
        }

        // Implement removal in terms of scrolling as that is the expected behavior
        let mut margin = self.margin;
        margin.top = cursor[1];
        for _ in 0..amount {
            self.scroll_region(remove, margin);
        }
    }

    /// Physically delete 'amount' cells from the screen buffer at the specified cursor,
    /// going to the right within the line
    pub fn delete_cells(&mut self, cursor: Cursor, amount: u32) {
        let cursor = self.get_buffer_cursor(&cursor);
        if !self.cell_exists(&cursor) {
            return; 
        }

        let offset = cursor.0[0];
        let line = &mut self.screen_buffer[cursor.0[1]];
        let range = offset..((offset + amount as usize).min(line.len()));
        line.drain(range);
    }

    /// Scroll within the current margin up or down by one line. Up 
    /// refers to the direction the text is moving
    pub fn scroll(&mut self, up: bool) {
        self.scroll_region(up, self.margin);
    }

    /// Push the cursor down or up onto the next line. Will scroll the region if on the
    /// margin boundary.
    pub fn push_cursor_vertically(&mut self, down: bool) {
        self.wants_wrap = false;
        if down && self.cursor[1] < self.margin.bottom {
            self.cursor[1] += 1;
        } else if !down && self.cursor[1] > self.margin.top {
            self.cursor[1] -= 1;
        } else {
            self.scroll_region(down, self.margin)
        }
    }

    /// Erases content between two cursors.
    pub fn erase(&mut self, start: Cursor, end: Cursor) {
        let mut start = start;
        let mut end = end;

        // Ensure end is after start
        if (start[1] == end[1] && end[0] < start[0]) || start[1] > end[1] {
            (start, end) = (end, start);
        }

        let start = self.get_buffer_cursor(&start);
        let end = self.get_buffer_cursor(&end);
        for y in start.0[1]..=end.0[1] {
            if y >= self.screen_buffer.len() {
                break;
            }

            let line = &mut self.screen_buffer[y];
            let mut start_x = 0;
            let mut end_x = line.len();

            if y == start.0[1] {
                start_x = start.0[0];
            }
            if y == end.0[1] {
                end_x = (end.0[0] + 1).min(end_x);
            }

            for x in start_x..end_x {
                line[x] = Default::default();
            }
        }
    }

    /// Gets the raw text contained in the screen buffer
    pub fn get_text(&self) -> String {
        let mut raw_lines = vec![];

        for buf_line in &self.screen_buffer {
            let mut line = String::new();
            for elem in buf_line {
                match &elem.elem {
                    CellContent::Char(c, _) => line.push(*c),
                    CellContent::Grapheme(s, _) => line.push_str(&s),
                    CellContent::Continuation(_) => {}
                    CellContent::Empty => line.push(' ')
                }
            }
            raw_lines.push(line);
        }

        raw_lines.join("\n")
    }

    /// Scroll the specified screenspace buffer region up or down by one line. Up 
    /// refers to the direction the text is moving
    fn scroll_region(&mut self, up: bool, margin: Margin) {
        // Only support default scrollback behavior if we don't have any margin.
        // Otherwise, physically scroll the buffer in memory when adding new characters
        let support_scrollback = margin.top == 0
                                 && margin.left == 0
                                 && margin.bottom == self.height - 1
                                 && margin.right == self.width - 1;

        if support_scrollback {
            // Scroll the region with scrollback by pushing/popping

            if up {
                let full = self.screen_buffer.len() >= self.max_scrollback;
                if full {
                    // Ensure we do not exceed capacity
                    self.screen_buffer.pop_front();
                }
                self.screen_buffer.push_back(vec![]);
            } else {
                self.screen_buffer.pop_back();
            }
        } else {
            let region_cursor_top = self.get_buffer_cursor(&[margin.left, margin.top]);
            let region_cursor_bot = self.get_buffer_cursor(&[margin.left, margin.bottom]);

            let evict_pos = match up {
                true => region_cursor_top,
                false => region_cursor_bot
            };
            let replace_pos = match up {
                true => region_cursor_bot,
                false => region_cursor_top
            };

            // We can directly edit the buffer line only if there is no x margin and the 
            let region_size_x = margin.right - margin.left;
            let can_trim_lines = region_size_x == self.width - 1;
            if can_trim_lines {
                if evict_pos.0[1] >= self.screen_buffer.len() {
                    return;
                }

                self.screen_buffer.remove(evict_pos.0[1]);

                if replace_pos.0[1] < self.screen_buffer.len() {
                    self.screen_buffer.insert(replace_pos.0[1], vec![]);
                }
            } else {
                // Perform simulated scrolling in the margins by replacing the contents of each line
                // in the scrolling region with the next or prev depending on direction,
                // erasing the final line at the end

                todo!("Scroll region with x margin");
            }
        }
    }

    // Replaces cell content at cursor position, accounting for inserting and removing continuation cells.
    // This assumes that pos is not out of bounds, and the content at pos is NOT empty or a continuation
    fn put_wide_content_unchecked(&mut self, pos: Cursor, style: StyleState, content: Either<String, char>) -> usize {
        let cell = &mut self.get_cell(pos);
        let new_width = match &content {
            Either::Left(str) => str.width(),
            Either::Right(c) => c.width().unwrap_or(1)
        }.max(1);
        let old_width = match cell.elem {
            CellContent::Char(_, w) => w,
            CellContent::Grapheme(_, w) => w,
            _ => 1
        };
        let continuations_pos = [pos[0] + 1, pos[1]];
        cell.style = style;
        cell.elem = match content {
            Either::Left(str) => CellContent::Grapheme(str, new_width),
            Either::Right(c) => CellContent::Char(c, new_width)
        };
        self.update_continuations(continuations_pos, style, old_width - 1, new_width - 1, 0);

        new_width
    }

    /// Place a character at the current cursor. Handles wide characters and continuations
    /// May modify the cursor.
    fn put_char_at_cursor(&mut self, c: char, style: &StyleState) -> usize {
        // TODO: definitely still lots of edge cases in here when dealing with
        // continuations. Wrapping behavior still unclear, and splitting lines
        // will break continuatinos
        let cur_pos = self.cursor;
        let cell = self.get_cell(cur_pos);

        match cell.elem {
            // Mutate cursor and navigate to start of character. This makes behavior
            // much more predictable and easy to implement
            CellContent::Continuation(width) => {
                // TODO: this sucks, also won't work if a cell is over two lines
                let diff = self.cursor[0] + self.width - width;
                self.cursor[0] = diff % self.width;
                if diff < self.width {
                    self.cursor = self.get_cursor_prev_line(self.cursor);
                }
            },
            _ => {}
        };

        let cur_pos = self.cursor;
        let cur_style = style;

        // Fast-path: if the new char is ASCII, skip grapheme merging
        if c.is_ascii() {
            return self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c));
        }

        let line = self.get_line(cur_pos);
        if cur_pos[0] > 0 {
            // Get the previous cell, accounting for continuations
            let last_pos = cur_pos[0] - 1;
            let last_pos = match &line[last_pos].elem {
                CellContent::Continuation(width) => [last_pos - width, cur_pos[1]],
                _ => [last_pos, cur_pos[1]]
            };
            let last_cell = &mut line[last_pos[0]];
            let last_style = last_cell.style;
            match &mut last_cell.elem {
                CellContent::Char(old_c, old_width) => {
                    let mut buf = [0; 10];
                    let len1 = old_c.encode_utf8(&mut buf).len();
                    let len2 = c.encode_utf8(&mut buf[len1..]).len();
                    let str = std::str::from_utf8(&buf[..len1 + len2]).unwrap();
                    match str.graphemes(true).count() {
                        0..=1 => {
                            let new_str = str.to_string();
                            let old_width = *old_width;
                            let new_width = self.put_wide_content_unchecked(last_pos, last_style, Either::Left(new_str));
                            new_width.saturating_sub(old_width)
                        },
                        _ => {
                            self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c))
                        }
                    }
                }
                CellContent::Grapheme(str, old_width) => {
                    // Temp mutate to check graphemes
                    str.push(c);
                    match str.graphemes(true).count() {
                        0..=1 => {
                            // TODO: dont need to clone since we mutated
                            let new_str = str.clone();
                            let old_width = *old_width;
                            let new_width = self.put_wide_content_unchecked(last_pos, last_style, Either::Left(new_str));
                            new_width.saturating_sub(old_width)
                        },
                        _ => {
                            str.pop();
                            self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c))
                        }
                    }
                }
                CellContent::Continuation(_) => {
                    debug_assert!(false, "BUG! This should never happen.");
                    self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c))
                }
                CellContent::Empty => {
                    self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c))
                }
            }
        } else {
            self.put_wide_content_unchecked(cur_pos, *cur_style, Either::Right(c))
        }
    }

    fn update_continuations(&mut self, pos: Cursor, style: StyleState, old_amount: usize, amount: usize, start_index: usize) {
        if amount == 0 && old_amount == 0 {
            // Don't do range check insertions if we aren't actually inserting anything
            return;
        }

        let width = self.width;
        let line = self.get_line(pos);

        // TODO: handle insert mode
        if false {
            // In insert mode, we insert new cells
            // (This shifts existing cells to the right)
            for i in 0..amount {
                let elem = ScreenBufferElement {
                    style,
                    elem: CellContent::Continuation(start_index + amount - i),
                    is_wrap: false
                };
                line.insert(pos[0], elem);
            }
        } else {
            // Otherwise, we update the existing cells in place
            let mut old_amount = old_amount;
            for i in 0..amount {
                let idx = pos[0] + i;
                let new_cell = ScreenBufferElement {
                    style,
                    elem: CellContent::Continuation(start_index + amount - i),
                    is_wrap: false
                };
                if idx < line.len() {
                    // Ensure that any intersecting graphemes also have their continuations
                    // cleared. Exploit old_amount to achieve this
                    match line[idx].elem {
                        CellContent::Char(_, w) |
                        CellContent::Grapheme(_, w) => old_amount = amount.max(i + w),
                        _ => {}
                    };
                    line[idx] = new_cell;
                } else if line.len() < width {
                    // TODO: this is OK because we ensure not to add past the grid width.
                    // Need a safer way to do this, also this is incorrect and continuations
                    // are not allowed to wrap
                    line.push(new_cell);
                }
            }
            // If the previous wide character had more continuation cells than we need now,
            // clear the extra cells by marking them as empty.
            if old_amount > amount {
                for i in amount..old_amount {
                    let idx = pos[0] + i;
                    if idx < line.len() {
                        line[idx].elem = CellContent::Empty;
                    }
                }
            }
        }
    }

    /// Reflow the current grid to fit within the current width, wrapping where possible
    /// Takes in the current buffer cursor BEFORE the resize was performed
    fn reflow(&mut self, buf_cursor: BufferCursor) {
        // First: reconstruct logical rows from the grown grid.
        // We assume that a new logical row starts when either we are at the very first line
        // or when the first cell is not marked as wrapped.
        let old_buffer = std::mem::take(&mut self.screen_buffer);
        let old_len = old_buffer.len();
        let mut logical_rows: Vec<ScreenBufferLine> = Vec::with_capacity(old_len);
        let mut current_logical: Option<ScreenBufferLine> = None;
        let mut cursor_abs_index = None;

        for (i, mut line) in old_buffer.into_iter().enumerate() {
            let line_len = line.len();

            // A new logical row starts if either current_logical is empty or the line’s first cell is not wrapped.
            if current_logical.is_none() || line.is_empty() || (!line.is_empty() && !line[0].is_wrap) {
                if let Some(row) = current_logical.take() {
                    // Record the starting offset for this logical row.
                    logical_rows.push(row);
                }

                current_logical = Some(line);
            } else {
                // In the grow algorithm a wrapped line had part of its cells moved into the previous line.
                // Here we “unmerge” by moving the first cell from this continuation line back to the end
                // of the current logical row.
                let cur = current_logical.as_mut().unwrap();
                let mut first_cell = line.remove(0);
                first_cell.is_wrap = false;
                cur.push(first_cell);
                cur.extend(line);
            }

            let cursor_on_line = buf_cursor.0[1] == i;
            if cursor_on_line {
                let offset = current_logical.as_ref().unwrap().len() - line_len;
                cursor_abs_index = Some((logical_rows.len(), buf_cursor.0[0] + offset));
            }
        }

        // Finalize any trailing logical row.
        if let Some(row) = current_logical.take() {
            logical_rows.push(row);
        }

        // Next: reflow each logical row into new lines not exceeding self.width.
        // Also update the cursor position if it fell into a logical row.
        let mut new_buffer: ScreenBuffer = VecDeque::with_capacity(old_len);
        let mut new_cursor: Option<BufferCursor> = None;

        for (logical_idx, logical) in logical_rows.into_iter().enumerate() {
            let cursor_in_line = match cursor_abs_index {
                Some((line, _)) => line == logical_idx,
                None => false
            };

            // Ensure to include empty lines
            let mut first_subline = true;
            let mut col = 0;
            while col < logical.len() || (first_subline && logical.is_empty()) {
                let remaining = logical.len() - col;
                let take = remaining.min(self.width);
                let mut subline: Vec<ScreenBufferElement> = logical[col..col + take].to_vec();

                // In our reflow, the very first subline of a logical row is not marked as wrapped.
                // All subsequent sublines are.
                if !subline.is_empty() {
                    if !first_subline {
                        subline[0].is_wrap = true;
                    } else {
                        subline[0].is_wrap = false;
                    }
                }

                // If the cursor is in this logical row, check if its local offset falls into this subline.
                if cursor_in_line {
                    let (_, offset) = cursor_abs_index.unwrap();
                    if offset >= col && offset < col + take {
                        new_cursor = Some(BufferCursor([offset - col, new_buffer.len()]));
                    }
                }

                new_buffer.push_back(subline);
                col += take;
                first_subline = false;
            }

            // If the cursor is in this logical row but not on any specific character,
            // handle that here
            if cursor_in_line && new_cursor.is_none() {
                let (_, offset) = cursor_abs_index.unwrap();
                let offset = offset - logical.len() + new_buffer.back().unwrap_or(&vec![]).len();
                new_cursor = Some(BufferCursor([offset.min(self.width - 1), new_buffer.len().saturating_sub(1)]));
            }
        }

        // Finally update the screen buffer and cursor.
        self.screen_buffer = new_buffer;
        if let Some(cursor) = new_cursor {
            self.set_buf_cursor(cursor);
        } else {
            // Cursor was out of bounds, so special case handling
            // TODO:
            //let y = buf_cursor.0[1].
        }
    }
}
