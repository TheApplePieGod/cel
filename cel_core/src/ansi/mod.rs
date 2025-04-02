use vte::Parser;
use bitflags::bitflags;

pub use self::ansi_produce::*;
pub use self::ansi_consume::*;

mod ansi_produce;
mod ansi_consume;

pub type Color = [f32; 3];
pub type Cursor = [usize; 2];
type SignedCursor = [isize; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Mouse1 = 0,
    Mouse2 = 2, // Flipped????
    Mouse3 = 1,
    Mouse4 = 64, // Wheel up
    Mouse5 = 65,  // Wheel down
    Mouse6 = 66, // Wheel left
    Mouse7 = 67,  // Wheel right
}

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct KeyboardModifierFlags: u32 {
        const Alt = 2;
        const Shift = 4;
        const Meta = 8;
        const Control = 16;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MouseMode {
    Default,
    UTF8,
    SGR
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTrackingMode {
    Disabled,
    Default,
    ButtonEvent,
    AnyEvent
}

#[derive(Clone, Copy, Debug)]
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

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct StyleFlags: u32 {
        const Bold = 1 << 0;
        const Faint = 1 << 1;
        const Blink = 1 << 2;
        const Italic = 1 << 3;
        const Underline = 1 << 4;
        const Invisible = 1 << 5;
        const CrossedOut = 1 << 6;
    }
}

#[derive(Clone, Copy, Default)]
pub struct StyleState {
    pub flags: StyleFlags,
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>
}

#[derive(Clone, Debug)]
pub enum CellContent {
    Char(char, usize),
    Grapheme(String, usize),
    Continuation(usize),
    Empty
}

#[derive(Clone)]
pub struct ScreenBufferElement {
    pub elem: CellContent,
    pub style: StyleState
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
    pub alt_screen_buffer_state: BufferState,
    pub cursor_state: CursorState,
    pub style_state: StyleState,
    pub background_color: [f32; 3], // Default background color to reset to
    pub margin: Margin,
    pub wants_wrap: bool,
    pub global_cursor_home: Cursor, // Location of (0, 0) in the screen buffer
    pub global_cursor: Cursor,
    pub screen_cursor: Cursor,
    pub mouse_mode: MouseMode,
    pub mouse_tracking_mode: MouseTrackingMode,
    pub bracketed_paste_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    Disabled,
    Enabled,
    Active
}

struct Performer {
    pub screen_width: usize,
    pub screen_height: usize,
    pub output_stream: Vec<u8>,
    pub is_empty: bool,
    pub prompt_id: u32,
    action_performed: bool,

    // State associated with one specific 'terminal' / 'buffer'
    pub terminal_state: TerminalState,
    pub saved_terminal_state: TerminalState,

    // Global state
    ignore_print: bool,
}

pub struct AnsiHandler {
    performer: Performer,
    state_machine: Parser,

    // Store last cell / press state for mouse buttons
    mouse_states: [(Cursor, bool); 256],
    scroll_states: [f32; 2]
}

pub struct AnsiBuilder {
    buffer: Vec<u8>
}

impl Default for ScreenBufferElement {
    fn default() -> Self {
        Self {
            elem: CellContent::Char('\0', 1),
            style: Default::default()
        }
    }
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            style: CursorStyle::Bar,
            visible: true,
            blinking: true
        }
    }
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            screen_buffer: Default::default(),
            alt_screen_buffer_state: BufferState::Enabled,
            cursor_state: Default::default(),
            style_state: Default::default(),
            background_color: [0.0, 0.0, 0.0],
            margin: Default::default(),
            wants_wrap: false,
            global_cursor_home: [0, 0],
            global_cursor: [0, 0],
            screen_cursor: [0, 0],
            mouse_mode: MouseMode::Default,
            mouse_tracking_mode: MouseTrackingMode::Disabled,
            bracketed_paste_enabled: false
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
