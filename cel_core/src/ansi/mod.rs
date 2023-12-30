use vte::Parser;

pub use self::ansi_produce::*;
pub use self::ansi_consume::*;

mod ansi_produce;
mod ansi_consume;

pub type Color = [f32; 3];
pub type Cursor = [usize; 2];
type SignedCursor = [isize; 2];

#[derive(Clone, Copy)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar
}

#[derive(Clone, Copy)]
pub struct CursorState {
    pub style: CursorStyle,
    pub visible: bool,
    pub blinking: bool
}

#[derive(Clone, Copy)]
pub enum ColorWeight {
    Normal,
    Bold,
    Faint
}

#[derive(Clone, Copy)]
pub struct ColorState {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub weight: ColorWeight
}

#[derive(Default, Clone, Copy)]
pub struct ScreenBufferElement {
    pub elem: char,
    pub fg_color: Option<Color>, // TODO: move this out
    pub bg_color: Option<Color> // TODO: move this out
}

pub type ScreenBuffer = Vec<Vec<ScreenBufferElement>>;

#[derive(Clone)]
pub struct TerminalState {
    pub screen_buffer: ScreenBuffer,
    pub cursor_state: CursorState,
    pub color_state: ColorState,
    pub global_cursor_home: Cursor, // Location of (0, 0) in the screen buffer
    pub global_cursor: Cursor,
    pub screen_cursor: Cursor,
}

#[derive(Default)]
struct Performer {
    pub screen_width: usize,
    pub screen_height: usize,
    pub output_stream: Vec<u8>,
    pub terminal_state: TerminalState,
    action_performed: bool
}

pub struct AnsiHandler {
    performer: Performer,
    state_machine: Parser,
}

pub struct AnsiBuilder {
    buffer: Vec<u8>
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            style: CursorStyle::Block,
            visible: true,
            blinking: true
        }
    }
}

impl Default for ColorState{
    fn default() -> Self {
        Self {
            foreground: None,
            background: None,
            weight: ColorWeight::Normal
        }
    }
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            screen_buffer: Default::default(),
            color_state: Default::default(),
            cursor_state: Default::default(),
            global_cursor_home: [0, 0],
            global_cursor: [0, 0],
            screen_cursor: [0, 0]
        }
    }
}
