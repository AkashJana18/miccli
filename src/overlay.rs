//! Global floating overlay for whisperflow mode.
//! Visible on opencode / any terminal, not confined to daemon's tty.

#![allow(unexpected_cfgs)]

use std::time::Duration;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum OverlayMsg {
    Show,
    Hide,
    SetRecording(bool),
    SetPaused(bool),
    Waveform(String),
    Transcription(String),
    Status(String),
}

pub struct OverlayHandle;
impl OverlayHandle {
    pub fn show(&self) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(|| unsafe { do_show() });
    }
    pub fn hide(&self) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(|| unsafe { do_hide() });
    }
    pub fn set_recording(&self, v: bool) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(move || unsafe { do_set_recording(v) });
    }
    pub fn set_paused(&self, v: bool) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(move || unsafe { do_set_paused(v) });
    }
    pub fn waveform(&self, s: String) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(move || unsafe { do_waveform(&s) });
    }
    pub fn transcription(&self, s: String) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(move || unsafe { do_transcription(&s) });
    }
    pub fn status(&self, s: String) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(move || unsafe { do_status(&s) });
    }
}
impl Drop for OverlayHandle {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        dispatch::Queue::main().exec_async(|| unsafe { do_hide() });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn spawn() -> Option<OverlayHandle> {
    None
}

#[cfg(target_os = "macos")]
pub fn spawn() -> Option<OverlayHandle> {
    use std::sync::mpsc;
    // Create window on main queue so it is visible on opencode/other terminals (global HUD).
    // Use async dispatch so we don't deadlock if called from main thread or when main queue isn't pumping
    // (e.g., during `cargo test`); we wait with timeout and fallback to None → terminal overlay.
    let (tx, rx) = mpsc::channel::<Option<bool>>();
    dispatch::Queue::main().exec_async(move || unsafe {
        let ok = create_window();
        let _ = tx.send(ok);
    });
    let ok = rx.recv_timeout(Duration::from_secs(2)).ok()??;
    if ok {
        Some(OverlayHandle)
    } else {
        None
    }
}

// --- macOS window state (main queue only) ---
#[cfg(target_os = "macos")]
mod imp {

    use cocoa::base::id;
    use std::sync::OnceLock;

    pub(super) struct WindowState {
        pub window: id,
        pub tf_top: id,
        pub tf_wave: id,
        pub tf_bottom: id,
    }
    unsafe impl Send for WindowState {}
    unsafe impl Sync for WindowState {}

    pub(super) static WINDOW: OnceLock<WindowState> = OnceLock::new();
    pub(super) static VISIBLE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
}

#[cfg(target_os = "macos")]
unsafe fn create_window() -> Option<bool> {
    use cocoa::appkit::{NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowStyleMask};
    use cocoa::base::{id, nil, NO, YES};
    use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize};
    use imp::{WindowState, WINDOW};

    if WINDOW.get().is_some() {
        return Some(true);
    }
    let _pool = NSAutoreleasePool::new(nil);

    let screen: id = NSScreen::mainScreen(nil);
    let screen_frame: NSRect = if screen != nil {
        use objc::{msg_send, sel, sel_impl};
        msg_send![screen, frame]
    } else {
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 900.))
    };

    let win_w: f64 = 560.0;
    let win_h: f64 = 96.0;
    let win_x = screen_frame.origin.x + (screen_frame.size.width - win_w) / 2.0;
    let win_y = screen_frame.origin.y + screen_frame.size.height - win_h - 28.0;
    let rect = NSRect::new(NSPoint::new(win_x, win_y), NSSize::new(win_w, win_h));

    let window: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
        rect,
        NSWindowStyleMask::NSBorderlessWindowMask,
        NSBackingStoreType::NSBackingStoreBuffered,
        NO,
    );
    if window == nil {
        tracing::warn!("overlay: failed to create NSWindow");
        return None;
    }

    {
        use objc::{msg_send, sel, sel_impl};
        let _: () = msg_send![window, setLevel: 1000i64];
        let _: () = msg_send![window, setOpaque: NO];
        let _: () = msg_send![window, setHasShadow: YES];
        let _: () = msg_send![window, setIgnoresMouseEvents: NO];
        let _: () = msg_send![window, setReleasedWhenClosed: NO];
        let _: () = msg_send![window, setHidesOnDeactivate: NO];
        let _: () = msg_send![window, setCollectionBehavior: 1u64 << 0];
        let bg: id = NSColor::colorWithRed_green_blue_alpha_(nil, 0.11, 0.11, 0.13, 0.92);
        let _: () = msg_send![window, setBackgroundColor: bg];
        let content: id = window.contentView();
        let _: () = msg_send![content, setWantsLayer: YES];
        let layer: id = msg_send![content, layer];
        if layer != nil {
            let _: () = msg_send![layer, setCornerRadius: 12.0f64];
            let _: () = msg_send![layer, setMasksToBounds: YES];
        }

        let tf_top: id = make_label(
            NSRect::new(
                NSPoint::new(14., win_h - 20.),
                NSSize::new(win_w - 28., 16.),
            ),
            "miccli — idle",
            11.0,
        );
        let _: () = msg_send![content, addSubview: tf_top];

        let tf_wave: id = make_label(
            NSRect::new(
                NSPoint::new(14., win_h - 50.),
                NSSize::new(win_w - 28., 28.),
            ),
            "",
            18.0,
        );
        let _: () = msg_send![content, addSubview: tf_wave];

        let tf_bottom: id = make_label(
            NSRect::new(NSPoint::new(14., 10.), NSSize::new(win_w - 28., 28.)),
            "Hold Shift+Control to talk",
            11.0,
        );
        let _: () = msg_send![tf_bottom, setLineBreakMode: 0u64];
        let _: () = msg_send![content, addSubview: tf_bottom];

        let _: () = msg_send![window, orderOut: nil];

        let state = WindowState {
            window,
            tf_top,
            tf_wave,
            tf_bottom,
        };
        let _ = WINDOW.set(state);
    }

    Some(true)
}

#[cfg(target_os = "macos")]
unsafe fn do_show() {
    use imp::{VISIBLE, WINDOW};
    use objc::{msg_send, sel, sel_impl};
    if let Some(s) = WINDOW.get() {
        let window: cocoa::base::id = s.window;
        let _: () = msg_send![window, orderFrontRegardless];
        VISIBLE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_hide() {
    use cocoa::base::nil;
    use imp::{VISIBLE, WINDOW};
    use objc::{msg_send, sel, sel_impl};
    if let Some(s) = WINDOW.get() {
        let window: cocoa::base::id = s.window as _;
        let _: () = msg_send![window, orderOut: nil];
        VISIBLE.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_set_recording(v: bool) {
    use cocoa::appkit::NSColor;
    use cocoa::base::nil;
    use imp::WINDOW;
    use objc::{msg_send, sel, sel_impl};
    if let Some(s) = WINDOW.get() {
        let tf: cocoa::base::id = s.tf_top as _;
        let txt = if v {
            "● REC — hold to talk"
        } else {
            "■ idle"
        };
        set_label(tf, txt);
        let col: cocoa::base::id = if v {
            NSColor::colorWithRed_green_blue_alpha_(nil, 1.0, 0.35, 0.35, 1.0)
        } else {
            NSColor::colorWithRed_green_blue_alpha_(nil, 0.75, 0.75, 0.78, 1.0)
        };
        let _: () = msg_send![tf, setTextColor: col];
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_set_paused(v: bool) {
    use cocoa::appkit::NSColor;
    use cocoa::base::nil;
    use imp::WINDOW;
    use objc::{msg_send, sel, sel_impl};
    if let Some(s) = WINDOW.get() {
        let tf: cocoa::base::id = s.tf_top as _;
        let txt = if v {
            "⏸ paused — toggle to resume"
        } else {
            "miccli — idle"
        };
        set_label(tf, txt);
        if v {
            let col: cocoa::base::id =
                NSColor::colorWithRed_green_blue_alpha_(nil, 1.0, 0.82, 0.2, 1.0);
            let _: () = msg_send![tf, setTextColor: col];
        }
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_waveform(s: &str) {
    use imp::WINDOW;
    if let Some(st) = WINDOW.get() {
        let tf: cocoa::base::id = st.tf_wave as _;
        set_label(tf, s);
        // color cyan
        use cocoa::appkit::NSColor;
        use cocoa::base::nil;
        use objc::{msg_send, sel, sel_impl};
        let col: cocoa::base::id =
            NSColor::colorWithRed_green_blue_alpha_(nil, 0.35, 0.85, 1.0, 1.0);
        let _: () = msg_send![tf, setTextColor: col];
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_transcription(s: &str) {
    use imp::WINDOW;
    if let Some(st) = WINDOW.get() {
        let tf: cocoa::base::id = st.tf_bottom as _;
        let display = if s.is_empty() {
            "".to_string()
        } else {
            format!("\"{}\"", s)
        };
        set_label(tf, &display);
    }
}
#[cfg(target_os = "macos")]
unsafe fn do_status(s: &str) {
    use imp::WINDOW;
    if let Some(st) = WINDOW.get() {
        let tf: cocoa::base::id = st.tf_bottom as _;
        // Only set if transcription is empty (caller checks), but we set anyway for simplicity
        // The caller ensures correct priority
        set_label(tf, s);
    }
}

#[cfg(target_os = "macos")]
unsafe fn make_label(rect: cocoa::foundation::NSRect, text: &str, size: f64) -> cocoa::base::id {
    use cocoa::appkit::{NSColor, NSTextField};
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{msg_send, sel, sel_impl};

    let tf: id = NSTextField::alloc(nil).initWithFrame_(rect);
    if tf == nil {
        return nil;
    }
    let ns: id = NSString::alloc(nil).init_str(text);
    let _: () = msg_send![tf, setStringValue: ns];
    let _: () = msg_send![tf, setBezeled: cocoa::base::NO];
    let _: () = msg_send![tf, setDrawsBackground: cocoa::base::NO];
    let _: () = msg_send![tf, setEditable: cocoa::base::NO];
    let _: () = msg_send![tf, setSelectable: cocoa::base::NO];
    // Try monospaced for waveform (size 18), else system font
    let font: id = if (size - 18.0).abs() < 0.1 {
        // Menlo for waveform — taller blocks
        let name = NSString::alloc(nil).init_str("Menlo");
        if let Some(cls) = objc::runtime::Class::get("NSFont") {
            let f: id = msg_send![cls, fontWithName: name size: size];
            if f == nil {
                msg_send![cls, systemFontOfSize: size]
            } else {
                f
            }
        } else {
            nil
        }
    } else {
        if let Some(cls) = objc::runtime::Class::get("NSFont") {
            msg_send![cls, systemFontOfSize: size]
        } else {
            nil
        }
    };
    if font != nil {
        let _: () = msg_send![tf, setFont: font];
    }
    let col: id = NSColor::colorWithRed_green_blue_alpha_(nil, 0.9, 0.9, 0.92, 1.0);
    let _: () = msg_send![tf, setTextColor: col];
    tf
}

#[cfg(target_os = "macos")]
unsafe fn set_label(tf: cocoa::base::id, text: &str) {
    use cocoa::base::nil;
    use cocoa::foundation::NSString;
    use objc::{msg_send, sel, sel_impl};
    if tf == nil {
        return;
    }
    let ns = NSString::alloc(nil).init_str(text);
    let _: () = msg_send![tf, setStringValue: ns];
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn manual_show() {
        let Some(h) = spawn() else {
            println!("no overlay (not macos)");
            return;
        };
        println!("overlay spawned, showing 4s — look for floating window at top-center...");
        h.show();
        h.set_recording(true);
        h.waveform("▁▂▃▄▅▆▇█▂▃▄▅▆▇█▁▂▃▄▅▆".to_string());
        h.transcription("Hello overlay world".to_string());
        h.status("● REC — manual test".to_string());
        std::thread::sleep(std::time::Duration::from_secs(4));
        println!("hiding...");
        h.hide();
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("done — if you saw a floating window at top-center, native overlay works");
    }
}
