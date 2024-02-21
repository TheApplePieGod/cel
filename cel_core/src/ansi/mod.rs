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

#[derive(Default, Clone, Copy)]
pub struct Margin {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize
}

#[derive(Clone)]
pub struct TerminalState {
    pub screen_buffer: ScreenBuffer,
    pub cursor_state: CursorState,
    pub color_state: ColorState,
    pub margin: Margin,
    pub wants_wrap: bool,
    pub global_cursor_home: Cursor, // Location of (0, 0) in the screen buffer
    pub global_cursor: Cursor,
    pub screen_cursor: Cursor,
}

#[derive(Clone, Copy)]
pub enum BufferState {
    Disabled,
    Enabled,
    Active
}

struct Performer {
    pub screen_width: usize,
    pub screen_height: usize,
    pub output_stream: Vec<u8>,
    action_performed: bool,

    // State associated with one specific 'terminal' / 'buffer'
    pub terminal_state: TerminalState,
    pub saved_terminal_state: TerminalState,

    // Global state
    pub alt_screen_buffer_state: BufferState,
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
            margin: Default::default(),
            wants_wrap: false,
            global_cursor_home: [0, 0],
            global_cursor: [0, 0],
            screen_cursor: [0, 0]
        }
    }
}

impl Default for Performer {
    fn default() -> Self {
        Self {
            screen_width: 1,
            screen_height: 1,
            output_stream: vec![],
            action_performed: false,

            terminal_state: Default::default(),
            saved_terminal_state: Default::default(),

            alt_screen_buffer_state: BufferState::Enabled,
        }
    }
}

impl Margin {
    fn get_from_screen_size(width: u32, height: u32) -> Self {
        Self {
            top: 0,
            bottom: height as usize - 1,
            left: 0,
            right: width as usize - 1
        }
    }
}
