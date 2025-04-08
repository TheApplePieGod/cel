use glfw::PWindow;
use cocoa::appkit::{NSWindowStyleMask, NSWindowTitleVisibility};
use cocoa::base::{id, YES};

pub fn setup_platform_window(window: &PWindow) {
    /*
    let ns_window = window.get_cocoa_window() as id;
    
    unsafe {
        let () = msg_send![ns_window, setTitleVisibility:NSWindowTitleVisibility::NSWindowTitleHidden];
        
        let () = msg_send![ns_window, setTitlebarAppearsTransparent:YES];
        
        let current_style: NSWindowStyleMask = msg_send![ns_window, styleMask];
        let new_style = current_style | NSWindowStyleMask::NSFullSizeContentViewWindowMask;
        let () = msg_send![ns_window, setStyleMask:new_style];
    }
    */
}
