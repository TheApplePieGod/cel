use super::AnsiBuilder;

impl AnsiBuilder {
    pub fn new() -> AnsiBuilder {
        Self {
            buffer: vec![]
        }
    }

    // Print

    pub fn add_text(self, text: &str) -> Self {
        self.add_raw(text.as_bytes())
    }

    // Execute characters

    pub fn add_newline(self) -> Self {
        self.add_raw(&[b'\n'])
    }

    pub fn add_carriage_return(self) -> Self {
        self.add_raw(&[b'\r'])
    }

    pub fn add_cr_and_newline(self) -> Self {
        self.add_carriage_return().add_newline()
    }

    pub fn add_backspace(self) -> Self {
        self.add_raw(&[0x08])
    }

    // Cursor

    pub fn move_cursor_up(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'A')
    }

    pub fn move_cursor_down(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'B')
    }

    pub fn move_cursor_right(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'C')
    }

    pub fn move_cursor_left(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'D')
    }

    pub fn set_cursor_pos(self, x: u32, y: u32) -> Self {
        self.add_raw_csi(&[y, x], None, 'H')
    }

    pub fn reset_cursor_pos(self) -> Self {
        self.set_cursor_pos(1, 1)
    }

    pub fn reverse_index(self) -> Self {
        self.add_raw(&[0x1b, b'M'])
    }

    // Margins

    pub fn set_scroll_margin_y(self, top: u32, bottom: u32) -> Self {
        self.add_raw_csi(&[top, bottom], None, 'r')
    }

    pub fn set_scroll_margin_x(self, left: u32, right: u32) -> Self {
        self.add_raw_csi(&[left, right], None, 's')
    }

    // Lines

    pub fn insert_lines(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'L')
    }

    pub fn remove_lines(self, amount: u32) -> Self {
        self.add_raw_csi(&[amount], None, 'M')
    }

    // Modes

    pub fn enable_mode(self, mode: u32) -> Self {
        self.add_raw_csi(&[mode], Some('?'), 'h')
    }

    pub fn disable_mode(self, mode: u32) -> Self {
        self.add_raw_csi(&[mode], Some('?'), 'l')
    }

    pub fn enable_wrap(self) -> Self {
        self.enable_mode(7)
    }

    pub fn disable_wrap(self) -> Self {
        self.disable_mode(7)
    }

    // Raw

    pub fn add_raw_csi(
        self,
        params: &[u32],
        intermediate: Option<char>,
        end_char: char
    ) -> Self {
        let formatted: Vec<String> = params.iter().map(|p| p.to_string()).collect();
        self.add_raw(format!(
            "\x1b[{}{}{}",
            match intermediate {
                Some(c) => c.to_string(),
                None => String::new()
            },
            formatted.join(";"),
            end_char
        ).as_bytes())
    }

    pub fn add_raw(mut self, raw: &[u8]) -> Self {
        self.buffer.extend_from_slice(raw);
        self
    }

    // Build

    pub fn build_stream(self) -> Vec<u8> {
        self.buffer
    }
}
