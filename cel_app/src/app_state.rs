use std::{cell::RefCell, rc::Rc};
use cel_renderer::{font::Font, resources::get_resource_path};

thread_local!(static APP_STATE: Rc<RefCell<AppState>> = Rc::new(RefCell::new(AppState::new())));

pub struct AppState {
    pub running: bool,
    pub font: Rc<RefCell<Font>>
}

impl AppState {
    fn new() -> Self {
        let priority_fonts = [
            get_resource_path("MartianMonoRegular.ttf"),
            get_resource_path("HackNerdMonoRegular.ttf"),
        ];

        let primary_font = match Font::new("Martian Mono", &priority_fonts) {
            Ok(font) => font,
            Err(msg) => { panic!("{}", msg) }
        };

        log::info!("Primary font '{}'", primary_font.get_primary_name());

        Self {
            running: true,
            font: Rc::new(RefCell::new(primary_font))
        }
    }

    pub fn current() -> Rc<RefCell<AppState>> {
        APP_STATE.with(|s| s.clone())
    }
}
