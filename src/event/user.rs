use crate::app::AppCommand;
use sdl2::sys::{SDL_Event, SDL_PushEvent, SDL_UserEvent};

pub fn handle_user(code: i32) -> Option<AppCommand> {
    let event = UserEvent::from_code(code);

    match event {
        UserEvent::HasUpdate => None,
        UserEvent::BrowserFrameReady | UserEvent::UiRepaintRequested => Some(AppCommand::Draw),
    }
}

#[repr(i32)]
#[derive(Copy, Clone)]
pub enum UserEvent {
    HasUpdate = 0,
    BrowserFrameReady = 1,
    UiRepaintRequested = 2,
}

impl UserEvent {
    pub const ALL: [UserEvent; 3] = [UserEvent::HasUpdate, UserEvent::BrowserFrameReady, UserEvent::UiRepaintRequested];

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
        self.send(UserEvent::HasUpdate);
    }

    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
}
