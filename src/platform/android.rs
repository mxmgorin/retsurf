//! Android glue that has no SDL API of its own: the log mirrored to a file, the
//! system share sheet, and finished downloads handed to the shared Downloads. The
//! Java half lives in `RetsurfActivity`. The share sheet is reached by SDL's
//! user-message channel, whose command ids must match on both sides; the downloads
//! by a JNI call on the calling thread, which returns where the file went.

use jni::objects::{JObject, JString};
use jni::{jni_sig, jni_str, EnvUnowned, JValue, Outcome};
use log::{Log, Metadata, Record};
use std::ffi::c_void;

/// SDL's `COMMAND_USER`: message ids from here up are the app's.
const COMMAND_USER: u32 = 0x8000;
/// `RetsurfActivity.COMMAND_SHARE_LOGS`.
const COMMAND_SHARE_LOGS: u32 = COMMAND_USER + 1;

/// Local references one call into the activity holds: the activity, its class,
/// the argument and the result.
const ACTIVITY_CALL_LOCALS: usize = 4;

extern "C" {
    fn SDL_AndroidSendMessage(command: u32, param: i32) -> i32;
    fn SDL_AndroidGetJNIEnv() -> *mut c_void;
    fn SDL_AndroidGetActivity() -> *mut c_void;
}

/// Open the share sheet with the log files; `false` when SDL could not post the request.
pub fn share_logs() -> bool {
    // SAFETY: SDL is initialized for the app's lifetime, and the call only posts a message.
    unsafe { SDL_AndroidSendMessage(COMMAND_SHARE_LOGS, 0) == 0 }
}

/// Move the finished download at `path` into the shared Downloads, where the
/// system's file apps list it; returns its path there. Blocks for the copy.
pub fn publish_download(path: &str) -> Result<String, String> {
    // SAFETY: SDL attaches the calling thread to the VM when it is not yet, and
    // keeps the attachment until the thread exits.
    let raw = unsafe { SDL_AndroidGetJNIEnv() };
    if raw.is_null() {
        return Err("no JNI environment".to_string());
    }
    // SAFETY: non-null and attached to this thread for longer than this call.
    let mut env = unsafe { EnvUnowned::from_raw(raw.cast()) };
    let outcome = env.with_env(|env| -> jni::errors::Result<Option<String>> {
        let moved = env.with_local_frame(ACTIVITY_CALL_LOCALS, |env| {
            // SAFETY: SDL returns a fresh local reference in this thread's frame.
            let activity = unsafe { JObject::from_raw(env, SDL_AndroidGetActivity().cast()) };
            let arg = env.new_string(path)?;
            let moved = env
                .call_method(
                    &activity,
                    jni_str!("publishDownload"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(&arg)],
                )?
                .l()?;
            if moved.is_null() {
                return Ok(None);
            }
            let moved = env.cast_local::<JString>(moved)?;
            moved.try_to_string(env).map(Some)
        });
        // A pending exception would abort the next JNI call this thread makes.
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
        }
        moved
    });
    match outcome.into_outcome() {
        Outcome::Ok(Some(moved)) => Ok(moved),
        Outcome::Ok(None) => Err("refused, see the activity's log".to_string()),
        Outcome::Err(e) => Err(e.to_string()),
        Outcome::Panic(_) => Err("panicked".to_string()),
    }
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
