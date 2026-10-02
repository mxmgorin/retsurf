//! Android glue that has no SDL API of its own: the log mirrored to a file, and
//! the system share sheet. The Java half lives in `RetsurfActivity`, reached by
//! SDL's user-message channel rather than JNI; command ids must match on both sides.

use log::{Log, Metadata, Record};

/// SDL's `COMMAND_USER`: message ids from here up are the app's.
const COMMAND_USER: u32 = 0x8000;
/// `RetsurfActivity.COMMAND_SHARE_LOGS`.
const COMMAND_SHARE_LOGS: u32 = COMMAND_USER + 1;

extern "C" {
    fn SDL_AndroidSendMessage(command: u32, param: i32) -> i32;
}

/// Open the share sheet with the log files; `false` when SDL could not post the request.
pub fn share_logs() -> bool {
    // SAFETY: SDL is initialized for the app's lifetime, and the call only posts a message.
    unsafe { SDL_AndroidSendMessage(COMMAND_SHARE_LOGS, 0) == 0 }
}

/// A logger writing each record to logcat and, when one could be opened, a file.
pub struct LogTee {
    pub logcat: android_logger::AndroidLogger,
    pub file: Option<env_logger::Logger>,
}

impl Log for LogTee {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.logcat.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        self.logcat.log(record);
        if let Some(file) = &self.file {
            file.log(record);
        }
    }

    fn flush(&self) {
        self.logcat.flush();
        if let Some(file) = &self.file {
            file.flush();
        }
    }
}
