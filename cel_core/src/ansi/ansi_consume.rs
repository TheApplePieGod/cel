use bitflags::Flags;
use unicode_segmentation::{UnicodeSegmentation, GraphemeCursor};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vte::{Parser, Params, Perform};
use std::fmt;

use super::*;

impl AnsiHandler {
    pub fn new(width: u32, height: u32) -> Self {
        let mut obj = Self {
            performer: Performer::new(width, height),
            state_machine: Parser::new(),

            mouse_states: [(Default::default(), false); 256],
            scroll_states: [0.0; 2]
        };

        obj.resize(width, height);

        obj
    }

    pub fn handle_byte(&mut self, byte: u8) {
        self.state_machine.advance(&mut self.performer, byte);
    }

    pub fn handle_sequence_bytes(
        &mut self,
        seq: &[u8],
        stop_early: bool
    ) -> Option<u32> {
        self.performer.action_performed = false;
        for (i, c) in seq.iter().enumerate() {
            //print!("{:?} ", *c as char);
            self.state_machine.advance(&mut self.performer, *c);
            if stop_early && self.performer.action_performed {
                return Some(i as u32)
            }
        }

        None
    }

    pub fn handle_scroll(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        flags: KeyboardModifierFlags,
        cell_position: &Cursor
    ) {
        self.scroll_states[0] += delta_x;
        self.scroll_states[1] += delta_y;

        // Horizontal scroll
        if self.scroll_states[0].abs() >= 1.0 {
            let left = self.scroll_states[0] > 0.0;
            let button = match left {
                true => MouseButton::Mouse6,
                false => MouseButton::Mouse7,
            };

            // Toggle
            self.handle_mouse_button(button, true, flags, cell_position);
            self.handle_mouse_button(button, false, flags, cell_position);
        }

        // Vertical scroll
        if self.scroll_states[1].abs() >= 1.0 {
            let up = self.scroll_states[1] > 0.0;
            let button = match up {
                true => MouseButton::Mouse4,
                false => MouseButton::Mouse5,
            };

            // Toggle
            self.handle_mouse_button(button, true, flags, cell_position);
            self.handle_mouse_button(button, false, flags, cell_position);
        }

        self.scroll_states[0] = self.scroll_states[0].fract();
        self.scroll_states[1] = self.scroll_states[1].fract();
    }

    pub fn handle_mouse_button(
        &mut self,
        button: MouseButton,
        press: bool,
        flags: KeyboardModifierFlags,
        cell_position: &Cursor // 0-indexed, (0,0) in top left
    ) {
        let term_state = &mut self.get_terminal_state();
        let mouse_state = &self.mouse_states[button as usize];
        match term_state.mouse_tracking_mode {
            MouseTrackingMode::Disabled => return,
            MouseTrackingMode::Default => {
                // Only send signal on state change
                if mouse_state.1 == press {
                    return;
                }
            },
            MouseTrackingMode::ButtonEvent => {
                // Send signal on state or cell change
                if mouse_state.1 == press && (!press || mouse_state.0 == *cell_position) {
                    return;
                }
            },
            // TODO: tracking when no buttons are pressed
            MouseTrackingMode::AnyEvent => {},
        }

        let mut sequence: Vec<u8> = vec![0x1b, b'[']; // TODO: reuse
        match term_state.mouse_mode {
            MouseMode::Default => {
                let cx = cell_position[0].min(222) as u32 + 32 + 1;
                let cy = cell_position[1].min(222) as u32 + 32 + 1;
                let mut cb = flags.bits() | match press {
                    true => button as u32,
                    false => 3 // Release
                } + 32;

                // Motion tracking events
                if press == mouse_state.1 {
                    cb += 32;
                }

                sequence.push(b'M');
                sequence.push(cb as u8);
                sequence.push(cx as u8);
                sequence.push(cy as u8);
            },
            MouseMode::UTF8 => { /* TODO */ },
            MouseMode::SGR => {
                let mut cb = flags.bits() | button as u32;
                let cx = cell_position[0] as u32 + 1;
                let cy = cell_position[1] as u32 + 1;

                // Motion tracking events
                if press == mouse_state.1 {
                    cb += 32;
                }

                sequence.push(b'<');
                sequence.extend(self.convert_num_to_ascii(cb));
                sequence.push(b';');
                sequence.extend(self.convert_num_to_ascii(cx));
                sequence.push(b';');
                sequence.extend(self.convert_num_to_ascii(cy));
                sequence.push(b';');
                sequence.push(match press {
                    true => b'M',
                    false => b'm'
                });


                //log::warn!("Sending: {}, {} [{}]", cx, cy, press);
                //log::warn!("Sending: {:?}", sequence);
            }
        }

        self.mouse_states[button as usize] = (*cell_position, press);
        self.performer.output_stream.extend(sequence);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // TODO: realtive margin update?
        self.performer.screen_width = width as usize;
        self.performer.screen_height = height as usize;
        self.performer.terminal_state.margin = Margin::get_from_screen_size(width, height);
    }

    pub fn get_terminal_state(&self) -> &TerminalState {
        &self.performer.terminal_state
    }

    pub fn consume_output_stream(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.performer.output_stream)
    }

    pub fn is_empty(&self) -> bool {
        self.performer.is_empty
    }

    pub fn is_alt_screen_buf_active(&self) -> bool {
        self.performer.terminal_state.alt_screen_buffer_state == BufferState::Active
    }

    pub fn reset(&mut self) {
        self.performer.terminal_state = Default::default();
        self.resize(self.performer.screen_width as u32, self.performer.screen_height as u32);
    }

    // TODO: utilize producer ?

    pub fn hide_cursor(&mut self) {
        self.performer.terminal_state.cursor_state.visible = false;
    }

    pub fn set_terminal_color(&mut self, color: &[f32; 3]) {
        self.performer.terminal_state.background_color = *color;
    }

    fn convert_num_to_ascii(&self, val: u32) -> Vec<u8> {
        if val == 0 {
            return vec![b'0'];
        }

        let mut result = vec![];

        let mut cur = val;
        while cur > 0 {
            let digit = cur % 10;
            result.push((digit + 48) as u8);
            cur /= 10;
        }

        result.reverse();
        result
    }
}

impl Performer {
    fn new(width: u32, height: u32) -> Self {
        Self {
            screen_width: width as usize,
            screen_height: height as usize,
            output_stream: vec![],
            is_empty: true,
            action_performed: false,

            terminal_state: Default::default(),
            saved_terminal_state: Default::default(),

            ignore_print: false
        }
    }

    fn parse_params(&self, params: &Params) -> Vec<u16> {
        let mut res: Vec<u16> = Vec::with_capacity(params.len());
        for param in params {
            for code in param {
                res.push(*code)
            }
        }

        res
    }

    fn parse_osc_command(&self, bytes: &[u8]) -> u16 {
        let mut result = 0;

        // Convert each byte to its integer rep from ascii
        let mut power = 1;
        for i in 0..bytes.len() {
            let byte = bytes[bytes.len() - i - 1];
            result += (byte - 48) as u16 * power;
            power *= 10;
        }
        
        result
    }

    // Code is [0, 7], assumes weight is already considered
    fn parse_4_bit_color(style_flags: StyleFlags, code: u16) -> [f32; 3] {
        let factor: f32 = if style_flags.contains(StyleFlags::Bold) {
            1.0
        } else if style_flags.contains(StyleFlags::Faint) {
            0.25
        } else {
            0.5
        };

        let one = (code & 1) as f32 * factor;
        let two = ((code & 2) >> 1) as f32 * factor;
        let four = ((code & 4) >> 2) as f32 * factor;
        match code {
            1..=6 => [one, two, four],
            0 => if style_flags.contains(StyleFlags::Bold) {
                    [0.5, 0.5, 0.5]
                } else if style_flags.contains(StyleFlags::Faint) {
                    [0.25, 0.25, 0.25]
                } else {
                    [0.0, 0.0, 0.0]
                },
            7 => if style_flags.contains(StyleFlags::Bold) {
                    [1.0, 1.0, 1.0]
                } else if style_flags.contains(StyleFlags::Faint) {
                    [0.5, 0.5, 0.5]
                } else {
                    [0.75, 0.75, 0.75]
                },
            _ => [0.0, 0.0, 0.0]
        }
    }

    fn parse_8_bit_color(code: u16) -> [f32; 3] {
        match code {
            0..=7 => Self::parse_4_bit_color(StyleFlags::default(), code),
            8..=15 => Self::parse_4_bit_color(StyleFlags::Bold, code - 8),
            16..=231 => {
                // RGB cube colors
                let base_id = code - 16;
                let r = ((base_id / 36) % 6) as f32 * 0.2;
                let g = ((base_id / 6) % 6) as f32 * 0.2;
                let b = (base_id % 6) as f32 * 0.2;
                [r, g, b]
            },
            232..=255 => {
                // Grayscale ramp
                let gray_value = ((code - 232) as f32) * (1.0 / 24.0);
                [gray_value; 3]
            }
            _ => [0.0, 0.0, 0.0]
        }
    }

    fn parse_rgb_color(rgb: &[u16]) -> [f32; 3] {
        let rgb: [u16; 3] = rgb.try_into().unwrap_or([0, 0, 0]);
        rgb.map(|c| c as f32 / 255.0)
    }

    fn parse_graphics_escape(&mut self, params: &Vec<u16>) {
        // TODO: check that params are in range
        let state = &mut self.terminal_state.style_state;
        let mut extended_mode = 0;
        let mut i = 0;
        while i < params.len() {
            let code = params[i];
            match code {
                0 => *state = Default::default(),
                1 => {
                    state.flags.insert(StyleFlags::Bold);
                    state.flags.remove(StyleFlags::Faint);
                }
                2 => match extended_mode {
                    38 => {
                        state.fg_color = Some(Self::parse_rgb_color(&params[(i + 1)..]));
                        return;
                    },
                    48 => {
                        state.bg_color = Some(Self::parse_rgb_color(&params[(i + 1)..]));
                        return;
                    },
                    _ => {
                        state.flags.insert(StyleFlags::Faint);
                        state.flags.remove(StyleFlags::Bold);
                    }
                },
                3 => state.flags.insert(StyleFlags::Italic),
                4 => state.flags.insert(StyleFlags::Underline),
                5 => match extended_mode {
                    38 if i + 1 < params.len() => {
                        state.fg_color = Some(Self::parse_8_bit_color(params[i + 1]));
                        extended_mode = 0;
                        i += 1;
                    },
                    38 => {}
                    48 if i + 1 < params.len() => {
                        state.bg_color = Some(Self::parse_8_bit_color(params[i + 1]));
                        extended_mode = 0;
                        i += 1;
                    },
                    48 => {}
                    _ => state.flags.insert(StyleFlags::Blink)
                },
                8 => state.flags.insert(StyleFlags::Invisible),
                9 => state.flags.insert(StyleFlags::CrossedOut),
                22 => {
                    state.flags.remove(StyleFlags::Bold);
                    state.flags.remove(StyleFlags::Faint);
                },
                23 => state.flags.remove(StyleFlags::Italic),
                24 => state.flags.remove(StyleFlags::Underline),
                25 => state.flags.remove(StyleFlags::Blink),
                28 => state.flags.remove(StyleFlags::Invisible),
                29 => state.flags.remove(StyleFlags::CrossedOut),
                30..=37 => state.fg_color = Some(Self::parse_4_bit_color(state.flags, code - 30)),
                40..=47 => state.bg_color = Some(Self::parse_4_bit_color(state.flags, code - 40)),
                90..=97   => state.fg_color = Some(Self::parse_4_bit_color(state.flags | StyleFlags::Bold, code - 90)),
                100..=107 => state.bg_color = Some(Self::parse_4_bit_color(state.flags | StyleFlags::Bold, code - 100)),
                38 => extended_mode = 38,
                39 => state.fg_color = None,
                48 => extended_mode = 48,
                49 => state.bg_color = None,
                _ => {}
            }

            i += 1;
        }
    }

    /// Top left position is (0, 0)
    fn compute_new_cursor_pos(
        &self,
        cur_screen: Cursor,
        cur_global: Cursor,
        target_screen: Cursor
    ) -> Cursor {
        let mut cur_screen = cur_screen;
        let mut cur_global = cur_global;
        let state = &self.terminal_state;
        loop {
            // Can figure out exactly where to go if the lines are empty
            if cur_global[1] >= state.screen_buffer.len() {
                return [
                    target_screen[0],
                    cur_global[1] + (target_screen[1] - cur_screen[1])
                ];
            }

            // log::warn!("Line {}", cur_global[1]);

            // TODO: this could definitely be optimized
            let line_count = self.get_remaining_wrapped_line_count(&cur_global) + 1;
            let mut char_idx = cur_global[0];
            for _ in 0..line_count {
                let cur_screen_y = cur_screen[1];

                if cur_screen_y == target_screen[1] {
                    return [
                        (char_idx as i32 + (target_screen[0] as i32 - cur_screen[0] as i32)) as usize,
                        cur_global[1]
                    ];
                }

                char_idx += self.screen_width;
                cur_screen[1] += 1;
            }

            cur_screen[0] = 0;
            cur_global[0] = 0;
            cur_global[1] += 1;
        }
    }

    /// Get the global cursor pos from an absolute screen position
    fn get_cursor_pos_absolute(&self, screen_pos: &Cursor) -> Cursor {
        self.compute_new_cursor_pos(
            [0, 0],
            self.terminal_state.global_cursor_home,
            *screen_pos
        )
    }

    fn set_cursor_pos_absolute(&mut self, screen_pos: &Cursor) {
        let old_screen = self.terminal_state.screen_cursor;
        let old_global = self.terminal_state.global_cursor;

        // TODO: could optimize if the new position is after the old position

        let clamped_pos = self.clamp_screen_cursor(screen_pos);
        self.terminal_state.global_cursor = self.get_cursor_pos_absolute(&clamped_pos);
        self.terminal_state.screen_cursor = clamped_pos;
        self.terminal_state.wants_wrap = false;

        log::debug!(
            "[set_cursor_pos_absolute{:?}] Screen: {:?} -> {:?}, Global: {:?} -> {:?}",
            clamped_pos,
            old_screen, self.terminal_state.screen_cursor,
            old_global, self.terminal_state.global_cursor
        );
    }

    /// Get the global cursor pos from a position relative to the current screen cursor
    fn get_cursor_pos_relative(&self, relative_screen: &SignedCursor) -> Cursor {
        // TODO: optimize better for relative? right now we have to recompute from
        // the origin in order to ensure correctness
        let target = self.get_relative_screen_cursor(relative_screen);
        self.get_cursor_pos_absolute(&target)
    }

    fn set_cursor_pos_relative(&mut self, relative_screen: &SignedCursor) {
        let old_screen = self.terminal_state.screen_cursor;
        let old_global = self.terminal_state.global_cursor;
        let old_wrap = self.terminal_state.wants_wrap;

        let target = self.get_relative_screen_cursor(relative_screen);
        self.terminal_state.global_cursor = self.get_cursor_pos_relative(relative_screen);
        self.terminal_state.screen_cursor = target;
        self.terminal_state.wants_wrap = false;

        /*
        log::debug!(
            "[set_cursor_pos_relative({:?})] Screen: {:?} -> {:?}, Global: {:?} -> {:?} {}",
            relative_screen,
            old_screen, self.terminal_state.screen_cursor,
            old_global, self.terminal_state.global_cursor,
            match old_wrap {
                true => "<WRAPPED>",
                false => ""
            }
        );
        */
    }

    fn get_max_screen_cursor(&self) -> Cursor {
        [self.screen_width - 1, self.screen_height - 1]
    }

    fn clamp_screen_cursor(&self, cursor: &Cursor) -> Cursor {
        [
            cursor[0].min(self.screen_width - 1),
            cursor[1].min(self.screen_height - 1)
        ]
    }

    fn is_in_margins(&self, screen_cursor: &Cursor) -> bool {
        let margin = &self.terminal_state.margin;
        return screen_cursor[0] >= margin.left
               && screen_cursor[0] <= margin.right
               && screen_cursor[1] >= margin.top
               && screen_cursor[1] <= margin.bottom;
    }

    fn get_relative_screen_cursor(&self, offset: &SignedCursor) -> Cursor {
        let screen_cursor = &self.terminal_state.screen_cursor;
        let relative = [
            (screen_cursor[0] as isize + offset[0]).max(0) as usize,
            (screen_cursor[1] as isize + offset[1]).max(0) as usize
        ];

        self.clamp_screen_cursor(&relative)
    }

    /// Computes the global cursor pos at the start of the current line
    fn get_cursor_pos_sol(&self) -> Cursor {
        let state = &self.terminal_state;
        [
            state.global_cursor[0] - state.screen_cursor[0],
            state.global_cursor[1]
        ]
    }

    /// Computes the global cursor pos at the end of the current line
    fn get_cursor_pos_eol(&self) -> Cursor {
        let state = &self.terminal_state;
        [
            state.global_cursor[0] + (self.screen_width - state.screen_cursor[0] - 1),
            state.global_cursor[1]
        ]
    }

    /// Computes the global cursor pos directly below the supplied cursor
    fn get_cursor_pos_next_line(&self, cursor: &Cursor) -> Cursor {
        let keep_line = self.get_remaining_wrapped_line_count(cursor) > 0;
        if keep_line {
            [cursor[0] + self.screen_width, cursor[1]]
        } else {
            [cursor[0] % self.screen_width, cursor[1] + 1]
        }
    }

    /// Computes the global cursor pos directly above the supplied cursor
    fn get_cursor_pos_prev_line(&self, cursor: &Cursor) -> Cursor {
        if cursor[1] == 0 {
            return *cursor;
        }

        let cur_wrap = cursor[0] % self.screen_width;
        let cur_line = cursor[0] / self.screen_width;
        if cur_line == 0 {
            // Move cursor to wrapped end of previous line
            let prev_line_count = self.get_total_wrapped_line_count(&[0, cursor[1] - 1]);
            [
                (prev_line_count - 1) as usize * self.screen_width + cur_wrap,
                cursor[1] - 1
            ]
        } else {
            // Subtract wrap from this line
            [cursor[0] - self.screen_width, cursor[1]]
        }
    }

    fn get_total_wrapped_line_count(&self, cursor: &Cursor) -> u32 {
        let buffer = &self.terminal_state.screen_buffer;
        match cursor[1] >= buffer.len() {
            true => 1,
            false => {
                let buffer_lines = buffer[cursor[1]].len() / (self.screen_width + 1) + 1;

                buffer_lines as u32
            }
        }
    }

    fn get_remaining_wrapped_line_count(&self, cursor: &Cursor) -> u32 {
        let buffer = &self.terminal_state.screen_buffer;
        match cursor[1] >= buffer.len() || cursor[0] >= buffer[cursor[1]].len() {
            true => 0,
            false => {
                let buffer_lines = buffer[cursor[1]].len() / (self.screen_width + 1);
                let cursor_lines = cursor[0] / self.screen_width;

                (buffer_lines - cursor_lines.min(buffer_lines)) as u32
            }
        }
    }

    // Splits a wrapped line into two lines, where the second line starts on the line
    // corresponding to cursor[0]
    fn split_line(&mut self, cursor: &Cursor) -> bool {
        let buffer = &mut self.terminal_state.screen_buffer;
        if cursor[1] >= buffer.len() || cursor[0] < self.screen_width {
            return false;
        }

        let start_index = cursor[0] - (cursor[0] % self.screen_width);
        let new_line = buffer[cursor[1]].drain(start_index..).collect();
        buffer.insert(cursor[1] + 1, new_line);

        true
    }

    fn erase(&mut self, start: Cursor, end: Cursor) {
        let state = &mut self.terminal_state;
        let mut start = start;
        let mut end = end;
        if (start[1] == end[1] && end[0] < start[0]) || start[1] > end[1] {
            (start, end) = (end, start);
        }

        for y in start[1]..=end[1] {
            if y >= state.screen_buffer.len() {
                break;
            }

            let line = &mut state.screen_buffer[y];
            if y == start[1] {
                line.resize(start[0], Default::default());
            } else if y == end[1] {
                // TODO: memset
                for x in 0..=end[0] {
                    if x >= line.len() {
                        break;
                    }
                    line[x] = Default::default();
                }
            } else {
                line.clear();
            }
        }

        /*
        log::debug!(
            "[erase] Global: {:?} -> {:?}",
            start, end
        );
        */
    }

    fn insert_or_remove_lines(&mut self, remove: bool, amount: u32) {
        if !self.is_in_margins(&self.terminal_state.screen_cursor) {
            return;
        }

        let mut removal_margin = self.terminal_state.margin;
        removal_margin.top = self.terminal_state.screen_cursor[1];
        for _ in 0..amount {
            self.scroll_region(remove, removal_margin);
        }
    }

    /// Scroll the specified screenspace buffer region up or down by one line. Up 
    /// refers to the direction the text is moving
    /// Returns true if the global cursor should be recomputed from the screen cursor
    /// in the case that the buffer was severely messed with
    fn scroll_region(&mut self, up: bool, margin: Margin) -> bool {
        let state = &self.terminal_state;

        log::debug!(
            "[scroll_region] Margin: T{} B{} L{} R{}, up={}",
            margin.top, margin.bottom, margin.left, margin.right, up
        );

        // Only support default scrollback behavior if we don't have any margin. Otherwise,
        // physically scroll the buffer in memory when adding new characters
        let support_scrollback = margin.top == 0
                                 && margin.left == 0
                                 && margin.bottom == self.screen_height - 1
                                 && margin.right == self.screen_width - 1;

        if support_scrollback {
            // Scroll the region with scrollback by updating the reference [0, 0]
            // position in screen space

            let home_still_wrapped = match state.global_cursor_home[1] < state.screen_buffer.len() {
                true => {
                    let global_line = &state.screen_buffer[state.global_cursor_home[1]];
                    match up {
                        true => state.global_cursor_home[0] + self.screen_width < global_line.len(),
                        false => state.global_cursor_home[1] >= self.screen_width
                    }
                }
                false => false
            };
            if home_still_wrapped {
                if up {
                    self.terminal_state.global_cursor_home[0] += self.screen_width;
                } else {
                    self.terminal_state.global_cursor_home[0] -= self.screen_width;
                }
            } else {
                self.terminal_state.global_cursor_home[0] = 0;
                if up {
                    self.terminal_state.global_cursor_home[1] += 1;
                } else if self.terminal_state.global_cursor_home[1] > 0 {
                    self.terminal_state.global_cursor_home[1] -= 1;
                }
            }

            false
        } else {
            // Start by isolating the lines in the region to scroll. That is, split
            // them such that the lines at the top and bottom of the region are their
            // own lines and are no longer wrapped. This way, we can simply remove /
            // insert around them without any issues. This simplifies logic greatly.
            
            let mut region_cursor_top = self.get_cursor_pos_absolute(&[margin.left, margin.top]);
            if self.split_line(&region_cursor_top) {
                region_cursor_top[0] = 0;
                region_cursor_top[1] += 1;
            }
            self.split_line(&self.get_cursor_pos_next_line(&region_cursor_top));

            let mut region_cursor_bot = self.get_cursor_pos_absolute(&[margin.left, margin.bottom]);
            if self.split_line(&region_cursor_bot) {
                region_cursor_bot[0] = 0;
                region_cursor_bot[1] += 1;
            }
            self.split_line(&self.get_cursor_pos_next_line(&region_cursor_bot));

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
            let can_trim_lines = region_size_x == self.screen_width - 1;
            if can_trim_lines {
                if evict_pos[1] >= self.terminal_state.screen_buffer.len() {
                    return false;
                }

                let buf = &mut self.terminal_state.screen_buffer;
                //log::warn!("Removed buffer line {}: {:?}", evict_pos[1], buf[evict_pos[1]]);

                buf.remove(evict_pos[1]);

                if replace_pos[1] < buf.len() {
                    buf.insert(replace_pos[1], vec![]);
                }

                true
            } else {
                // Perform simulated scrolling in the margins by replacing the contents of each line
                // in the scrolling region with the next or prev depending on direction,
                // erasing the final line at the end

                todo!("Scroll region with x margin");
                /*
                let mut evict_pos = evict_pos;
                let mut replace_pos = match up {
                    true => replace_pos = self.get_cursor_pos_next_line(&evict_pos),
                    false => self.get_cursor_pos_prev_line(&evict_pos)
                };

                for _ in 0..region_size_y {
                    let evict_range = evict_pos[0]..=(evict_pos[0] + region_size_x);
                    let replace_range = replace_pos[0]..=(replace_pos[0] + region_size_x);

                    // Ensure the evict region is large enough
                    let evict_buf = &mut self.terminal_state.screen_buffer[evict_pos[1]];
                    if evict_buf.len() < evict_pos[0] + region_size_x + 1 {
                        evict_buf.resize(evict_pos[0] + region_size_x + 1, Default::default());
                    }

                    // Ensure the replacement region is large enough
                    let replace_buf = &mut self.terminal_state.screen_buffer[replace_pos[1]];
                    if replace_buf.len() < replace_pos[0] + region_size_x + 1 {
                        replace_buf.resize(replace_pos[0] + region_size_x + 1, Default::default());
                    }
                    let replace_chars = replace_buf[replace_range].to_vec();

                    self.terminal_state.screen_buffer[evict_pos[1]].splice(
                        evict_range,
                        replace_chars
                    );

                    evict_pos = replace_pos;
                    replace_pos = match up {
                        true => self.get_cursor_pos_next_line(&replace_pos),
                        false => self.get_cursor_pos_prev_line(&replace_pos),
                    };
                }

                // Erase the final 'scrolled' line
                self.erase(
                    evict_pos,
                    [evict_pos[0] + region_size_x, evict_pos[1]]
                );

                false
                */
            }
        }
    }

    /// Advance the screen cursor y by one, potentially scrolling the region if necessary
    fn advance_screen_cursor_with_scroll(&mut self, down: bool) {
        let state = &self.terminal_state;
        let old_screen = state.screen_cursor;
        let old_home = state.global_cursor_home;
        let old_global = state.global_cursor;

        if down && state.screen_cursor[1] < state.margin.bottom {
            self.terminal_state.screen_cursor[1] += 1;
        } else if !down && state.screen_cursor[1] > state.margin.top {
            self.terminal_state.screen_cursor[1] -= 1;
        } else {
            if self.scroll_region(down, state.margin) {
                // After messing with the buffer state, recompute the correct global
                // cursor absolutely rather than trying to use deltas to figure out
                // how it should change. This could work in the future, but it's very
                // complicated with many edge cases and this is much simpler and more
                // reliable. Another downside to this is that it will not continue wrapping
                // and put things on a new line (which should be ok)
                self.terminal_state.global_cursor = self.get_cursor_pos_absolute(
                    &self.terminal_state.screen_cursor
                );
            }
        }

        log::debug!(
            "[advance_screen_cursor_with_scroll] Screen: {:?} -> {:?}, Global: {:?} -> {:?}, Home: {:?} -> {:?}, down={}",
            old_screen, self.terminal_state.screen_cursor,
            old_global, self.terminal_state.global_cursor,
            old_home, self.terminal_state.global_cursor_home,
            down
        );
    }

    fn activate_alternate_screen_buffer(&mut self) {
        self.saved_terminal_state = self.terminal_state.clone();
        self.terminal_state = Default::default();

        // Reset default margins
        self.terminal_state.margin = Margin::get_from_screen_size(
            self.screen_width as u32,
            self.screen_height as u32
        );

        self.terminal_state.alt_screen_buffer_state = BufferState::Active;

        log::debug!("[activate_alternate_screen_buffer]");
    }

    fn deactivate_alternate_screen_buffer(&mut self) {
        let moved_state = std::mem::replace(&mut self.saved_terminal_state, Default::default());
        self.terminal_state = moved_state;

        // Reset default margins
        self.terminal_state.margin = Margin::get_from_screen_size(
            self.screen_width as u32,
            self.screen_height as u32
        );

        log::debug!("[deactivate_alternate_screen_buffer]");
    }

    // Replaces cell content at cursor position, accounting for inserting and removing continuation cells.
    // This assumes that pos is not out of bounds, and the content at pos is NOT empty or a continuation
    fn put_wide_char_unchecked(&mut self, pos: Cursor, style: StyleState, c: char) -> usize {
        // TODO: optimize
        let width = c.width().unwrap_or(1).max(1);
        let cell = &mut self.terminal_state.screen_buffer[pos[1]][pos[0]];
        let old_width = match cell.elem {
            CellContent::Char(_, w) => w,
            CellContent::Grapheme(_, w) => w,
            _ => 1
        };
        cell.elem = CellContent::Char(c, width);
        cell.style = style;
        let continuations_pos = [pos[0] + 1, pos[1]];
        self.update_continuations(continuations_pos, style, old_width - 1, width - 1, 0);

        width
    }

    fn put_char_at_cursor(&mut self, c: char) -> usize {
        let state = &mut self.terminal_state;

        while state.global_cursor[1] >= state.screen_buffer.len() {
            state.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut state.screen_buffer[state.global_cursor[1]];
        while state.global_cursor[0] >= buffer_line.len() {
            buffer_line.push(Default::default());
        }

        match &buffer_line[state.global_cursor[0]].elem {
            // Mutate cursor and navigate to start of character. This makes behavior
            // much more predictable and easy to implement
            CellContent::Continuation(width) => state.global_cursor[0] -= width,
            _ => {}
        };

        let cur_pos = state.global_cursor;
        let cur_style = state.style_state;

        // Fast-path: if the new char is ASCII, skip grapheme merging
        if c.is_ascii() {
            return self.put_wide_char_unchecked(cur_pos, cur_style, c);
        }

        let (left_cells, _) = buffer_line.split_at_mut(state.global_cursor[0]);
        if state.global_cursor[0] > 0 {
            // Get the previous cell, accounting for continuations
            let last_cell = match &left_cells.last().unwrap().elem {
                CellContent::Continuation(width) => &mut left_cells[left_cells.len() - width - 1],
                _ => left_cells.last_mut().unwrap()
            };
            let last_style = last_cell.style;
            match &mut last_cell.elem {
                CellContent::Char(old_c, old_width) => {
                    let mut buf = [0; 10];
                    let len1 = old_c.encode_utf8(&mut buf).len();
                    let len2 = c.encode_utf8(&mut buf[len1..]).len();
                    let str = std::str::from_utf8(&buf[..len1 + len2]).unwrap();
                    match str.graphemes(true).count() {
                        0..=1 => {
                            let width = *old_width;
                            let new_width = str.width();
                            let num_continutations = new_width - width;
                            last_cell.elem = CellContent::Grapheme(str.to_string(), new_width);
                            self.update_continuations(cur_pos, last_style, num_continutations, 0, width);
                            num_continutations
                        },
                        _ => {
                            self.put_wide_char_unchecked(cur_pos, cur_style, c)
                        }
                    }
                }
                CellContent::Grapheme(str, old_width) => {
                    // Temp mutate to check graphemes
                    str.push(c);
                    match str.graphemes(true).count() {
                        0..=1 => {
                            let width = *old_width;
                            let new_width = str.width();
                            let num_continutations = new_width - width;
                            *old_width = new_width;
                            self.update_continuations(cur_pos, last_style, num_continutations, 0, width);
                            num_continutations
                        },
                        _ => {
                            str.pop();
                            self.put_wide_char_unchecked(cur_pos, cur_style, c)
                        }
                    }
                }
                CellContent::Continuation(_) => {
                    log::warn!("BUG! This should never happen.");
                    self.put_wide_char_unchecked(cur_pos, cur_style, c)
                }
                CellContent::Empty => {
                    self.put_wide_char_unchecked(cur_pos, cur_style, c)
                }
            }
        } else {
            self.put_wide_char_unchecked(cur_pos, cur_style, c)
        }
    }

    fn remove_characters(&mut self, pos: Cursor, amount: u32) {
        let state = &mut self.terminal_state;
        if pos[1] >= state.screen_buffer.len() {
            return;
        }

        let offset = pos[0];
        let buffer_line = &mut state.screen_buffer[pos[1]];
        let range = offset..((offset + amount as usize).min(buffer_line.len()));
        buffer_line.drain(range);
    }

    fn update_continuations(&mut self, pos: Cursor, style: StyleState, old_amount: usize, amount: usize, start_index: usize) {
        if amount == 0 && old_amount == 0 {
            // Don't do range check insertions if we aren't actually inserting anything
            return;
        }

        let state = &mut self.terminal_state;

        while pos[1] >= state.screen_buffer.len() {
            state.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut state.screen_buffer[pos[1]];
        while pos[0] >= buffer_line.len() {
            buffer_line.push(Default::default());
        }

        // TODO: handle insert mode
        if false {
            // In insert mode, we insert new cells
            // (This shifts existing cells to the right)
            for i in 0..amount {
                let elem = ScreenBufferElement {
                    style,
                    elem: CellContent::Continuation(start_index + amount - i),
                };
                buffer_line.insert(pos[0], elem);
            }
        } else {
            // Otherwise, we update the existing cells in place
            let mut old_amount = old_amount;
            for i in 0..amount {
                let idx = pos[0] + i;
                let new_cell = ScreenBufferElement {
                    style,
                    elem: CellContent::Continuation(start_index + amount - i),
                };
                if idx < buffer_line.len() {
                    // Ensure that any intersecting graphemes also have their continuations
                    // cleared. Exploit old_amount to achieve this
                    match buffer_line[idx].elem {
                        CellContent::Char(_, w) |
                        CellContent::Grapheme(_, w) => old_amount = amount.max(i + w),
                        _ => {}
                    };
                    buffer_line[idx] = new_cell;
                } else {
                    buffer_line.push(new_cell);
                }
            }
            // If the previous wide character had more continuation cells than we need now,
            // clear the extra cells by marking them as empty.
            if old_amount > amount {
                for i in amount..old_amount {
                    let idx = pos[0] + i;
                    if idx < buffer_line.len() {
                        buffer_line[idx].elem = CellContent::Empty;
                    }
                }
            }
        }
    }
}

// TO ADD:
// - OSC commands (color query, window name, font, etc)
// - Cursor modes
// - Mouse modes (1000-1034)
// - Origin mode
// - Insert/replace mode

impl Perform for Performer {
    fn print(&mut self, c: char) {
        if self.ignore_print {
            return;
        }

        self.action_performed = true;

        // Handle wrapping only when we place a character
        if self.terminal_state.wants_wrap {
            self.terminal_state.screen_cursor[0] = 0;
            self.terminal_state.global_cursor[0] += 1;
            self.terminal_state.wants_wrap = false;
            self.advance_screen_cursor_with_scroll(true);
        }

        // Put char at the current position and advance if necessary
        let num_advances = self.put_char_at_cursor(c);
        for adv in 0..num_advances {
            // Handle wrapping only when we place a character
            if self.terminal_state.wants_wrap {
                self.terminal_state.screen_cursor[0] = 0;
                self.terminal_state.global_cursor[0] += 1;
                self.terminal_state.wants_wrap = false;
                self.advance_screen_cursor_with_scroll(true);
            }

            // Check for wrap. If we want to wrap, update the state accordingly. Otherwise,
            // update the cursor directly
            let state = &mut self.terminal_state;
            let wrap = state.screen_cursor[0] + 1 >= self.screen_width;
            if wrap {
                state.wants_wrap = true;
            } else {
                // Advance the cursor
                state.global_cursor[0] += 1;
                state.screen_cursor[0] += 1;
            }

            log::trace!(
                "Print {:?} (adv {}) {}", c,
                adv,
                match wrap {
                    true => "<NEXT WRAP>",
                    false => ""
                }
            );
        }

        if num_advances == 0 {
            log::trace!("Print {:?} <APPEND>", c);
        }

        if !c.is_whitespace() {
            self.is_empty = false;
        }
    }

    fn execute(&mut self, byte: u8) {
        self.action_performed = true;

        log::trace!("Execute [{:?}]", byte as char);

        match byte {
            b'\n' => {
                let old_global = self.terminal_state.global_cursor;

                self.terminal_state.wants_wrap = false;
                self.terminal_state.global_cursor = self.get_cursor_pos_next_line(
                    &self.terminal_state.global_cursor
                );

                self.advance_screen_cursor_with_scroll(true);

                /*
                log::debug!(
                    "[\\n] Global: {:?} -> {:?}",
                    old_global,
                    self.terminal_state.global_cursor
                );
                */
            },
            b'\r' => {
                let old_global = self.terminal_state.global_cursor;

                self.terminal_state.wants_wrap = false;
                self.terminal_state.global_cursor = self.get_cursor_pos_sol();
                self.terminal_state.screen_cursor[0] = 0;

                /*
                log::debug!(
                    "[\\r] Global: {:?} -> {:?}",
                    old_global,
                    self.terminal_state.global_cursor
                );
                */
            },
            0x07 => { // Bell
                log::debug!("Bell!!!");
            }
            0x08 => { // Backspace
                // Move cursor back by one with back-wrapping
                let state = &mut self.terminal_state;
                if state.global_cursor[0] > 0 {
                    state.global_cursor[0] -= 1;
                    if state.screen_cursor[0] > 0 {
                        state.screen_cursor[0] -= 1;
                    } else if state.screen_cursor[1] > 0 {
                        state.screen_cursor[0] = self.screen_width - 1;
                        state.screen_cursor[1] -= 1;
                    }
                }
            },
            _ => {
                log::debug!("[execute] {:02x}", byte);
            }
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        /*
        log::debug!(
            "[hook] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
            params, intermediates, ignore, c
        );
        */
        log::debug!("Hook [{:?}]", c);
    }

    fn put(&mut self, byte: u8) {
        //log::debug!("[put] {:02x}", byte);
        log::debug!("Put [{:?}]", byte as char);
    }

    fn unhook(&mut self) {
        log::debug!("[unhook]");
    }

    fn osc_dispatch(&mut self, all_params: &[&[u8]], bell_terminated: bool) {
        self.action_performed = true;

        // TODO: investigate the bell, seems like it is relevant for many commands

        let command = self.parse_osc_command(all_params[0]);
        let params = match all_params.len() {
            1 => vec![],
            _ => all_params[1].to_vec()
        };
        match command {
            /*
            11 | 16 => { // Set background color
                if params.len() == 0 {
                    return;
                }

                if params[0] == b'?' { // Requesting default background color
                    let esc_str = "\x1b]11;rgb:0d0d/0f0f/1818\x07"; // Note: ends with bell
                    self.output_stream.extend_from_slice(esc_str.as_bytes());
                    return;
                }
            },
            */
            _ => {
                log::debug!("[osc_dispatch] params={:?} bell_terminated={}", all_params, bell_terminated);
            }
        }

    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        self.action_performed = true;

        //log::trace!("Handling CSI '{:?}'", c);

        let params = self.parse_params(params);
        match c {
            'A' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor up [{:?}]", amount);
                self.set_cursor_pos_relative(&[0, -amount as isize])
            },
            'B' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor down [{:?}]", amount);
                self.set_cursor_pos_relative(&[0, amount])
            },
            'C' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor right [{:?}]", amount);
                self.set_cursor_pos_relative(&[amount, 0])
            },
            'D' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor left [{:?}]", amount);
                self.set_cursor_pos_relative(&[-amount, 0])
            },
            'E' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor next line [{:?}]", amount);
                let mut new_screen = self.get_relative_screen_cursor(&[0, amount]);
                new_screen[0] = 0;
                self.set_cursor_pos_absolute(&new_screen);
            },
            'F' => {
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor preceding line [{:?}]", amount);
                let mut new_screen = self.get_relative_screen_cursor(&[0, -amount]);
                new_screen[0] = 0;
                self.set_cursor_pos_absolute(&new_screen);
            },
            'G' => { // Place cursor in row
                let col = match params.len() {
                    0 | 1 if params[0] == 0 => 0,
                    _ => params[0] as usize - 1
                };
                log::debug!("Cursor set col [{}]", col);
                self.set_cursor_pos_absolute(&[col, self.terminal_state.screen_cursor[1]]);
            },
            'H' | 'f' => { // Place cursor
                // Params have row, col format
                let row = match params.len() {
                    1..=2 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                let col = match params.len() {
                    2 if params[1] > 0 => params[1] as usize - 1,
                    _ => 0
                };
                log::debug!("Cursor set position [{}, {}]", col, row);
                self.set_cursor_pos_absolute(&[col, row]);
            },
            'J' => { // Erase in display
                //log::debug!("Erase in display");
                let min_cursor = self.terminal_state.global_cursor_home;
                let max_cursor = self.get_cursor_pos_absolute(
                    &self.get_max_screen_cursor()
                );
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(self.terminal_state.global_cursor, max_cursor),
                    1 => self.erase(self.terminal_state.global_cursor, min_cursor),
                    2 => {
                        self.erase(min_cursor, max_cursor);
                        self.terminal_state.global_cursor = min_cursor;
                        self.terminal_state.screen_cursor = [0, 0];
                    },
                    3 => {}
                    _ => {}
                }
            },
            'K' => { // Erase in line
                //log::debug!("Erase in line");
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(
                        self.terminal_state.global_cursor,
                        self.get_cursor_pos_eol()
                    ),
                    1 => self.erase(
                        self.terminal_state.global_cursor,
                        self.get_cursor_pos_sol()
                    ),
                    2 => self.erase(
                        self.get_cursor_pos_sol(),
                        self.get_cursor_pos_eol()
                    ),
                    _ => {}
                }
            },
            'L' | 'M' => { // Insert/Remove lines
                let remove = c == 'M';
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as u32;
                log::debug!(
                    "{} lines [{:?}]",
                    match remove {
                        true => "Delete",
                        false => "Insert"
                    },
                    amount
                );
                self.insert_or_remove_lines(remove, amount);
            },
            'P' => { // Delete characters
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as u32;
                log::debug!("Delete chars [{:?}]", amount);
                self.remove_characters(self.terminal_state.global_cursor, amount);
            },
            'c' => { // Send device attributes
                if !params.is_empty() && params[0] != 0 {
                    return;
                }
                match intermediates.len() {
                    0 => {}, // Primary
                    1 => {
                        match intermediates[0] {
                            b'>' => { // Secondary
                                let esc_str = "\x1b[>0;10;1c";
                                self.output_stream.extend_from_slice(esc_str.as_bytes());
                            },
                            b'=' => {}, // Tertiary
                            _ => {}
                        }
                    },
                    _ => {}
                }
            },
            'd' => { // Set line position absolute
                let row = match params.len() {
                    1 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                log::debug!("Set line position abs [_, {}]", row);
                self.set_cursor_pos_absolute(
                    &[self.terminal_state.screen_cursor[0], row]
                );
            },
            'e' => { // Set line position relative
                let row = match params.len() {
                    1 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                log::debug!("Set line position relative [_, +{}]", row);
                self.set_cursor_pos_absolute(
                    &[
                        self.terminal_state.screen_cursor[0],
                        self.terminal_state.screen_cursor[1] + row
                    ]
                );
            },
            'h' | 'l' => {
                if params.len() != 1 {
                    return;
                }

                let enabled = c == 'h';
                let public = match intermediates.len() {
                    0 => true,
                    1 if intermediates[0] == b'?' => false,
                    _ => return
                };
                match public {
                    true => {},
                    false => {
                        match params[0] {
                            25 => self.terminal_state.cursor_state.visible = enabled,
                            1000 => self.terminal_state.mouse_tracking_mode = match enabled {
                                true => MouseTrackingMode::Default,
                                false => MouseTrackingMode::Disabled
                            },
                            1002 => self.terminal_state.mouse_tracking_mode = match enabled {
                                true => MouseTrackingMode::ButtonEvent,
                                false => MouseTrackingMode::Disabled
                            },
                            1003 => self.terminal_state.mouse_tracking_mode = match enabled {
                                true => MouseTrackingMode::AnyEvent,
                                false => MouseTrackingMode::Disabled
                            },
                            1005 => self.terminal_state.mouse_mode = match enabled {
                                true => MouseMode::UTF8,
                                false => MouseMode::Default
                            },
                            1006 => self.terminal_state.mouse_mode = match enabled {
                                true => MouseMode::SGR,
                                false => MouseMode::Default
                            },
                            1046 => match enabled {
                                true => match self.terminal_state.alt_screen_buffer_state {
                                    BufferState::Active => {}
                                    BufferState::Enabled => {}
                                    BufferState::Disabled => self.terminal_state.alt_screen_buffer_state = BufferState::Enabled
                                }
                                false => match self.terminal_state.alt_screen_buffer_state {
                                    BufferState::Active => {
                                        self.deactivate_alternate_screen_buffer();
                                    },
                                    BufferState::Enabled => self.terminal_state.alt_screen_buffer_state = BufferState::Disabled,
                                    BufferState::Disabled => {}
                                }
                            }
                            // These should technically do different things, but this implementation  
                            // always saves & restores the cursor so we can just treat them as the same
                            1047 | 1049 => match self.terminal_state.alt_screen_buffer_state {
                                BufferState::Active if !enabled => {
                                    self.deactivate_alternate_screen_buffer();
                                },
                                BufferState::Enabled if enabled => {
                                    self.activate_alternate_screen_buffer();
                                }
                                BufferState::Disabled => {}
                                _ => {}
                            }
                            _ => {}
                        }
                    }
                }

                log::debug!(
                    "Set (public={}, enabled={}) mode [{:?}]",
                    public,
                    enabled,
                    params[0]
                );
            },
            'm' => { // Graphics
                self.parse_graphics_escape(&params);

                log::trace!(
                    "Graphics [{:?}] -> {:?}",
                    params,
                    self.terminal_state.style_state
                );
            },
            'n' => { // Device status report
                let state = &mut self.terminal_state;
                if params.len() != 1 {
                    return;
                }
                match params[0] {
                    5 => {}, // Status report
                    6 => { // Report cursor position
                        let esc_str = format!(
                            "\x1b[{};{}R",
                            state.screen_cursor[1] + 1,
                            state.screen_cursor[0] + 1
                        );
                        self.output_stream.extend_from_slice(esc_str.as_bytes());
                    },
                    _ => {}
                }
                log::debug!("DSR requested");
            }
            'q' => {
                let state = &mut self.terminal_state;
                if intermediates.len() != 1 {
                    return;
                }
                match intermediates[0] {
                    b' ' => {
                        let param = match params.len() {
                            1 => params[0],
                            _ => 0
                        };
                        state.cursor_state.style = match param {
                            3..=4 => CursorStyle::Underline,
                            5..=6 => CursorStyle::Bar,
                            _ => CursorStyle::Block
                        };
                        state.cursor_state.blinking = param % 2 == 1 || param == 0;

                        log::debug!(
                            "Cursor style: [{:?}, {:?}]",
                            state.cursor_state.style,
                            state.cursor_state.blinking
                        );
                    }
                    _ => {}
                }
            },
            'r' => { // Set scroll margin Y
                let top = match params.len() {
                    1..=2 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                let bottom = match params.len() {
                    2 if params[1] > 0 => params[1] as usize - 1,
                    _ => self.screen_height - 1
                };

                if top >= bottom || bottom >= self.screen_height {
                    return;
                }

                self.terminal_state.margin.top = top;
                self.terminal_state.margin.bottom = bottom;
                self.set_cursor_pos_absolute(&[0, 0]);

                log::debug!("Scroll margin Y: [{:?}, {:?}]", top, bottom);
            },
            's' => { // Set scroll margin X
                let left = match params.len() {
                    1..=2 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                let right = match params.len() {
                    2 if params[1] > 0 => params[1] as usize - 1,
                    _ => self.screen_width - 1
                };

                if left >= right || right >= self.screen_width {
                    return;
                }

                self.terminal_state.margin.left = left;
                self.terminal_state.margin.right = right;
                self.set_cursor_pos_absolute(&[0, 0]);

                log::debug!("Scroll margin X: [{:?}, {:?}]", left, right);
            },
            _ => {
                log::debug!(
                    "[csi_dispatch] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
                    params, intermediates, ignore, c
                );
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.action_performed = true;

        log::trace!("Esc [{:?}]", byte as char);

        match byte {
            b'B' => {},
            b'M' => { // Reverse index
                let old_global = self.terminal_state.global_cursor;

                self.terminal_state.wants_wrap = false;
                self.terminal_state.global_cursor = self.get_cursor_pos_prev_line(
                    &self.terminal_state.global_cursor
                );

                self.advance_screen_cursor_with_scroll(false);

                log::debug!(
                    "[reverse_index] Global: {:?} -> {:?}",
                    old_global,
                    self.terminal_state.global_cursor
                );
            },
            // Special sequences generated by the screen-256color term we are claiming
            // to be. Everything inside can be ignored.
            0x6b => self.ignore_print = true,
            0x5c => self.ignore_print = false,
            _ => {
                log::debug!(
                    "[esc_dispatch] intermediates={:?}, ignore={:?}, byte={:02x}",
                    intermediates, ignore, byte
                );
            }
        }
    }
}

impl fmt::Debug for StyleState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "StyleState: ")?;
        write!(f, "Style<>")?;
        match self.fg_color {
            Some(c) => write!(f, "FG<{}, {}, {}>, ", c[0], c[1], c[2])?,
            None => write!(f, "FG<None>")?
        };
        match self.bg_color {
            Some(c) => write!(f, "BG<{}, {}, {}>, ", c[0], c[1], c[2])?,
            None => write!(f, "BG<None>")?
        };

        Ok(())
    }
}

impl fmt::Debug for ScreenBufferElement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SBufElem: ")?;
        write!(f, "C: {:?}, ", self.elem)?;
        //write!(f, "STY: {:?}, ", self.style)?;
        Ok(())
    }
}
