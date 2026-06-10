use crate::app::AppCommand;
use sdl2::event::WindowEvent;

pub fn handle_window(win_event: WindowEvent) -> Option<AppCommand> {
    match win_event {
        WindowEvent::Close => Some(AppCommand::Shutdown),
        WindowEvent::Resized(..) | WindowEvent::SizeChanged(..) => Some(AppCommand::Resize),
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
        | WindowEvent::DisplayChanged(_) => None,
    }
}
