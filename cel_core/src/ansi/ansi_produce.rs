use super::AnsiBuilder;

impl AnsiBuilder {
    pub fn new() -> AnsiBuilder {
        Self {
            buffer: vec![]
        }
    }

    // Raw

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
            formatted.join(";"),
            match intermediate {
                Some(c) => c.to_string(),
                None => String::new()
            },
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
