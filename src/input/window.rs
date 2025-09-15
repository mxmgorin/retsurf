use sdl2::event::WindowEvent;

use crate::app::AppCmd;

pub fn handle_window(win_event: WindowEvent) -> Vec<AppCmd> {
    match win_event {
        WindowEvent::Close => vec![AppCmd::Quit],
        WindowEvent::Resized(w, h) | WindowEvent::SizeChanged(w, h) => vec![AppCmd::Resize(w as u32, h as u32)],
        WindowEvent::None
        | WindowEvent::Shown
        | WindowEvent::Hidden
        | WindowEvent::Exposed
        | WindowEvent::Moved(_, _)
        | WindowEvent::Minimized
        | WindowEvent::Maximized
        | WindowEvent::Restored
        | WindowEvent::Enter
        | WindowEvent::Leave
        | WindowEvent::FocusGained
        | WindowEvent::FocusLost
        | WindowEvent::TakeFocus
        | WindowEvent::HitTest
        | WindowEvent::ICCProfChanged
        | WindowEvent::DisplayChanged(_) => vec![],
    }
}
