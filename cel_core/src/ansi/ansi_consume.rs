use vte::{Parser, Params, Perform};
use std::fmt;

use super::*;

impl AnsiHandler {
    pub fn new() -> Self {
        Self {
            performer: Default::default(),
            state_machine: Parser::new()
        }
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
}

impl Performer {
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

    fn parse_4_bit_color(weight: ColorWeight, code: u16) -> [f32; 3] {
        let factor: f32 = match weight {
            ColorWeight::Normal => 0.5,
            ColorWeight::Bold => 1.0,
            ColorWeight::Faint => 0.25
        };
        let one = (code & 1) as f32 * factor;
        let two = ((code & 2) >> 1) as f32 * factor;
        let four = ((code & 4) >> 2) as f32 * factor;
        match code {
            1..=6 => [one, two, four],
            0     => match weight {
                ColorWeight::Normal => [0.0, 0.0, 0.0],
                ColorWeight::Bold => [0.5, 0.5, 0.5],
                ColorWeight::Faint => [0.25, 0.25, 0.25]
            },
            7     => match weight {
                ColorWeight::Normal => [0.75, 0.75, 0.75],
                ColorWeight::Bold => [1.0, 1.0, 1.0],
                ColorWeight::Faint => [0.5, 0.5, 0.5]
            }
            _ => [0.0, 0.0, 0.0]
        }
    }

    fn parse_8_bit_color(rgb: &[u16]) -> [f32; 3] {
        let rgb: [u16; 3] = rgb.try_into().unwrap_or([0, 0, 0]);
        rgb.map(|c| c as f32 / 255.0)
    }

    fn parse_color_escape(&mut self, params: &Vec<u16>) {
        let state = &mut self.terminal_state.color_state;
        let mut extended_mode = 0;
        for (i, code) in params.iter().enumerate() {
            match code {
                0 => *state = Default::default(),
                1 => state.weight = ColorWeight::Bold,
                2 => match extended_mode {
                    38 => {
                        state.foreground = Some(Self::parse_8_bit_color(&params[(i + 1)..]));
                        return;
                    },
                    48 => {
                        state.background = Some(Self::parse_8_bit_color(&params[(i + 1)..]));
                        return;
                    },
                    _ => state.weight = ColorWeight::Faint,
                },
                22 => state.weight = ColorWeight::Normal,
                30..=37 => state.foreground = Some(Self::parse_4_bit_color(state.weight, code - 30)),
                40..=47 => state.background = Some(Self::parse_4_bit_color(state.weight, code - 40)),
                90..=97   => state.foreground = Some(Self::parse_4_bit_color(ColorWeight::Bold, code - 90)),
                100..=107 => state.background = Some(Self::parse_4_bit_color(ColorWeight::Bold, code - 100)),
                38 => extended_mode = 38,
                39 => state.foreground = None,
                48 => extended_mode = 48,
                49 => state.background = None,
                _ => {}
            }
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
            let line = &state.screen_buffer[cur_global[1]];
            for char_idx in cur_global[0]..line.len() {
                // Handle wrap only when we have a character to place there
                if cur_screen[0] >= self.screen_width {
                    cur_screen[0] = 0;
                    cur_screen[1] += 1;
                }

                // log::warn!("{}", line[char_idx].elem);

                if cur_screen[1] == target_screen[1] {
                    return [
                        char_idx + (target_screen[0] - cur_screen[0]),
                        cur_global[1]
                    ];
                }

                cur_screen[0] += 1;
            }

            // Should only happen if line is empty
            if cur_screen[1] == target_screen[1] {
                return [
                    target_screen[0] - cur_screen[0],
                    cur_global[1]
                ];
            }

            let is_wrapped = cur_screen[0] == 0 && (line.len().max(1) / self.screen_width) != 0;
            if  !is_wrapped {
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
        let state = &self.terminal_state;
        let keep_line = self.get_remaining_wrapped_line_count(cursor) > 0;
        if keep_line {
            [cursor[0] + self.screen_width, cursor[1]]
        } else {
            [cursor[0] % self.screen_width, cursor[1] + 1]
        }
    }

    /// Computes the global cursor pos directly above the supplied cursor
    fn get_cursor_pos_prev_line(&self, cursor: &Cursor) -> Cursor {
        let state = &self.terminal_state;
        if cursor[1] == 0 {
            return *cursor;
        }

        let cur_wrap = cursor[0] % self.screen_width;
        if cur_wrap == 0 {
            let prev_line_count = self.get_total_wrapped_line_count(&[0, cursor[1] - 1]);
            [
                (prev_line_count - 1) as usize * self.screen_width + cur_wrap,
                cursor[1] - 1
            ]
        } else {
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

                (buffer_lines - cursor_lines) as u32
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

            let home_still_wrapped = match state.global_cursor_home[1]  < state.screen_buffer.len() {
                true => {
                    let global_line = &state.screen_buffer[state.global_cursor_home[1]];
                    state.global_cursor_home[0] + self.screen_width < global_line.len()
                }
                false => false
            };
            if home_still_wrapped {
                self.terminal_state.global_cursor_home[0] += self.screen_width;
            } else {
                self.terminal_state.global_cursor_home[0] = 0;
                self.terminal_state.global_cursor_home[1] += 1;
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
                // Perform simulated scrolling in the margins by removing entire or partial
                // buffer lines, which will affect the positioning of the lines and visually
                // simulates scrolling

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
    fn advance_screen_cursor_with_scroll(&mut self) {
        let state = &self.terminal_state;
        let old_screen = state.screen_cursor;
        let old_home = state.global_cursor_home;
        let old_global = state.global_cursor;

        if state.screen_cursor[1] < state.margin.bottom {
            self.terminal_state.screen_cursor[1] += 1;
        } else {
            if self.scroll_region(true, state.margin) {
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
            "[advance_screen_cursor_with_scroll] Screen: {:?} -> {:?}, Global: {:?} -> {:?}, Home: {:?} -> {:?}",
            old_screen, self.terminal_state.screen_cursor,
            old_global, self.terminal_state.global_cursor,
            old_home, self.terminal_state.global_cursor_home
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
}

// TO ADD:
// - OSC commands (color query, window name, font, etc)
// - Cursor modes
// - Alternate screen buffer
// - Mouse modes (1000-1034)
// - Origin mode

impl Perform for Performer {
    fn print(&mut self, c: char) {
        self.action_performed = true;

        // Handle wrapping only when we place a character
        if self.terminal_state.wants_wrap {
            self.terminal_state.screen_cursor[0] = 0;
            self.terminal_state.global_cursor[0] += 1;
            self.terminal_state.wants_wrap = false;
            self.advance_screen_cursor_with_scroll();
        }

        let state = &mut self.terminal_state;

        while state.global_cursor[1] >= state.screen_buffer.len() {
            state.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut state.screen_buffer[state.global_cursor[1]];
        while state.global_cursor[0] >= buffer_line.len() {
            buffer_line.push(Default::default());
        }

        buffer_line[state.global_cursor[0]] = ScreenBufferElement {
            elem: c,
            fg_color: state.color_state.foreground,
            bg_color: state.color_state.background
        };

        // Check for wrap. If we want to wrap, update the state accordingly. Otherwise,
        // update the cursor directly
        let wrap = state.screen_cursor[0] + 1 >= self.screen_width;
        if wrap {
            state.wants_wrap = true;
        } else {
            // Advance the cursor
            state.global_cursor[0] += 1;
            state.screen_cursor[0] += 1;
        }

        log::trace!(
            "Print {:?} {}",
            c,
            match wrap {
                true => "<NEXT WRAP>",
                false => ""
            }
        );
    }

    fn execute(&mut self, byte: u8) {
        self.action_performed = true;
        log::debug!("Exec [{:?}]", byte as char);

        match byte {
            b'\n' => {
                let old_global = self.terminal_state.global_cursor;

                self.terminal_state.wants_wrap = false;
                self.terminal_state.global_cursor = self.get_cursor_pos_next_line(
                    &self.terminal_state.global_cursor
                );

                self.advance_screen_cursor_with_scroll();

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
                    } else {
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
        println!(
            "[hook] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
            params, intermediates, ignore, c
        );
        */
        log::debug!("Hook [{:?}]", c);
    }

    fn put(&mut self, byte: u8) {
        //println!("[put] {:02x}", byte);
        log::debug!("Put [{:?}]", byte as char);
    }

    fn unhook(&mut self) {
        log::debug!("[unhook]");
    }

    fn osc_dispatch(&mut self, all_params: &[&[u8]], bell_terminated: bool) {
        self.action_performed = true;

        // TODO: investigate the bell, seems like it is relevant for many commands

        println!("[osc_dispatch] params={:?} bell_terminated={}", all_params, bell_terminated);
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
                println!("[osc_dispatch] params={:?} bell_terminated={}", all_params, bell_terminated);
            }
        }

    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        self.action_performed = true;

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
            'H' => { // Place cursor
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
                            1046 => match enabled {
                                true => match self.alt_screen_buffer_state {
                                    BufferState::Active => {}
                                    BufferState::Enabled => {}
                                    BufferState::Disabled => self.alt_screen_buffer_state = BufferState::Enabled
                                }
                                false => match self.alt_screen_buffer_state {
                                    BufferState::Active => {
                                        self.alt_screen_buffer_state = BufferState::Disabled;
                                        self.deactivate_alternate_screen_buffer();
                                    },
                                    BufferState::Enabled => self.alt_screen_buffer_state = BufferState::Disabled,
                                    BufferState::Disabled => {}
                                }
                            }
                            // These should technically do different things, but this implementation  
                            // always saves & restores the cursor so we can just treat them as the same
                            1047 | 1049 => match self.alt_screen_buffer_state {
                                BufferState::Active if !enabled => {
                                    self.alt_screen_buffer_state = BufferState::Enabled;
                                    self.deactivate_alternate_screen_buffer();
                                },
                                BufferState::Enabled if enabled => {
                                    self.alt_screen_buffer_state = BufferState::Active;
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
                self.parse_color_escape(&params);
                /*
                log::debug!(
                    "Graphics [{:?}] -> {:?}",
                    params,
                    self.terminal_state.color_state
                );
                */
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
                log::warn!("Cursor: {:?}, {:?}", params, intermediates);
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
                println!(
                    "[csi_dispatch] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
                    params, intermediates, ignore, c
                );
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.action_performed = true;
        //log::debug!("Esc [{:?}]", byte as char);

        match byte {
            b'B' => {},
            _ => {
                println!(
                    "[esc_dispatch] intermediates={:?}, ignore={:?}, byte={:02x}",
                    intermediates, ignore, byte
                );
            }
        }
    }
}

impl fmt::Debug for ColorState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ColorState: ")?;
        match self.foreground {
            Some(c) => write!(f, "FG<{}, {}, {}>, ", c[0], c[1], c[2])?,
            None => write!(f, "FG<None>")?
        };
        match self.background {
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
        write!(f, "FG: {:?}, ", self.fg_color)?;
        write!(f, "BG: {:?}", self.bg_color)?;
        Ok(())
    }
}
