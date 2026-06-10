use crate::app::AppCommand;
use sdl2::sys::{SDL_Event, SDL_PushEvent, SDL_UserEvent};

pub fn handle_user(code: i32) -> Option<AppCommand> {
    let event = UserEvent::from_code(code);

    match event {
        UserEvent::BrowserWakeup => None,
        UserEvent::BrowserFrameReady => None,
        // Sent by download workers/interception purely to wake the loop; the
        // per-frame downloads poll in `App::run` picks up the new state.
        UserEvent::DownloadUpdate => None,
        // Sent by the hint-collection JS callback purely to wake the loop; the
        // main loop drains the collected rects.
        UserEvent::HintsReady => None,
    }
}

#[repr(i32)]
#[derive(Copy, Clone)]
pub enum UserEvent {
    BrowserWakeup = 0,
    BrowserFrameReady = 1,
    DownloadUpdate = 2,
    HintsReady = 3,
}

impl UserEvent {
    pub const ALL: [UserEvent; 4] = [
        UserEvent::BrowserWakeup,
        UserEvent::BrowserFrameReady,
        UserEvent::DownloadUpdate,
        UserEvent::HintsReady,
    ];

    pub fn from_code(code: i32) -> UserEvent {
        Self::ALL[code as usize]
    }
}

#[derive(Clone)]
pub struct UserEventSender {
    event_type: u32,
}

impl UserEventSender {
    pub fn new() -> Self {
        Self {
            event_type: unsafe { sdl2::sys::SDL_RegisterEvents(1) },
        }
    }

    pub fn send(&self, event: UserEvent) {
        unsafe {
            let mut evt = SDL_Event {
                user: SDL_UserEvent {
                    type_: self.event_type,
                    timestamp: 0,
                    windowID: 0,
                    code: event as i32,
                    data1: std::ptr::null_mut(),
                    data2: std::ptr::null_mut(),
                },
            };
            SDL_PushEvent(&mut evt);
        }
    }
}

impl servo::EventLoopWaker for UserEventSender {
    fn wake(&self) {
        self.send(UserEvent::BrowserWakeup);
    }

    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
}
