//! Ad blocking. Servo's `load_web_resource` delegate hook fires for every
//! network request with its URL, destination, and referrer — enough to run
//! Brave's adblock-rust engine (EasyList syntax) over it; blocked loads are
//! intercepted with an empty 200 response in [`crate::browser`]. Toggled with
//! `[adblock] enabled` in the config.
//!
//! The engine is not thread-safe (the crate's faster single-thread build), so
//! it never leaves the main thread: a background thread downloads the filter
//! lists, builds its own engine, and hands back the *serialized* DAT (also
//! cached to `cache/adblock.dat` in the user data dir); the main thread deserializes
//! it lazily on the next request check. With a cache present, startup loads it
//! directly and only refreshes in the background once it's older than
//! `update_days`.

use crate::config::{self, AdblockConfig};
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use content_security_policy::Destination;
use servo::WebResourceRequest;
use std::cell::RefCell;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub struct Adblock {
    enabled: bool,
    /// The engine, present once loaded — from cache at startup, or lazily from
    /// `fresh` once the builder thread delivers.
    engine: RefCell<Option<Engine>>,
    /// Serialized engine handed over by the builder thread.
    fresh: Arc<Mutex<Option<Vec<u8>>>>,
}

impl Adblock {
    pub fn new(cfg: &AdblockConfig) -> Self {
        let fresh = Arc::new(Mutex::new(None));
        if !cfg.enabled {
            return Self {
                enabled: false,
                engine: RefCell::new(None),
                fresh,
            };
        }

        let cache = cache_path();
        let engine = load_cached(&cache);
        if (engine.is_none() || cache_is_stale(&cache, cfg.update_days)) && !cfg.lists.is_empty() {
            let lists = cfg.lists.clone();
            let fresh = fresh.clone();
            std::thread::spawn(move || build_engine(lists, cache, fresh));
        }
        Self {
            enabled: true,
            engine: RefCell::new(engine),
            fresh,
        }
    }

    /// Whether this request should be blocked. Lazily swaps in a freshly built
    /// engine when the builder thread has delivered one.
    pub fn should_block(&self, request: &WebResourceRequest) -> bool {
        if !self.enabled {
            return false;
        }
        self.take_fresh();
        let engine = self.engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return false;
        };
        // Never block the page itself, only subresources / frames.
        if request.is_for_main_frame {
            return false;
        }
        let url = request.url.as_str();
        let source = request
            .referrer_url
            .as_ref()
            .map(url::Url::as_str)
            .unwrap_or(url);
        let Ok(req) = Request::new(url, source, request_type(request.destination)) else {
            return false;
        };
        engine.check_network_request(&req).matched
    }

    /// Move a builder-thread DAT (if one arrived) into the live engine.
    fn take_fresh(&self) {
        // try_lock: never stall request handling on the builder thread.
        let Ok(mut fresh) = self.fresh.try_lock() else {
            return;
        };
        let Some(dat) = fresh.take() else {
            return;
        };
        drop(fresh);
        match deserialize(&dat) {
            Some(engine) => {
                log::info!("adblock: fresh engine active ({} bytes)", dat.len());
                *self.engine.borrow_mut() = Some(engine);
            }
            None => log::warn!("adblock: discarding undeserializable fresh engine"),
        }
    }
}

fn cache_path() -> String {
    format!("{}adblock.dat", config::cache_dir())
}

fn deserialize(dat: &[u8]) -> Option<Engine> {
    let mut engine = Engine::default();
    match engine.deserialize(dat) {
        Ok(()) => Some(engine),
        Err(e) => {
            log::warn!("adblock: could not deserialize engine: {e:?}");
            None
        }
    }
}

fn load_cached(path: &str) -> Option<Engine> {
    let dat = std::fs::read(path).ok()?;
    let engine = deserialize(&dat);
    if engine.is_some() {
        log::info!("adblock: loaded cached engine ({} bytes)", dat.len());
    }
    engine
}

/// Whether the cached engine is older than the refresh interval (`0` = never
/// refresh). Unreadable metadata counts as stale.
fn cache_is_stale(path: &str, update_days: u64) -> bool {
    if update_days == 0 {
        return false;
    }
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > Duration::from_secs(update_days * 86_400))
        .unwrap_or(false)
}

/// Builder thread: download the filter lists, build and serialize an engine,
/// cache the DAT, and hand it to the main thread via `out`. Lists that fail to
/// download are skipped; with none at all, the existing engine stays as is.
fn build_engine(lists: Vec<String>, cache: String, out: Arc<Mutex<Option<Vec<u8>>>>) {
    let mut filter_set = FilterSet::new(false);
    let mut fetched = 0usize;
    for url in &lists {
        match fetch_list(url) {
            Ok(text) => {
                filter_set.add_filters(text.lines(), ParseOptions::default());
                fetched += 1;
                log::info!("adblock: fetched list `{url}`");
            }
            Err(e) => log::warn!("adblock: could not fetch list `{url}`: {e}"),
        }
    }
    if fetched == 0 {
        return;
    }
    let engine = Engine::from_filter_set(filter_set, true);
    let dat = engine.serialize();
    if let Err(e) = std::fs::write(&cache, &dat) {
        log::warn!("adblock: could not write engine cache: {e}");
    }
    *out.lock().unwrap() = Some(dat);
}

fn fetch_list(url: &str) -> Result<String, String> {
    let mut text = String::new();
    ureq::get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

/// Map a fetch destination to adblock-rust's request-type string.
fn request_type(destination: Destination) -> &'static str {
    match destination {
        Destination::Document => "document",
        Destination::Frame | Destination::IFrame | Destination::Embed | Destination::Object => {
            "subdocument"
        }
        Destination::Script
        | Destination::Worker
        | Destination::SharedWorker
        | Destination::ServiceWorker
        | Destination::AudioWorklet
        | Destination::PaintWorklet
        | Destination::Xslt => "script",
        Destination::Style => "stylesheet",
        Destination::Image => "image",
        Destination::Font => "font",
        Destination::Audio | Destination::Video | Destination::Track => "media",
        // Fetch/XHR requests arrive with no specific destination.
        Destination::None | Destination::Json => "xhr",
        _ => "other",
    }
}
