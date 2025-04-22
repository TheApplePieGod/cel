use std::ffi::c_void;

use glfw::PWindow;
use cocoa::foundation::{NSRect, NSUInteger};
use cocoa::appkit::{CGFloat, NSApplicationPresentationOptions, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility, NSWindowToolbarStyle};
use cocoa::base::{id, nil, BOOL, NO, YES};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel, Class};

#[allow(unexpected_cfgs)]
fn handle_window_delegate(ns_window: *mut Object) {
    // Update the window delegate so we can control fullscreen options.
    // Specifically, we need to hide the toolbar

    unsafe {
        // Get the existing delegate
        let existing_delegate: id = msg_send![ns_window, delegate];
        
        // Get the class of the existing delegate
        let delegate_class: *mut Class = msg_send![existing_delegate, class];
        
        unsafe extern "C" fn window_will_use_fullscreen_presentation_options(
            this: &Object, 
            _sel: Sel, 
            _window: id, 
            _proposed_options: NSUInteger
        ) -> NSUInteger {
            let mut opts: NSApplicationPresentationOptions = Default::default();
            opts |= NSApplicationPresentationOptions::NSApplicationPresentationAutoHideToolbar;
            opts |= NSApplicationPresentationOptions::NSApplicationPresentationAutoHideMenuBar;
            opts |= NSApplicationPresentationOptions::NSApplicationPresentationFullScreen;
            opts.bits()
        }

        // Inject the method
        let method: *mut c_void = std::mem::transmute(
            window_will_use_fullscreen_presentation_options as unsafe extern "C" fn(&Object, Sel, id, NSUInteger) -> NSUInteger
        );
        let selector = sel!(window:willUseFullScreenPresentationOptions:);
        let _ = objc::runtime::class_addMethod(
            delegate_class,
            selector,
            std::mem::transmute(method),
            b"@:@Q\0".as_ptr() as *const i8
        );
    }
}

#[allow(unexpected_cfgs)]
pub fn setup_platform_window(window: &PWindow) {
    let ns_window = window.get_cocoa_window() as id;
    
    unsafe {
        let () = msg_send![ns_window, setTitleVisibility: NSWindowTitleVisibility::NSWindowTitleHidden];
        let () = msg_send![ns_window, setTitlebarAppearsTransparent: YES];
        let () = msg_send![ns_window, setMovableByWindowBackground: NO];

        // Cover titlebar with window content
        let current_style: NSWindowStyleMask = msg_send![ns_window, styleMask];
        let new_style = current_style | NSWindowStyleMask::NSFullSizeContentViewWindowMask;
        let () = msg_send![ns_window, setStyleMask:new_style];

        // Setup toolbar & merge with titlebar for for more padding
        let tb_id: id = msg_send![class!(NSString), alloc];
        let tb_id: id = msg_send![tb_id, initWithUTF8String:"toolbar"];
        let toolbar: id = msg_send![class!(NSToolbar), alloc];
        let toolbar: id = msg_send![toolbar, initWithIdentifier: tb_id];
        let () = msg_send![toolbar, setShowsBaselineSeparator: NO];
        let () = msg_send![ns_window, setToolbar: toolbar];
        let () = msg_send![ns_window, setToolbarStyle: NSWindowToolbarStyle::NSWindowToolbarStyleUnifiedCompact];

        // Add shadow for visiblity
        let () = msg_send![ns_window, setHasShadow: YES];

        handle_window_delegate(ns_window);
    }
}

#[allow(unexpected_cfgs)]
pub fn get_titlebar_height_px(window: &PWindow) -> f32 {
    let ns_window = window.get_cocoa_window() as id;

    unsafe {
        // Full window frame (includes titlebar and border)
        let frame: NSRect = msg_send![ns_window, frame];

        // Layout rect (area available to your content view, minus titlebar & toolbars)
        let layout_rect: NSRect = msg_send![ns_window, contentLayoutRect];

        // The difference in height is the titlebar + toolbar area
        (frame.size.height - layout_rect.size.height) as f32
    }
}

#[allow(unexpected_cfgs)]
pub fn get_titlebar_decoration_width_px(window: &PWindow, fullscreen: bool) -> f32 {
    if fullscreen {
        // No buttons visible in fullscreen
        return 0.0;
    }

    let ns_window = window.get_cocoa_window() as id;

    unsafe {
        let close_button: id = msg_send![ns_window, standardWindowButton:0]; // NSWindowCloseButton
        let zoom_button: id = msg_send![ns_window, standardWindowButton:2]; // NSWindowZoomButton
        
        if close_button != nil && zoom_button != nil {
            let close_frame: NSRect = msg_send![close_button, frame];
            let zoom_frame: NSRect = msg_send![zoom_button, frame];
            
            // Calculate the area from window edge to right edge of zoom button
            // The leftmost button (close) starts at x position and 
            // rightmost button (zoom) ends at its x + width
            let left_edge = close_frame.origin.x;
            let right_edge = zoom_frame.origin.x + zoom_frame.size.width;
            
            let padding: CGFloat = left_edge;
            let total_inset = right_edge + padding;
            
            return total_inset as f32;
        }
        
        // Fallback value if we can't access the buttons
        return 70.0;
    }
}

#[allow(unexpected_cfgs)]
pub fn set_draggable(window: &PWindow, draggable: bool) {
    let ns_window = window.get_cocoa_window() as id;
    unsafe { ns_window.setMovable_(draggable) }
}
