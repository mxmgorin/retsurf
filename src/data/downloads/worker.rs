//! One background thread per file (ureq is blocking): stream to `<path>.part`,
//! rename into place. Requests present as the browser (User-Agent, Referer) and
//! the save name is picked from the response (Content-Disposition, `download`
//! attribute, redirect target). A watchdog fails stalled transfers (ureq has
//! no idle timeout).

use crate::browser::DownloadRequest;
use crate::event::user::{UserEvent, UserEventSender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Deadline for each pre-body phase of the request (DNS, connect, headers).
const PHASE_TIMEOUT: Duration = Duration::from_secs(30);

/// No received bytes for this long fails the transfer as stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the watchdog checks progress and the cancel flag.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// Cancel grace before the watchdog resolves the entry for a blocked worker.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

/// Worker → main-thread progress; `result` is write-once via `done`.
pub(super) struct Shared {
    pub received: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Destination path, set once the response headers picked the final name.
    pub path: Mutex<Option<String>>,
    /// Claimed (exactly once) by whoever stores `result`.
    done: AtomicBool,
    pub result: Mutex<Option<Result<(), String>>>,
}

/// Shared agent with per-phase deadlines; mid-body stalls are the watchdog's job.
fn agent() -> &'static ureq::Agent {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::Agent::config_builder()
            .timeout_resolve(Some(PHASE_TIMEOUT))
            .timeout_connect(Some(PHASE_TIMEOUT))
            .timeout_recv_response(Some(PHASE_TIMEOUT))
            .build()
            .new_agent()
    });
    &AGENT
}

/// Everything one worker needs to fetch its file.
struct Job {
    url: String,
    referer: Option<String>,
    suggested_name: Option<String>,
    user_agent: String,
    dir: String,
}

/// Spawn the worker and its watchdog; the destination path arrives through the
/// returned handle once known.
pub(super) fn spawn(
    request: &DownloadRequest,
    user_agent: &str,
    dir: &str,
    sender: &UserEventSender,
) -> Arc<Shared> {
    let shared = Arc::new(Shared {
        received: AtomicU64::new(0),
        total: AtomicU64::new(0),
        cancel: AtomicBool::new(false),
        path: Mutex::new(None),
        done: AtomicBool::new(false),
        result: Mutex::new(None),
    });
    {
        let job = Job {
            url: request.url.clone(),
            referer: request.referer.clone(),
            suggested_name: request.suggested_name.clone(),
            user_agent: user_agent.to_string(),
            dir: dir.to_string(),
        };
        let shared = shared.clone();
        let sender = sender.clone();
        std::thread::spawn(move || run(job, shared, sender));
    }
    {
        let shared = shared.clone();
        let sender = sender.clone();
        std::thread::spawn(move || watch(shared, sender));
    }
    shared
}

/// Worker-thread entry: fetch, then publish the result.
fn run(job: Job, shared: Arc<Shared>, sender: UserEventSender) {
    let result = fetch(&job, &shared, &sender);
    if let Err(e) = &result {
        log::warn!("download `{}` failed: {e}", job.url);
    }
    finish(&shared, result, &sender);
}

/// Publish `result` once (first of worker/watchdog wins) and wake the main loop.
fn finish(shared: &Shared, result: Result<(), String>, sender: &UserEventSender) {
    if shared.done.swap(true, Ordering::Relaxed) {
        return;
    }
    *shared.result.lock().unwrap() = Some(result);
    sender.send(UserEvent::DownloadUpdate);
}

/// Resolve what the worker can't: an unacknowledged cancel or a stalled socket.
fn watch(shared: Arc<Shared>, sender: UserEventSender) {
    let mut last_received = 0;
    let mut last_change = Instant::now();
    let mut cancelled_at: Option<Instant> = None;
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        if shared.done.load(Ordering::Relaxed) {
            return;
        }
        let received = shared.received.load(Ordering::Relaxed);
        if received != last_received {
            last_received = received;
            last_change = Instant::now();
        }
        if shared.cancel.load(Ordering::Relaxed) {
            if cancelled_at.get_or_insert_with(Instant::now).elapsed() >= CANCEL_GRACE {
                finish(&shared, Err("cancelled".to_string()), &sender);
                return;
            }
        } else if last_change.elapsed() >= STALL_TIMEOUT {
            shared.cancel.store(true, Ordering::Relaxed);
            finish(
                &shared,
                Err(format!("stalled: no data for {}s", STALL_TIMEOUT.as_secs())),
                &sender,
            );
            return;
        }
    }
}

/// Fetch the URL, pick the save name from the response, and stream to
/// `<path>.part`; renamed into place on success, removed on failure.
fn fetch(job: &Job, shared: &Shared, sender: &UserEventSender) -> Result<(), String> {
    use ureq::ResponseExt;

    let mut request = agent().get(&job.url).header("User-Agent", &job.user_agent);
    if let Some(referer) = &job.referer {
        request = request.header("Referer", referer);
    }
    let response = request.call().map_err(|e| e.to_string())?;
    if let Some(total) = crate::net::content_length(response.headers()) {
        shared.total.store(total, Ordering::Relaxed);
    }

    let name = pick_filename(
        response.headers(),
        response.get_uri().path(),
        job.suggested_name.as_deref(),
        &job.url,
    );
    let (path, part, file) = create_unique(&job.dir, &name)?;
    log::info!("downloading `{}` -> `{path}`", job.url);
    *shared.path.lock().unwrap() = Some(path.clone());
    sender.send(UserEvent::DownloadUpdate);

    let reader = response.into_body().into_reader();
    let result = crate::net::stream(reader, file, |_, received, due| {
        shared.received.store(received, Ordering::Relaxed);
        if due {
            sender.send(UserEvent::DownloadUpdate);
        }
        !shared.cancel.load(Ordering::Relaxed)
    })
    .and_then(|()| std::fs::rename(&part, &path).map_err(|e| format!("rename: {e}")));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Save-name precedence (per the HTML spec): Content-Disposition, the page's
/// `download` attribute, a file-naming redirect target, the link URL.
fn pick_filename(
    headers: &ureq::http::HeaderMap,
    final_path: &str,
    suggested: Option<&str>,
    url: &str,
) -> String {
    let disposition = headers
        .get("Content-Disposition")
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    if let Some(name) = disposition.as_deref().and_then(filename_from_disposition) {
        return name;
    }
    if let Some(name) = suggested.and_then(sanitize) {
        return name;
    }
    if let Some(name) = filename_from_path(final_path) {
        if has_extension(&name) {
            return name;
        }
    }
    filename_from_url(url)
}

/// Filename from a Content-Disposition value (RFC 6266): `filename*` (RFC 5987)
/// wins over plain `filename=`, quoted or bare.
fn filename_from_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for param in value.split(';') {
        let Some((key, val)) = param.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            // charset'language'percent-encoded; everything past the last quote.
            "filename*" => {
                let encoded = val.rsplit_once('\'').map_or(val, |(_, e)| e);
                extended = Some(
                    percent_encoding::percent_decode_str(encoded)
                        .decode_utf8_lossy()
                        .into_owned(),
                );
            }
            "filename" => plain = Some(val.trim_matches('"').to_string()),
            _ => {}
        }
    }
    extended.or(plain).and_then(|name| sanitize(&name))
}

/// Save name from the URL's last path segment, falling back to `download`.
pub(super) fn filename_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| filename_from_path(u.path()))
        .unwrap_or_else(|| "download".to_string())
}

/// Last non-empty segment of a URL path, percent-decoded and sanitized.
fn filename_from_path(path: &str) -> Option<String> {
    let segment = path.rsplit('/').find(|s| !s.is_empty())?;
    let name = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
    sanitize(&name)
}

/// Make an untrusted name safe as a bare file name: no separators, control
/// characters, or leading dots. `None` when nothing usable remains.
fn sanitize(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.').trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn has_extension(name: &str) -> bool {
    matches!(name.rsplit_once('.'), Some((stem, ext)) if !stem.is_empty() && !ext.is_empty())
}

/// Reserve a free destination (`name`, `stem-1.ext`, …): creating the `.part`
/// exclusively is the reservation, so parallel workers can't collide.
fn create_unique(dir: &str, filename: &str) -> Result<(String, String, std::fs::File), String> {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (filename.to_string(), String::new()),
    };
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            filename.to_string()
        } else {
            format!("{stem}-{n}{ext}")
        };
        let path = format!("{dir}{name}");
        let part = format!("{path}.part");
        n += 1;
        if std::path::Path::new(&path).exists() {
            continue;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
        {
            Ok(file) => return Ok((path, part, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("create: {e}")),
        }
    }
}

/// `dir/filename`, suffixed `-1`, `-2`, … until neither the file nor its `.part`
/// exists. For whole-file writes on the main thread; workers use [`create_unique`].
pub(super) fn unique_path(dir: &str, filename: &str) -> String {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (filename.to_string(), String::new()),
    };
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            filename.to_string()
        } else {
            format!("{stem}-{n}{ext}")
        };
        let path = format!("{dir}{name}");
        let part = format!("{path}.part");
        if !std::path::Path::new(&path).exists() && !std::path::Path::new(&part).exists() {
            return path;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(disposition: Option<&'static str>) -> ureq::http::HeaderMap {
        let mut h = ureq::http::HeaderMap::new();
        if let Some(v) = disposition {
            h.insert(
                "Content-Disposition",
                ureq::http::HeaderValue::from_static(v),
            );
        }
        h
    }

    /// A quoted filename parameter names the file.
    #[test]
    fn disposition_quoted_filename() {
        let name = filename_from_disposition(r#"attachment; filename="release notes.pdf""#);
        assert_eq!(name.as_deref(), Some("release notes.pdf"));
    }

    /// A bare (unquoted) filename parameter also works.
    #[test]
    fn disposition_bare_filename() {
        let name = filename_from_disposition("attachment;filename=game.zip");
        assert_eq!(name.as_deref(), Some("game.zip"));
    }

    /// The RFC 5987 form is percent-decoded and preferred over plain filename.
    #[test]
    fn disposition_extended_filename_wins() {
        let name = filename_from_disposition(
            "attachment; filename=fallback.bin; filename*=UTF-8''na%C3%AFve%20rom.gba",
        );
        assert_eq!(name.as_deref(), Some("naïve rom.gba"));
    }

    /// A disposition without any filename yields nothing.
    #[test]
    fn disposition_without_filename() {
        assert_eq!(filename_from_disposition("inline"), None);
        assert_eq!(filename_from_disposition("attachment; size=42"), None);
    }

    /// Server names are untrusted: no traversal, no hidden files.
    #[test]
    fn disposition_name_is_sanitized() {
        let name = filename_from_disposition(r#"attachment; filename="../../.hidden""#);
        assert_eq!(name.as_deref(), Some("hidden"));
        assert_eq!(
            filename_from_disposition(r#"attachment; filename="...""#),
            None
        );
    }

    /// Content-Disposition beats the download attribute and both URLs.
    #[test]
    fn pick_prefers_disposition() {
        let h = headers(Some(r#"attachment; filename="real.zip""#));
        let name = pick_filename(
            &h,
            "/mirror/obj123",
            Some("asked.zip"),
            "https://x.test/a.zip",
        );
        assert_eq!(name, "real.zip");
    }

    /// Without a disposition, the page's download attribute names the file.
    #[test]
    fn pick_uses_the_suggested_name() {
        let h = headers(None);
        let name = pick_filename(
            &h,
            "/files/real.chd",
            Some("asked.chd"),
            "https://x.test/gen",
        );
        assert_eq!(name, "asked.chd");
    }

    /// Without either, a redirect target that names a file wins over the link.
    #[test]
    fn pick_uses_final_path_when_it_names_a_file() {
        let h = headers(None);
        let name = pick_filename(&h, "/files/game-1.2.chd", None, "https://x.test/latest.chd");
        assert_eq!(name, "game-1.2.chd");
    }

    /// An extension-less redirect target (a CDN token path) loses to the link URL.
    #[test]
    fn pick_falls_back_to_the_link_url() {
        let h = headers(None);
        let name = pick_filename(
            &h,
            "/obj/ab12f3",
            None,
            "https://x.test/roms/game.sfc?sig=1",
        );
        assert_eq!(name, "game.sfc");
    }

    /// URL names are percent-decoded and never empty.
    #[test]
    fn filename_from_url_decodes_and_falls_back() {
        assert_eq!(
            filename_from_url("https://x.test/a%20b.zip"),
            "a b.zip".to_string()
        );
        assert_eq!(filename_from_url("https://x.test/"), "download".to_string());
        assert_eq!(filename_from_url("not a url"), "download".to_string());
    }

    /// Path separators and control characters never reach the file system.
    #[test]
    fn sanitize_strips_separators_and_controls() {
        assert_eq!(sanitize("a/b\\c"), Some("abc".to_string()));
        assert_eq!(sanitize("re\nport.pdf"), Some("report.pdf".to_string()));
        assert_eq!(sanitize("  .. "), None);
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// Fresh download dir (with the trailing separator the workers expect).
    fn temp_dir(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("retsurf-worker-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        format!("{}/", dir.display())
    }

    /// Read one request's header block, returning its lines.
    fn read_request(stream: &mut std::net::TcpStream) -> Vec<String> {
        let mut lines = vec![];
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("request line");
            let line = line.trim_end().to_string();
            if line.is_empty() {
                return lines;
            }
            lines.push(line);
        }
    }

    fn wait_result(shared: &Shared) -> Result<(), String> {
        for _ in 0..1000 {
            if let Some(result) = shared.result.lock().unwrap().take() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("worker did not finish within 10s");
    }

    /// End to end: browser headers are sent, the redirect is followed, and the
    /// server's Content-Disposition names the saved file.
    #[test]
    fn fetch_follows_redirect_and_honors_disposition() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().expect("accept");
            let request = read_request(&mut first);
            // Connection: close, or ureq reuses this socket for the redirect.
            first
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /real/path\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("redirect");
            let (mut second, _) = listener.accept().expect("accept redirect");
            read_request(&mut second);
            second
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\
                      Content-Disposition: attachment; filename=\"server.bin\"\r\n\r\nhello",
                )
                .expect("response");
            request
        });

        let dir = temp_dir("ok");
        let sender = UserEventSender::new();
        let request = DownloadRequest {
            url: format!("http://127.0.0.1:{port}/linked.zip"),
            referer: Some("https://example.test/page".to_string()),
            suggested_name: None,
        };
        let shared = spawn(&request, "retsurf-test-ua", &dir, &sender);

        assert_eq!(wait_result(&shared), Ok(()));
        let path = shared.path.lock().unwrap().take().expect("published path");
        assert_eq!(path, format!("{dir}server.bin"));
        assert_eq!(std::fs::read(&path).expect("saved file"), b"hello");
        assert!(!std::path::Path::new(&format!("{path}.part")).exists());
        let request = server.join().expect("server");
        let has = |h: &str| request.iter().any(|l| l.eq_ignore_ascii_case(h));
        assert!(has("user-agent: retsurf-test-ua"));
        assert!(has("referer: https://example.test/page"));
        std::fs::remove_dir_all(dir.trim_end_matches('/')).expect("cleanup");
    }

    /// Cancelling mid-body fails the entry and removes the partial file.
    #[test]
    fn cancel_mid_body_removes_the_partial() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (hold_tx, hold_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            read_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nfirst")
                .expect("partial body");
            stream.flush().expect("flush");
            // Hold the connection until the test has set cancel, then send the
            // rest so the worker's read returns and it sees the flag.
            hold_rx.recv().expect("cancel signal");
            let _ = stream.write_all(&[b'x'; 95]);
        });

        let dir = temp_dir("cancel");
        let sender = UserEventSender::new();
        let request = DownloadRequest {
            url: format!("http://127.0.0.1:{port}/big.iso"),
            referer: None,
            suggested_name: None,
        };
        let shared = spawn(&request, "retsurf-test-ua", &dir, &sender);

        for _ in 0..1000 {
            if shared.received.load(Ordering::Relaxed) > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        shared.cancel.store(true, Ordering::Relaxed);
        hold_tx.send(()).expect("unblock server");

        assert_eq!(wait_result(&shared), Err("cancelled".to_string()));
        let path = shared.path.lock().unwrap().take().expect("published path");
        assert!(!std::path::Path::new(&path).exists());
        assert!(!std::path::Path::new(&format!("{path}.part")).exists());
        std::fs::remove_dir_all(dir.trim_end_matches('/')).expect("cleanup");
    }
}
