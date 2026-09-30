//! A local folder served as `http://<folder>.localhost/` through Servo's
//! `load_web_resource` interception, with no socket: the folder gets an origin,
//! and so storage, of its own, stable across restarts and folder moves.
//!
//! Servo holds one lock over every interception in the process until the
//! response finishes, and buffers the body whole. A worker thread reads the file
//! and sends it in one piece; the load stays on the main thread, which alone may
//! answer it, and is finished on the next pass. Servo also ignores
//! `Content-Encoding` on an intercepted response, so a pre-compressed file is
//! decompressed here.

use crate::event::user::{UserEvent, UserEventSender};
use http::header::{HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use http::{HeaderMap, Method, StatusCode};
use percent_encoding::percent_decode_str;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use url::Url;

/// `*.localhost` is a secure context by spec.
const HOST_SUFFIX: &str = ".localhost";
/// DNS label limit.
const MAX_LABEL_LEN: usize = 63;
/// The label when a folder name keeps no ASCII letter or digit.
const FALLBACK_LABEL: &str = "game";
const INDEX_FILE: &str = "index.html";
const OCTET_STREAM: &str = "application/octet-stream";
const BROTLI_BUFFER: usize = 64 * 1024;
/// Text without a declared charset decodes as windows-1252 by spec.
const TEXT_CHARSET: &str = "; charset=utf-8";

/// A folder and the origin it is served under.
pub struct LocalSite {
    host: String,
    /// Canonical, so a symlink resolving outside it is caught by a prefix test.
    root: PathBuf,
    pub start_url: Url,
}

/// A [`LocalSite`] and the loads it is answering.
pub struct LocalServer {
    site: Arc<LocalSite>,
    next_id: Cell<u64>,
    /// Loads whose file is still being read, by id.
    waiting: RefCell<HashMap<u64, servo::WebResourceLoad>>,
    jobs: Sender<Job>,
    replies: Receiver<(u64, Reply)>,
}

/// One request, as the worker needs it.
struct Job {
    id: u64,
    method: Method,
    url_path: String,
    range: Option<String>,
}

impl LocalServer {
    /// Starts the worker; it ends with the server.
    pub fn new(site: LocalSite, waker: UserEventSender) -> Self {
        let site = Arc::new(site);
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (reply_tx, replies) = mpsc::channel();
        let worker_site = Arc::clone(&site);
        std::thread::spawn(move || {
            for job in job_rx {
                let reply = worker_site.reply(&job.method, &job.url_path, job.range.as_deref());
                if !reply.status.is_success() {
                    log::warn!("local: {} {}", reply.status.as_u16(), job.url_path);
                }
                if reply_tx.send((job.id, reply)).is_err() {
                    break;
                }
                waker.send(UserEvent::BrowserWakeup);
            }
        });
        Self {
            site,
            next_id: Cell::new(0),
            waiting: RefCell::default(),
            jobs,
            replies,
        }
    }

    pub fn owns(&self, url: &Url) -> bool {
        self.site.owns(url)
    }

    /// Queues the read; the reply goes out only through [`Self::finish_ready`],
    /// which the owner must call every pass.
    pub fn serve(&self, load: servo::WebResourceLoad) {
        let request = load.request();
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let job = Job {
            id,
            method: request.method.clone(),
            url_path: request.url.path().to_string(),
            range: request
                .headers
                .get(http::header::RANGE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        };
        if self.jobs.send(job).is_err() {
            // Dropping the load finishes it, which Servo reads as a network error.
            log::error!("local: worker gone; {} not served", request.url);
            return;
        }
        self.waiting.borrow_mut().insert(id, load);
    }

    pub fn finish_ready(&self) {
        while let Ok((id, reply)) = self.replies.try_recv() {
            let Some(load) = self.waiting.borrow_mut().remove(&id) else {
                continue;
            };
            let url = load.request().url.clone();
            let response = servo::WebResourceResponse::new(url)
                .status_code(reply.status)
                .headers(reply.headers);
            super::delegate::finish_intercepted(load, response, reply.body);
        }
    }
}

/// Pre-compression named by suffix, as Apache's `AddEncoding` maps it and Unity
/// builds expect. A `.gz` fetched by name arrives decoded too: the archive case
/// loses to Unity's, which asks for `x.wasm.br` outright.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Brotli,
    Gzip,
}

impl Encoding {
    const ALL: [Encoding; 2] = [Encoding::Brotli, Encoding::Gzip];

    fn suffix(self) -> &'static str {
        match self {
            Encoding::Brotli => ".br",
            Encoding::Gzip => ".gz",
        }
    }

    fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| name.ends_with(e.suffix()))
    }

    fn decode(self, file: File) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Encoding::Brotli => {
                brotli_decompressor::Decompressor::new(file, BROTLI_BUFFER).read_to_end(&mut out)?
            }
            Encoding::Gzip => flate2::read::GzDecoder::new(file).read_to_end(&mut out)?,
        };
        Ok(out)
    }
}

struct Found {
    path: PathBuf,
    encoding: Option<Encoding>,
    /// Read off the name less any encoding suffix.
    mime: String,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
enum ByteRange {
    Whole,
    /// Inclusive, as `Content-Range` states it.
    Part(u64, u64),
    Unsatisfiable,
}

impl LocalSite {
    /// The site for `path`: a folder opens its `index.html`, a file opens itself
    /// from its own folder.
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let (root, start) = if path.is_dir() {
            (path, INDEX_FILE.to_string())
        } else {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| format!("{}: not a UTF-8 file name", path.display()))?
                .to_string();
            let root = path
                .parent()
                .expect("a canonical file path has a parent")
                .to_path_buf();
            (root, name)
        };
        let folder = root.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let host = format!("{}{HOST_SUFFIX}", host_label(folder));
        let mut start_url = Url::parse(&format!("http://{host}/"))
            .map_err(|err| format!("{}: {err}", root.display()))?;
        start_url
            .path_segments_mut()
            .expect("an http URL has a path")
            .pop_if_empty()
            .push(&start);
        log::info!("local: serving {} as http://{host}/", root.display());
        Ok(Self {
            host,
            root,
            start_url,
        })
    }

    pub fn owns(&self, url: &Url) -> bool {
        url.scheme() == "http" && url.port().is_none() && url.host_str() == Some(&self.host)
    }

    fn reply(&self, method: &Method, url_path: &str, range: Option<&str>) -> Reply {
        if method != Method::GET && method != Method::HEAD {
            return Reply::status(StatusCode::METHOD_NOT_ALLOWED);
        }
        let Some(found) = self.resolve(url_path) else {
            return Reply::status(StatusCode::NOT_FOUND);
        };
        let reply = match found.encoding {
            Some(encoding) => File::open(&found.path)
                .and_then(|file| encoding.decode(file))
                .map(|body| Reply::slice(body, range)),
            None => read_range(&found.path, range),
        };
        let mut reply = match reply {
            Ok(reply) => reply,
            Err(err) => {
                log::warn!("local: reading {}: {err}", found.path.display());
                return Reply::status(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };
        reply.headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&found.mime).expect("mime_guess yields ASCII"),
        );
        if method == Method::HEAD {
            reply.body.clear();
        }
        reply
    }

    /// The file `url_path` names under the root, or `None` for anything missing
    /// or outside it. A missing name falls back to a pre-compressed sibling.
    fn resolve(&self, url_path: &str) -> Option<Found> {
        let decoded = percent_decode_str(url_path).decode_utf8().ok()?;
        let mut path = self.root.clone();
        for part in Path::new(decoded.as_ref()).components() {
            match part {
                Component::Normal(name) => path.push(name),
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir | Component::Prefix(_) => return None,
            }
        }
        if path.is_dir() {
            path.push(INDEX_FILE);
        }
        let path = self.inside(&path).or_else(|| {
            Encoding::ALL.into_iter().find_map(|e| {
                let mut sibling = path.clone().into_os_string();
                sibling.push(e.suffix());
                self.inside(Path::new(&sibling))
            })
        })?;
        let name = path.file_name()?.to_str()?;
        let encoding = Encoding::of(name);
        let inner = encoding.map_or(name, |e| &name[..name.len() - e.suffix().len()]);
        Some(Found {
            mime: mime_for(inner),
            encoding,
            path,
        })
    }

    /// `path` resolved, if it is a file under the root.
    fn inside(&self, path: &Path) -> Option<PathBuf> {
        let real = path.canonicalize().ok()?;
        (real.starts_with(&self.root) && real.is_file()).then_some(real)
    }
}

impl Reply {
    fn status(status: StatusCode) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from(0));
        Self {
            status,
            headers,
            body: Vec::new(),
        }
    }

    fn slice(body: Vec<u8>, range: Option<&str>) -> Self {
        let len = body.len() as u64;
        match byte_range(range, len) {
            ByteRange::Whole => Self::bytes(StatusCode::OK, body, None),
            ByteRange::Part(start, end) => {
                let part = body[start as usize..=end as usize].to_vec();
                Self::bytes(StatusCode::PARTIAL_CONTENT, part, Some((start, end, len)))
            }
            ByteRange::Unsatisfiable => Self::unsatisfiable(len),
        }
    }

    fn bytes(status: StatusCode, body: Vec<u8>, part: Option<(u64, u64, u64)>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
        if let Some((start, end, len)) = part {
            let value = format!("bytes {start}-{end}/{len}");
            headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&value).expect("digits, dashes and a slash are valid"),
            );
        }
        Self {
            status,
            headers,
            body,
        }
    }

    fn unsatisfiable(len: u64) -> Self {
        let mut reply = Self::status(StatusCode::RANGE_NOT_SATISFIABLE);
        let value = format!("bytes */{len}");
        reply.headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&value).expect("digits and a slash are valid"),
        );
        reply
    }
}

/// Reads only the requested bytes: a media seek asks for a tail.
fn read_range(path: &Path, range: Option<&str>) -> io::Result<Reply> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    Ok(match byte_range(range, len) {
        ByteRange::Whole => {
            let mut body = Vec::with_capacity(len as usize);
            file.read_to_end(&mut body)?;
            Reply::bytes(StatusCode::OK, body, None)
        }
        ByteRange::Part(start, end) => {
            file.seek(SeekFrom::Start(start))?;
            let mut body = vec![0; (end - start + 1) as usize];
            file.read_exact(&mut body)?;
            Reply::bytes(StatusCode::PARTIAL_CONTENT, body, Some((start, end, len)))
        }
        ByteRange::Unsatisfiable => Reply::unsatisfiable(len),
    })
}

/// A single `bytes=` range; anything else is answered whole, as the spec allows.
fn byte_range(header: Option<&str>, len: u64) -> ByteRange {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return ByteRange::Whole;
    };
    let Some((first, last)) = spec.split_once('-') else {
        return ByteRange::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    let (start, end) = match (first.parse::<u64>(), last.parse::<u64>()) {
        // `bytes=-N`: the last N bytes.
        (Err(_), Ok(n)) if first.is_empty() => {
            if n == 0 || len == 0 {
                return ByteRange::Unsatisfiable;
            }
            (len.saturating_sub(n), len - 1)
        }
        (Ok(start), Err(_)) if last.is_empty() => (start, len.saturating_sub(1)),
        (Ok(start), Ok(end)) if start <= end => (start, end.min(len.saturating_sub(1))),
        _ => return ByteRange::Whole,
    };
    if start >= len {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Part(start, end)
}

fn mime_for(name: &str) -> String {
    let mime = mime_guess::from_path(name)
        .first_raw()
        .unwrap_or(OCTET_STREAM);
    match mime.starts_with("text/") {
        true => format!("{mime}{TEXT_CHARSET}"),
        false => mime.to_string(),
    }
}

/// ASCII letters and digits, lowercased; any other run becomes one dash.
fn host_label(name: &str) -> String {
    let mut label = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            label.push(c.to_ascii_lowercase());
        } else if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    label.truncate(MAX_LABEL_LEN);
    let label = label.trim_end_matches('-');
    if label.is_empty() {
        FALLBACK_LABEL.to_string()
    } else {
        label.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const BODY: &[u8] = b"0123456789";

    /// A scratch site with `files` written under it, removed on drop.
    struct Scratch {
        dir: PathBuf,
        site: LocalSite,
    }

    impl Scratch {
        fn new(tag: &str, files: &[(&str, &[u8])]) -> Self {
            let dir =
                std::env::temp_dir().join(format!("retsurf-local-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let root = dir.join("My Game");
            for (name, bytes) in files {
                let path = root.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, bytes).unwrap();
            }
            std::fs::create_dir_all(&root).unwrap();
            let site = LocalSite::open(&root).unwrap();
            Self { dir, site }
        }

        fn get(&self, path: &str, range: Option<&str>) -> Reply {
            self.site.reply(&Method::GET, path, range)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn header(reply: &Reply, name: http::header::HeaderName) -> &str {
        reply.headers.get(name).unwrap().to_str().unwrap()
    }

    #[test]
    fn origin_comes_from_the_folder_name() {
        let s = Scratch::new("origin", &[(INDEX_FILE, b"x")]);
        assert_eq!(
            s.site.start_url.as_str(),
            "http://my-game.localhost/index.html"
        );
        assert!(s
            .site
            .owns(&Url::parse("http://my-game.localhost/a.js").unwrap()));
        assert!(!s
            .site
            .owns(&Url::parse("http://my-game.localhost:8080/").unwrap()));
        assert!(!s
            .site
            .owns(&Url::parse("https://my-game.localhost/").unwrap()));
    }

    #[test]
    fn a_file_opens_from_its_folder() {
        let s = Scratch::new("file", &[("play me.html", b"x")]);
        let site = LocalSite::open(&s.site.root.join("play me.html")).unwrap();
        assert_eq!(
            site.start_url.as_str(),
            "http://my-game.localhost/play%20me.html"
        );
        assert_eq!(site.reply(&Method::GET, "/play%20me.html", None).body, b"x");
    }

    #[test]
    fn labels_are_valid_dns() {
        assert_eq!(host_label("Super Game_2 (v1.0)"), "super-game-2-v1-0");
        assert_eq!(host_label("--x--"), "x");
        assert_eq!(host_label("Тетрис"), FALLBACK_LABEL);
        assert_eq!(host_label(&"a".repeat(80)).len(), MAX_LABEL_LEN);
    }

    #[test]
    fn serves_files_with_their_type() {
        let s = Scratch::new("serve", &[(INDEX_FILE, b"<p>"), ("Build/a.wasm", BODY)]);
        let index = s.get("/", None);
        assert_eq!(index.status, StatusCode::OK);
        assert_eq!(index.body, b"<p>");
        assert_eq!(header(&index, CONTENT_TYPE), "text/html; charset=utf-8");
        let wasm = s.get("/Build/a.wasm", None);
        assert_eq!(header(&wasm, CONTENT_TYPE), "application/wasm");
        assert_eq!(header(&wasm, CONTENT_LENGTH), "10");
        assert_eq!(s.get("/missing.js", None).status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn head_sends_no_body() {
        let s = Scratch::new("head", &[("a.txt", BODY)]);
        let reply = s.site.reply(&Method::HEAD, "/a.txt", None);
        assert_eq!(reply.status, StatusCode::OK);
        assert!(reply.body.is_empty());
        assert_eq!(header(&reply, CONTENT_LENGTH), "10");
        let post = s.site.reply(&Method::POST, "/a.txt", None);
        assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn nothing_outside_the_root() {
        let s = Scratch::new("escape", &[(INDEX_FILE, b"x")]);
        std::fs::write(s.dir.join("secret.txt"), b"s").unwrap();
        assert_eq!(s.get("/../secret.txt", None).status, StatusCode::NOT_FOUND);
        assert_eq!(
            s.get("/%2e%2e/secret.txt", None).status,
            StatusCode::NOT_FOUND
        );
        #[cfg(unix)]
        {
            let link = s.site.root.join("link.txt");
            std::os::unix::fs::symlink(s.dir.join("secret.txt"), link).unwrap();
            assert_eq!(s.get("/link.txt", None).status, StatusCode::NOT_FOUND);
        }
    }

    #[test]
    fn precompressed_files_arrive_decoded() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(BODY).unwrap();
        let gz = gz.finish().unwrap();
        let mut br = Vec::new();
        brotli::BrotliCompress(&mut &BODY[..], &mut br, &Default::default()).unwrap();
        let s = Scratch::new(
            "encoded",
            &[("a.wasm.br", &br), ("b.js.gz", &gz), ("c.data.gz", &gz)],
        );
        let wasm = s.get("/a.wasm.br", None);
        assert_eq!(wasm.body, BODY);
        assert_eq!(header(&wasm, CONTENT_TYPE), "application/wasm");
        assert_eq!(s.get("/b.js.gz", None).body, BODY);
        // Asked for by its plain name, found by its compressed sibling.
        let sibling = s.get("/c.data", None);
        assert_eq!(sibling.body, BODY);
        assert_eq!(header(&sibling, CONTENT_TYPE), OCTET_STREAM);
    }

    #[test]
    fn ranges() {
        let s = Scratch::new("range", &[("a.mp4", BODY)]);
        let part = s.get("/a.mp4", Some("bytes=2-4"));
        assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.body, b"234");
        assert_eq!(header(&part, CONTENT_RANGE), "bytes 2-4/10");
        assert_eq!(s.get("/a.mp4", Some("bytes=7-")).body, b"789");
        assert_eq!(s.get("/a.mp4", Some("bytes=-2")).body, b"89");
        assert_eq!(s.get("/a.mp4", Some("bytes=8-99")).body, b"89");
        let past = s.get("/a.mp4", Some("bytes=10-"));
        assert_eq!(past.status, StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(header(&past, CONTENT_RANGE), "bytes */10");
        assert_eq!(
            s.get("/a.mp4", Some("bytes=0-1,4-5")).status,
            StatusCode::OK
        );
    }
}
