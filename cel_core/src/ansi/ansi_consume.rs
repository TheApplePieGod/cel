use vte::{Parser, Params, Perform};
use std::fmt;

use super::*;

impl TerminalState {
    pub fn get_num_lines(&self, include_cursor: bool) -> usize {
        match include_cursor && self.cursor_state.visible {
            true => self.grid.get_buffer_len_with_cursor(),
            false => self.grid.get_buffer_len()
        }
    }
}

impl AnsiHandler {
    pub fn new(max_rows: u32, max_cols: u32, max_scrollback: u32) -> Self {
        let mut obj = Self {
            performer: Performer::new(max_cols, max_rows, max_scrollback),
            state_machine: Parser::new(),

            mouse_states: [(Default::default(), false); 256],
            scroll_states: [0.0; 2]
        };

        obj.resize(max_cols, max_rows, false);

        obj
    }

    pub fn handle_byte(&mut self, byte: u8) {
        self.state_machine.advance(&mut self.performer, byte);
    }

    pub fn handle_sequence_bytes(
        &mut self,
        seq: &[u8],
        stop_early: bool
    ) -> Option<(u32, bool)> {
        let starting_prompt = self.performer.prompt_id;
        self.performer.action_performed = false;
        for (i, c) in seq.iter().enumerate() {
            //print!("{:?} ", *c as char);
            self.state_machine.advance(&mut self.performer, *c);
            let prompt_change = starting_prompt != self.performer.prompt_id;
            if (stop_early && self.performer.action_performed) || prompt_change {
                return Some((i as u32, prompt_change))
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
            MouseTrackingMode::AnyEvent => {
                // TODO: tracking when no buttons are pressed
                return;
            },
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

    pub fn resize(&mut self, width: u32, height: u32, should_clear: bool) {
        let width = width as usize;
        let height = height as usize;

        if width != self.performer.screen_width || height != self.performer.screen_height {
            // TODO: relative margin update?
            self.performer.screen_width = width;
            self.performer.screen_height = height;
            self.performer.terminal_state.grid.resize(
                width as usize,
                height as usize,
                !self.performer.terminal_state.clear_on_resize,
                false
            );

            if self.performer.terminal_state.clear_on_resize && should_clear {
                self.performer.terminal_state.grid.clear();
            }
        }
    }

    pub fn get_terminal_state(&self) -> &TerminalState {
        &self.performer.terminal_state
    }

    pub fn get_terminal_state_mut(&mut self) -> &mut TerminalState {
        &mut self.performer.terminal_state
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

    pub fn get_width(&self) -> u32 {
        self.performer.screen_width as u32
    }

    pub fn get_height(&self) -> u32 {
        self.performer.screen_height as u32
    }

    pub fn get_current_dir(&self) -> &str {
        &self.performer.current_dir
    }

    pub fn get_exit_code(&self) -> Option<u32> {
        self.performer.exit_code
    }

    pub fn get_element(&self, pos: Cursor) -> Option<ScreenBufferElement> {
        let elem = self.performer.terminal_state.grid.get_cell_opt(pos)?;
        Some(elem.clone())
    }

    pub fn get_text(&self) -> String {
        self.performer.terminal_state.grid.get_text()
    }

    pub fn reset(&mut self) {
        self.performer.terminal_state = Default::default();
        self.resize(self.performer.screen_width as u32, self.performer.screen_height as u32, false);
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
    fn new(width: u32, height: u32, max_scrollback: u32) -> Self {
        let mut obj = Self {
            screen_width: width as usize,
            screen_height: height as usize,
            output_stream: vec![],
            is_empty: true,
            exit_code: None,
            prompt_id: 0,
            current_dir: String::new(),
            action_performed: false,

            terminal_state: Default::default(),
            saved_terminal_state: Default::default(),

            ignore_print: false
        };

        obj.terminal_state.grid.resize(width as usize, height as usize, false, false);
        obj.terminal_state.grid.set_max_scrollback(max_scrollback as usize);
        
        obj
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

    fn parse_ascii_integer(&self, bytes: &[u8]) -> u16 {
        let mut result: u16 = 0;

        // Convert each byte to its integer rep from ascii
        let mut power = 1;
        for i in 0..bytes.len().min(5) {
            let byte = bytes[bytes.len() - i - 1];
            // Ensure no overflow
            result = result.saturating_add(((byte.saturating_sub(48)) as u16).saturating_mul(power));
            power = power.saturating_mul(10);
        }
        
        result
    }

    fn parse_ascii_string(&self, bytes: &[u8]) -> String {
        let mut result = String::new();

        for i in 0..bytes.len().min(9999) {
            result.push(char::from(bytes[i]));
        }
        
        result
    }

    // Code is [0, 7], assumes weight is already considered
    fn parse_4_bit_color(style_flags: StyleFlags, code: u16) -> [f32; 3] {
        const BASE_COLORS: [[f32; 3]; 8] = [
            [0.13, 0.15, 0.17], // Black (dark gray)
            [0.87, 0.27, 0.27], // Red
            [0.48, 0.76, 0.29], // Green
            [0.98, 0.72, 0.21], // Yellow
            [0.33, 0.63, 0.87], // Blue
            [0.82, 0.47, 0.87], // Magenta
            [0.20, 0.80, 0.78], // Cyan
            [0.90, 0.91, 0.91], // White (light gray)
        ];

        let factor: f32 = if style_flags.contains(StyleFlags::Bold) {
            1.0
        } else if style_flags.contains(StyleFlags::Faint) {
            0.5
        } else {
            0.75
        };

        if code < 8 {
            let base = BASE_COLORS[code as usize];
            [base[0] * factor, base[1] * factor, base[2] * factor]
        } else {
            // Fallback
            [0.0, 0.0, 0.0]
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

    fn activate_alternate_screen_buffer(&mut self) {
        self.saved_terminal_state = self.terminal_state.clone();
        self.terminal_state = Default::default();

        self.terminal_state.grid.resize(self.screen_width, self.screen_height, false, true);

        self.terminal_state.alt_screen_buffer_state = BufferState::Active;

        log::debug!("[activate_alternate_screen_buffer]");
    }

    fn deactivate_alternate_screen_buffer(&mut self) {
        let moved_state = std::mem::replace(&mut self.saved_terminal_state, Default::default());
        self.terminal_state = moved_state;

        log::debug!("[deactivate_alternate_screen_buffer]");
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

        self.terminal_state.grid.print_char(c, &self.terminal_state.style_state);

        if !c.is_whitespace() {
            self.is_empty = false;
        }
    }

    fn execute(&mut self, byte: u8) {
        self.action_performed = true;

        log::trace!("Execute [{:?}]", byte as char);

        let state = &mut self.terminal_state;
        match byte {
            b'\n' => {
                state.grid.push_cursor_vertically(true);

                /*
                log::debug!(
                    "[\\n] Global: {:?} -> {:?}",
                    old_global,
                    self.terminal_state.global_cursor
                );
                */
            },
            b'\r' => {
                state.grid.set_cursor(
                    state.grid.get_cursor_sol(state.grid.cursor)
                );

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
                state.grid.move_backward();
            },
            _ => {
                log::debug!("<Unhandled> [execute] {:02x}", byte);
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

        let command = self.parse_ascii_integer(all_params[0]);
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
            1337 => { // Update prompt id
                if params.len() == 0 {
                    return;
                }

                self.prompt_id = self.parse_ascii_integer(&params) as u32;
            },
            1338 => { // Update current dir
                if params.len() == 0 {
                    return;
                }

                self.current_dir = self.parse_ascii_string(&params);
            },
            1339 => { // Update exit code
                if params.len() == 0 {
                    return;
                }

                self.exit_code = Some(self.parse_ascii_integer(&params) as u32);
            },
            1340 => { // Set clear-on-resize status
                if params.len() == 0 {
                    return;
                }

                let state = self.parse_ascii_integer(&params) as u32;
                self.terminal_state.clear_on_resize = state == 1;
            },
            _ => {
                log::debug!("<Unhandled> [osc_dispatch] params={:?} bell_terminated={}", all_params, bell_terminated);
            }
        }

    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        self.action_performed = true;

        //log::trace!("Handling CSI '{:?}'", c);

        let params = self.parse_params(params);
        match c {
            'A' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor up [{:?}]", amount);
                state.grid.set_cursor(
                    state.grid.get_cursor_relative(state.grid.cursor, [0, -amount])
                );
            },
            'B' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor down [{:?}]", amount);
                state.grid.set_cursor(
                    state.grid.get_cursor_relative(state.grid.cursor, [0, amount])
                );
            },
            'C' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor right [{:?}]", amount);
                state.grid.set_cursor(
                    state.grid.get_cursor_relative(state.grid.cursor, [amount, 0])
                );
            },
            'D' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor left [{:?}]", amount);
                state.grid.set_cursor(
                    state.grid.get_cursor_relative(state.grid.cursor, [-amount, 0])
                );
            },
            'E' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor next line [{:?}]", amount);
                state.grid.set_cursor(state.grid.get_cursor_next_line(state.grid.cursor));
                state.grid.cursor[0] = 0;
            },
            'F' => {
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Cursor preceding line [{:?}]", amount);
                state.grid.set_cursor(state.grid.get_cursor_prev_line(state.grid.cursor));
                state.grid.cursor[0] = 0;
            },
            'G' => { // Place cursor in row
                let state = &mut self.terminal_state;
                let col = match params.len() {
                    0 | 1 if params[0] == 0 => 0,
                    _ => params[0] as usize - 1
                };
                log::debug!("Cursor set col [{}]", col);
                state.grid.set_cursor([col, state.grid.cursor[1]]);
            },
            'H' | 'f' => { // Place cursor
                // Params have row, col format
                let state = &mut self.terminal_state;
                let row = match params.len() {
                    1..=2 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                let col = match params.len() {
                    2 if params[1] > 0 => params[1] as usize - 1,
                    _ => 0
                };
                log::debug!("Cursor set position [{}, {}]", col, row);
                state.grid.set_cursor([col, row]);
            },
            'J' => { // Erase in display
                //log::debug!("Erase in display");
                let state = &mut self.terminal_state;
                let min_cursor = [0, 0];
                let max_cursor = state.grid.get_cursor_max();
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => state.grid.erase(state.grid.cursor, max_cursor),
                    1 => state.grid.erase(state.grid.cursor, min_cursor),
                    2 => {
                        state.grid.erase(min_cursor, max_cursor);
                        state.grid.set_cursor(min_cursor);
                    },
                    _ => {}
                }
            },
            'K' => { // Erase in line
                //log::debug!("Erase in line");
                let state = &mut self.terminal_state;
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => state.grid.erase(
                        state.grid.cursor,
                        state.grid.get_cursor_eol(state.grid.cursor)
                    ),
                    1 => state.grid.erase(
                        state.grid.cursor,
                        state.grid.get_cursor_sol(state.grid.cursor)
                    ),
                    2 => state.grid.erase(
                        state.grid.get_cursor_sol(state.grid.cursor),
                        state.grid.get_cursor_eol(state.grid.cursor)
                    ),
                    _ => {}
                }
            },
            'L' | 'M' => { // Insert/Remove lines
                let state = &mut self.terminal_state;
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
                state.grid.insert_or_remove_lines(state.grid.cursor, amount, remove);
            },
            'P' => { // Delete characters
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as u32;
                log::debug!("Delete chars [{:?}]", amount);
                state.grid.delete_cells(state.grid.cursor, amount);
            },
            'S' | 'T' => { // Scroll region
                let state = &mut self.terminal_state;
                if intermediates.len() != 0 {
                    return;
                }
                let amount = match params.len() {
                    1 if params[0] > 0 => params[0] as usize,
                    _ => 1
                };
                let up = c == 'S';
                log::debug!("Scroll {} by {}", if up { "up" } else { "down" }, amount);
                for _ in 0..amount {
                    // TODO: improve perf
                    state.grid.scroll(up);
                }
            },
            'X' => { // 'Erase' characters
                let state = &mut self.terminal_state;
                let amount = match params.len() {
                    0 | 1 if params[0] == 0 => 1,
                    _ => params[0]
                } as isize;
                log::debug!("Erase chars [{:?}]", amount);
                let start = state.grid.cursor;
                let end = state.grid.get_cursor_relative(start, [amount, 0]);
                state.grid.erase(start, end);
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
                let state = &mut self.terminal_state;
                let row = match params.len() {
                    1 if params[0] > 0 => params[0] as usize - 1,
                    _ => 0
                };
                log::debug!("Set line position abs [_, {}]", row);
                state.grid.set_cursor([state.grid.cursor[0], row]);
            },
            'e' => { // Set line position relative
                let state = &mut self.terminal_state;
                let row = match params.len() {
                    1 if params[0] > 0 => params[0] as isize - 1,
                    _ => 0
                };
                log::debug!("Set line position relative [_, +{}]", row);
                state.grid.set_cursor(
                    state.grid.get_cursor_relative(state.grid.cursor, [0, row])
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
                            7 => self.terminal_state.grid.autowrap = enabled,
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
                            },
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
                            },
                            2004 => self.terminal_state.bracketed_paste_enabled = enabled,
                            _ => log::debug!("<Unhandled> Mode {} = {}", params[0], enabled)
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
                            state.grid.cursor[1] + 1,
                            state.grid.cursor[0] + 1
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

                self.terminal_state.grid.set_vertical_margin(top, bottom);
                self.terminal_state.grid.set_cursor([0, 0]);

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

                self.terminal_state.grid.set_horizontal_margin(left, right);
                self.terminal_state.grid.set_cursor([0, 0]);

                log::debug!("Scroll margin X: [{:?}, {:?}]", left, right);
            },
            _ => {
                log::debug!(
                    "<Unhandled> [csi_dispatch] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
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
                let old_cursor = self.terminal_state.grid.cursor;

                self.terminal_state.grid.push_cursor_vertically(false);

                log::debug!(
                    "[reverse_index] Cursor: {:?} -> {:?}",
                    old_cursor,
                    self.terminal_state.grid.cursor
                );
            },
            // Special sequences generated by the screen-256color term we are claiming
            // to be. Everything inside can be ignored.
            0x6b => self.ignore_print = true,
            0x5c => self.ignore_print = false,
            _ => {
                log::debug!(
                    "<Unhandled> [esc_dispatch] intermediates={:?}, ignore={:?}, byte={:02x}",
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
