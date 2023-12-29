use vte::{Params, Parser, Perform};
use std::fmt;

use crate::font::{Font, RenderType};
use crate::renderer::{RenderState, Vertex};
use crate::util::Util;

type Cursor = [usize; 2];
type SignedCursor = [isize; 2];

#[derive(Default)]
struct ColorState {
    foreground: Option<[f32; 3]>,
    background: Option<[f32; 3]>
}

#[derive(Default)]
struct Performer {
    pub color_state: ColorState,
    pub screen_buffer: Vec<Vec<char>>,
    pub screen_width: usize,
    pub screen_height: usize,
    pub output_stream: Vec<u8>,
    global_cursor_home: Cursor, // Location of (0, 0) in the screen buffer
    global_cursor: Cursor,
    screen_cursor: Cursor,
    action_performed: bool
}

pub struct AnsiHandler {
    performer: Performer,
    state_machine: Parser,
}

impl AnsiHandler {
    pub fn new() -> Self {
        Self {
            performer: Default::default(),
            state_machine: Parser::new()
        }
    }

    pub fn handle_sequence(&mut self, seq: &[String]) -> Option<(u32, u32)> {
        self.performer.action_performed = false;
        for (i, string) in seq.iter().enumerate() {
            for (j, c) in string.bytes().enumerate() {
                self.state_machine.advance(&mut self.performer, c);
                if self.performer.action_performed {
                    return Some((i as u32, j as u32))
                }
            }
        }

        None
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.performer.screen_width = width as usize;
        self.performer.screen_height = height as usize;
    }

    pub fn get_screen_buffer(&self) -> &Vec<Vec<char>> {
        &self.performer.screen_buffer
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

    fn parse_16_bit_color(&self, bold: bool, code: u16) -> [f32; 3] {
        let factor: f32 = match bold {
            true => 1.0,
            false => 0.5
        };
        let one = (code & 1) as f32 * factor;
        let two = ((code & 2) >> 1) as f32 * factor;
        let four = ((code & 4) >> 2) as f32 * factor;
        match code {
            1..=6 => [one, two, four],
            0     => match bold {
                true => [0.5, 0.5, 0.5],
                false => [0.0, 0.0, 0.0]
            },
            7     => match bold {
                true => [1.0, 1.0, 1.0],
                false => [0.75, 0.75, 0.75]
            }
            _ => [0.0, 0.0, 0.0]
        }
    }

    fn parse_color_escape(&self, params: &Vec<u16>) -> ColorState {
        let mut state: ColorState = Default::default();

        let mut is_bold = false;
        for code in params {
            match code {
                1 => is_bold = true,
                30..=37 => state.foreground = Some(self.parse_16_bit_color(is_bold, code - 30)),
                40..=47 => state.background = Some(self.parse_16_bit_color(is_bold, code - 40)),
                90..=97   => state.foreground = Some(self.parse_16_bit_color(true, code - 90)),
                100..=107 => state.background = Some(self.parse_16_bit_color(true, code - 100)),
                38 => state.foreground = None,
                39 => state.background = None,
                _ => {}
            }
        }

        state
    }

    /// Top left position is (0, 0)
    fn compute_new_cursor_pos(
        &self,
        cur_screen: Cursor,
        cur_global: Cursor,
        target_screen: Cursor
    ) -> Cursor {
        let target_screen = [
            target_screen[0].clamp(0, self.screen_width - 1),
            target_screen[1].clamp(0, self.screen_height - 1)
        ];
        let mut cur_screen = cur_screen;
        let mut cur_global = cur_global;
        loop {
            // Can figure out exactly where to go if the lines are empty
            if cur_global[1] >= self.screen_buffer.len() {
                return [
                    target_screen[0],
                    cur_global[1] + (target_screen[1] - cur_screen[1])
                ];
            }

            // TODO: this could definitely be optimized
            let line = &self.screen_buffer[cur_global[1]];
            for char_idx in cur_global[0]..line.len() {
                if cur_screen[1] == target_screen[1] {
                    return [
                        char_idx + (target_screen[0] - cur_screen[0]),
                        cur_global[1]
                    ];
                }

                cur_screen[0] += 1;
                if cur_screen[0] >= self.screen_width {
                    cur_screen[0] = 0;
                    cur_screen[1] += 1;
                }
            }

            // Should only happen if line is empty
            if cur_screen[1] == target_screen[1] {
                return [
                    target_screen[0] - cur_screen[0],
                    cur_global[1]
                ];
            }

            cur_screen[0] = 0;
            cur_screen[1] += 1;
            cur_global[1] += 1;
        }
    }

    /// Get the global cursor pos from an absolute screen position
    fn get_cursor_pos_absolute(&self, screen_pos: &Cursor) -> Cursor {
        self.compute_new_cursor_pos(
            [0, 0],
            self.global_cursor_home,
            *screen_pos
        )
    }

    fn set_cursor_pos_absolute(&mut self, screen_pos: &Cursor) {
        let old_screen = self.screen_cursor;
        let old_global = self.global_cursor;

        self.global_cursor = self.get_cursor_pos_absolute(screen_pos);
        self.screen_cursor = *screen_pos;

        log::debug!(
            "[set_cursor_pos_absolute{:?}] Screen: {:?} -> {:?}, Global: {:?} -> {:?}",
            screen_pos,
            old_screen, self.screen_cursor,
            old_global, self.global_cursor
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
        let old_screen = self.screen_cursor;
        let old_global = self.global_cursor;

        let target = self.get_relative_screen_cursor(relative_screen);
        self.global_cursor = self.get_cursor_pos_relative(relative_screen);
        self.screen_cursor = target;

        log::debug!(
            "[set_cursor_pos_relative({:?})] Screen: {:?} -> {:?}, Global: {:?} -> {:?}",
            relative_screen,
            old_screen, self.screen_cursor,
            old_global, self.global_cursor
        );
    }

    fn get_max_screen_cursor(&self) -> Cursor {
        [self.screen_width - 1, self.screen_height - 1]
    }

    fn get_relative_screen_cursor(&self, offset: &SignedCursor) -> Cursor {
        let relative = [
            (self.screen_cursor[0] as isize + offset[0]).max(0) as usize,
            (self.screen_cursor[1] as isize + offset[1]).max(0) as usize
        ];

        /*
        log::debug!(
            "[get_relative_screen_cursor({:?})] Screen {:?} -> {:?}",
            offset,
            self.screen_cursor, relative
        );
        */

        relative
    }

    /// Computes the global cursor pos at the start of the current line
    fn get_cursor_pos_sol(&self) -> Cursor {
        [
            self.global_cursor[0] - self.screen_cursor[0],
            self.global_cursor[1]
        ]
    }

    /// Computes the global cursor pos at the end of the current line
    fn get_cursor_pos_eol(&self) -> Cursor {
        [
            self.global_cursor[0] + (self.screen_width - self.screen_cursor[0] - 1),
            self.global_cursor[1]
        ]
    }

    fn erase(&mut self, start: Cursor, end: Cursor) {
        let mut start = start;
        let mut end = end;
        if (start[1] == end[1] && end[0] < start[0]) || start[1] > end[1] {
            (start, end) = (end, start);
        }

        for y in start[1]..=end[1] {
            if y >= self.screen_buffer.len() {
                break;
            }

            let line = &mut self.screen_buffer[y];
            if y == start[1] {
                line.resize(start[0], Default::default());
            } else if y == end[1] {
                // TODO: memset
                for x in 0..=end[0] {
                    if x >= line.len() {
                        break;
                    }
                    line[x] = ' ';
                }
            } else {
                line.clear();
            }
        }

        log::debug!(
            "[erase] Global: {:?} -> {:?}",
            start, end
        );
    }

    /// Advance the screen cursor y by one, potentially scrolling the buffer if
    /// necessary (updating the global cursor)
    fn advance_screen_cursor_y(&mut self) {
        let old_screen = self.screen_cursor;
        let old_home = self.global_cursor_home;

        if self.screen_cursor[1] < self.screen_height - 1 {
            self.screen_cursor[1] += 1;
        } else {
            let global_line = &self.screen_buffer[self.global_cursor_home[1]];
            let still_wrapped = self.global_cursor_home[0] + self.screen_width < global_line.len();
            if still_wrapped {
                self.global_cursor_home[0] += self.screen_width;
            } else {
                self.global_cursor_home[0] = 0;
                self.global_cursor_home[1] += 1;
            }
        }

        log::debug!(
            "[advance_screen_cursor_y] Screen: {:?} -> {:?}, Home: {:?} -> {:?}",
            old_screen, self.screen_cursor,
            old_home, self.global_cursor_home
        );
    }
}

impl Perform for Performer {
    fn print(&mut self, c: char) {
        self.action_performed = true;

        // Filled in with all the rasterized positions of each line,
        // even if there is nothing there. This way we can easily populate
        // the primary buffer just by looking up which line it should be in the
        // raster buffer
        /*
        let raster_buffer: Vec<Vec<[usize; 2]>> = vec![];
        let buffer_offset = raster_buffer[self.cursor[1]][self.cursor[0]];
        let line_offset = render_state.base_y as u32;

        while buffer_offset[1] >= self.screen_buffer.len() {
            self.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut self.screen_buffer[buffer_offset[1]];
        while buffer_offset[0] >= buffer_line.len() {
            buffer_line.push('\0');
        }
        */

        while self.global_cursor[1] >= self.screen_buffer.len() {
            self.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut self.screen_buffer[self.global_cursor[1]];
        while self.global_cursor[0] >= buffer_line.len() {
            buffer_line.push(' ');
        }

        buffer_line[self.global_cursor[0]] = c;

        // Advance the cursor
        self.global_cursor[0] += 1;
        self.screen_cursor[0] += 1;

        if self.screen_cursor[0] >= self.screen_width {
            self.screen_cursor[0] = 0;
            self.advance_screen_cursor_y();
        }

        // Check if the cursor has moved to the next line and will impact the screen.
        // If so, we need to update the raster buffer
        /*
        if self.cursor[0] == buffer_line.len() {
            self.cursor[0] = 0;
            self.cursor[1] += 1;

        }
        */
    }

    fn execute(&mut self, byte: u8) {
        self.action_performed = true;
        log::debug!("Exec [{:?}]", byte as char);

        match byte {
            b'\n' => {
                let old_global = self.global_cursor;

                self.global_cursor[0] = self.global_cursor[0] % self.screen_width;
                self.global_cursor[1] += 1;
                self.advance_screen_cursor_y();

                log::debug!(
                    "[\\n] Global: {:?} -> {:?}",
                    old_global,
                    self.global_cursor
                );
            },
            b'\r' => {
                let old_global = self.global_cursor;

                self.global_cursor = self.get_cursor_pos_sol();
                self.screen_cursor[0] = 0;

                log::debug!(
                    "[\\r] Global: {:?} -> {:?}",
                    old_global,
                    self.global_cursor
                );
            },
            0x07 => { // Bell
                log::debug!("Bell!!!");
            }
            0x08 => { // Backspace
                // Move cursor back by one with back-wrapping
                if self.global_cursor[0] > 0 {
                    self.global_cursor[0] -= 1;
                    if self.screen_cursor[0] > 0 {
                        self.screen_cursor[0] -= 1;
                    } else {
                        self.screen_cursor[0] = self.screen_width - 1;
                        self.screen_cursor[1] -= 1;
                    }
                }
            },
            _ => {
                //println!("[execute] {:02x}", byte);
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
    }

    fn put(&mut self, byte: u8) {
        //println!("[put] {:02x}", byte);
    }

    fn unhook(&mut self) {
        //println!("[unhook]");
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        //println!("[osc_dispatch] params={:?} bell_terminated={}", params, bell_terminated);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        self.action_performed = true;

        let params = self.parse_params(params);
        match c {
            'A' => {
                if params.is_empty() {
                    return;
                }
                log::debug!("Cursor up [{:?}]", params[0]);
                self.set_cursor_pos_relative(&[0, -(params[0] as isize)])
            },
            'B' => {
                if params.is_empty() {
                    return;
                }
                log::debug!("Cursor down [{:?}]", params[0]);
                self.set_cursor_pos_relative(&[0, params[0] as isize])
            },
            'C' => {
                if params.is_empty() {
                    return;
                }
                log::debug!("Cursor right [{:?}]", params[0]);
                self.set_cursor_pos_relative(&[params[0] as isize, 0])
            },
            'D' => {
                if params.is_empty() {
                    return;
                }
                log::debug!("Cursor left [{:?}]", params[0]);
                self.set_cursor_pos_relative(&[-(params[0] as isize), 0])
            },
            'H' | 'f' => { // Place cursor
                log::debug!("Cursor set position");
                match params.len() {
                    0 => self.set_cursor_pos_absolute(&[0, 0]),
                    2 => self.set_cursor_pos_absolute(&[
                        // row, col format
                        params[1] as usize - 1,
                        params[0] as usize - 1
                    ]),
                    _ => {}
                }
            },
            'J' => { // Erase in display
                log::debug!("Erase in display");
                let min_cursor = self.global_cursor_home;
                let max_cursor = self.get_cursor_pos_absolute(
                    &self.get_max_screen_cursor()
                );
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(self.global_cursor, max_cursor),
                    1 => self.erase(self.global_cursor, min_cursor),
                    2 => {
                        self.erase(min_cursor, max_cursor);
                        self.global_cursor = min_cursor;
                        self.screen_cursor = [0, 0];
                    },
                    3 => {}
                    _ => {}
                }
            },
            'K' => { // Erase in line
                log::debug!("Erase in line");
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(
                        self.global_cursor,
                        self.get_cursor_pos_eol()
                    ),
                    1 => self.erase(
                        self.global_cursor,
                        self.get_cursor_pos_sol()
                    ),
                    2 => self.erase(
                        self.get_cursor_pos_sol(),
                        self.get_cursor_pos_eol()
                    ),
                    _ => {}
                }
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
            'h' => {
                if params.len() != 1 {
                    return;
                }
                // TODO: public vs private mode
                log::debug!("Set mode [{:?}]", params[0]);
            },
            'm' => { // Graphics
                //self.perform_state.color_state = self.parse_color_escape(params.iter());
                //log::debug!("{:?}", self.color_state);
            },
            'n' => { // Device status report
                if params.len() != 1 {
                    return;
                }
                match params[0] {
                    5 => {}, // Status report
                    6 => { // Report cursor position
                        let esc_str = format!(
                            "\x1b[{};{}R",
                            self.screen_cursor[1] + 1,
                            self.screen_cursor[0] + 1
                        );
                        self.output_stream.extend_from_slice(esc_str.as_bytes());
                    },
                    _ => {}
                }
            }
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
        log::debug!("Esc [{:?}]", byte as char);

        /*
        println!(
            "[esc_dispatch] intermediates={:?}, ignore={:?}, byte={:02x}",
            intermediates, ignore, byte
        );
        */
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
