// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Linux-specific fix for WebKitGTK rendering/input issues
    #[cfg(target_os = "linux")]
    {
        // Fix for "Events queue growing too big" / IBus issues
        // Forces simple input method (no IBus/Fcitx) which fixes "cant type"
        std::env::set_var("GTK_IM_MODULE", "gtk-im-context-simple");
        
        // Fix for common WebKitGTK rendering/input glitches
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    tandem_lib::run()
}
