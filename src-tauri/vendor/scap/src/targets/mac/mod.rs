use cidre::{cg, sc};
use cocoa::appkit::NSScreen;
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use futures::executor::block_on;
use objc::{msg_send, sel, sel_impl};

use crate::engine::mac::ext::DirectDisplayIdExt;

use super::{Display, Target};

fn get_display_name(display_id: cg::DirectDisplayId) -> String {
    unsafe {
        // Get all screens
        let screens: id = NSScreen::screens(nil);
        let count: u64 = msg_send![screens, count];

        for i in 0..count {
            let screen: id = msg_send![screens, objectAtIndex: i];
            let device_description: id = msg_send![screen, deviceDescription];
            let display_id_number: id = msg_send![device_description, objectForKey: NSString::alloc(nil).init_str("NSScreenNumber")];
            let display_id_number: u32 = msg_send![display_id_number, unsignedIntValue];

            if display_id_number == display_id.0 {
                let localized_name: id = msg_send![screen, localizedName];
                let name: *const i8 = msg_send![localized_name, UTF8String];
                return std::ffi::CStr::from_ptr(name)
                    .to_string_lossy()
                    .into_owned();
            }
        }

        format!("Unknown Display {}", display_id.0)
    }
}

pub fn get_all_targets() -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();

    let content = block_on(sc::ShareableContent::current()).unwrap();

    // Add displays to targets
    for display in content.displays().iter() {
        let id = display.display_id();

        let title = get_display_name(id);

        let target = Target::Display(super::Display {
            id: id.0,
            title,
            raw_handle: id,
        });

        targets.push(target);
    }

    // Add windows to targets
    for window in content.windows().iter() {
        let id = window.id();
        let frame = window.frame();
        if !window.is_on_screen() || frame.size.width <= 0.0 || frame.size.height <= 0.0 {
            continue;
        }
        let title = window
            .title()
            // on intel chips we can have Some but also a null pointer for some reason
            .filter(|v| !unsafe { v.utf8_chars_ar().is_null() });

        let target = Target::Window(super::Window {
            id,
            title: title.map(|v| v.to_string()).unwrap_or_default(),
            raw_handle: id,
        });
        targets.push(target);
    }

    targets
}

pub fn get_main_display() -> Display {
    let id = cg::direct_display::Id::main();
    let title = get_display_name(id);

    Display {
        id: id.0,
        title,
        raw_handle: id,
    }
}

pub fn get_scale_factor(target: &Target) -> f64 {
    match target {
        Target::Window(_) => 1.0,
        Target::Display(display) => {
            let mode = display.raw_handle.display_mode().unwrap();
            (mode.pixel_width() / mode.width()) as f64
        }
    }
}

pub fn get_target_dimensions(target: &Target) -> (u64, u64) {
    match target {
        Target::Window(window) => window_dimensions(window.raw_handle).unwrap_or((0, 0)),
        Target::Display(display) => {
            let mode = display.raw_handle.display_mode().unwrap();
            (mode.width(), mode.height())
        }
    }
}

fn window_dimensions(window_id: cg::WindowId) -> Option<(u64, u64)> {
    let content = block_on(sc::ShareableContent::current()).ok()?;
    let windows = content.windows();
    let window = windows.iter().find(|window| window.id() == window_id)?;
    let frame = window.frame();
    let width = frame.size.width.max(0.0) as u64;
    let height = frame.size.height.max(0.0) as u64;

    if width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}
