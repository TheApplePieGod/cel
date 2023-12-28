use vte::{Params, Parser, Perform};
use std::fmt;

use crate::font::{Font, RenderType};
use crate::renderer::{RenderState, Vertex};
use crate::util::Util;

type Cursor = [usize; 2];

#[derive(Default)]
struct ColorState {
    foreground: Option<[f32; 3]>,
    background: Option<[f32; 3]>
}

#[derive(Default)]
struct Performer {
    pub color_state: ColorState,
    pub screen_buffer: Vec<Vec<char>>,
    pub screen_offset: usize,
    pub screen_width: usize,
    pub screen_height: usize,
    pub cursor: Cursor,
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

    pub fn handle_sequence(&mut self, seq: &Vec<String>) {
        for string in seq {
            for c in string.bytes() {
                self.state_machine.advance(&mut self.performer, c);
            }
        }
    }

    pub fn update_screen_offset(&mut self, x: u32, y: u32) {
        self.performer.screen_offset = y as usize;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.performer.screen_width = width as usize;
        self.performer.screen_height = height as usize;
    }

    pub fn get_screen_buffer(&self) -> &Vec<Vec<char>> {
        &self.performer.screen_buffer
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

    fn get_max_screen_pos(&self) -> (u16, u16) {
        (self.screen_width as u16 - 1, self.screen_height as u16 - 1)
    }

    /// Top left position is (0, 0)
    fn get_cursor_pos_from_screen(
        &mut self,
        target_x: u16,
        target_y: u16
    ) -> Cursor {
        log::warn!(
            "Target: {}/{}, {}/{}",
            target_x,
            self.screen_width,
            target_y,
            self.screen_height
        );
        let target_x = target_x as usize;
        let target_y = target_y as usize;
        let mut screen_x = 0;
        let mut screen_y = 0;
        let mut line_y = 0;
        loop {
            let line_idx = line_y + self.screen_offset;

            // Can figure out exactly where to go if the lines are empty
            if line_idx >= self.screen_buffer.len() {
                log::warn!(
                    "Target: {}, Screen: {}",
                    target_y,
                    screen_y
                );
                return [target_x, line_idx + (target_y - screen_y)];
            }

            // TODO: this could definitely be optimized
            let line = &self.screen_buffer[line_idx];
            for char_idx in 0..line.len() {
                if screen_y == target_y {
                    return [char_idx + (target_x - screen_x), line_y];
                }

                if screen_x >= self.screen_width {
                    screen_x = 0;
                    screen_y += 1;
                }
            }

            // Should only happen if line is empty
            if screen_y == target_y {
                return [target_x - screen_x, line_y];
            }

            screen_x = 0;
            screen_y += 1;
            line_y += 1;
        }
    }

    fn set_cursor_pos_from_screen(&mut self, target_x: u16, target_y: u16) {
        self.cursor = self.get_cursor_pos_from_screen(target_x, target_y);
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
    }
}

impl Perform for Performer {
    fn print(&mut self, c: char) {
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

        while self.cursor[1] >= self.screen_buffer.len() {
            self.screen_buffer.push(vec![]);
        }
        let buffer_line = &mut self.screen_buffer[self.cursor[1]];
        while self.cursor[0] >= buffer_line.len() {
            buffer_line.push(' ');
        }

        buffer_line[self.cursor[0]] = c;

        // Advance the cursor
        self.cursor[0] += 1;

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
        //println!("[execute] {:02x}", byte);
        match byte {
            b'\n' => {
                self.cursor[0] = 0;
                self.cursor[1] += 1;
                /*
                self.perform_state.line_count += 1;
                self.perform_state.line_char_count = 0;
                self.perform_state.x_pos = self.render_state.base_x;
                self.perform_state.y_pos -= self.render_state.face_metrics.height;
                */
            },
            _ => {}
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        println!(
            "[hook] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
            params, intermediates, ignore, c
        );
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
        let params = self.parse_params(params);
        match c {
            'm' => { // Graphics
                //self.perform_state.color_state = self.parse_color_escape(params.iter());
                //log::debug!("{:?}", self.color_state);
            },
            'H' | 'f' => { // Place cursor
                match params.len() {
                    0 => self.set_cursor_pos_from_screen(0, 0),
                    2 => self.set_cursor_pos_from_screen(params[0], params[1]),
                    _ => {}
                }
            },
            'J' => { // Erase in display
                let max_pos = self.get_max_screen_pos();
                let max_cursor = self.get_cursor_pos_from_screen(max_pos.0, max_pos.1);
                let min_cursor = self.get_cursor_pos_from_screen(0, 0);
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(self.cursor, max_cursor),
                    1 => self.erase(self.cursor, min_cursor),
                    2 => {
                        self.erase(min_cursor, max_cursor);
                        self.cursor = min_cursor;
                    },
                    3 => {}
                    _ => {}
                }
            },
            'K' => { // Erase in line
                let code = match params.len() {
                    1 => params[0],
                    _ => 0
                };
                match code {
                    0 => self.erase(self.cursor, [std::usize::MAX, self.cursor[1]]),
                    1 => self.erase(self.cursor, [0, self.cursor[1]]),
                    2 => self.erase([0, self.cursor[1]], [std::usize::MAX, self.cursor[1]]),
                    _ => {}
                }
            },
            'n' => { // Device status report
                
            }
            _ => {
                println!(
                    "[csi_dispatch] params={:#?}, intermediates={:?}, ignore={:?}, char={:?}",
                    params, intermediates, ignore, c
                );
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
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
