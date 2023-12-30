use std::{cell::RefCell, rc::Rc};

thread_local!(static APP_STATE: Rc<RefCell<AppState>> = Rc::new(RefCell::new(AppState::new())));

pub struct AppState {
    pub running: bool
}

impl AppState {
    fn new() -> Self {
        Self {
            running: true
        }
    }

    pub fn current() -> Rc<RefCell<AppState>> {
        APP_STATE.with(|s| s.clone())
    }
}
