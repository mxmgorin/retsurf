//! [`H264Decoder`] against a fake libpmhwdec: hardware first, software when it
//! is missing or refuses, the mid-stream hand-off, seeks and full buffers.
//! The fake gives out flat grey pictures, so a frame's source shows in its
//! pixels; openh264 decodes the real clip (64x48, 14 frames, a key frame every
//! 6, B-frames).

use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void};
use std::sync::Mutex;

use super::*;
use crate::media::player::hwdec::{Caps, Picture, AGAIN, OK, UNSUPPORTED};

const CLIP: &[u8] = include_bytes!("testdata/gop6.h264");
const W: usize = 64;
const H: usize = 48;
const GREY: u8 = 128;

#[derive(Default)]
struct Fake {
    no_hardware: bool,
    refuse_headers: bool,
    /// Refuse the stream at this access unit (0 = the first after the headers).
    fail_at: Option<usize>,
    /// `AGAIN` while this many pictures are out. A picture is still decoding
    /// until a `get` after its `send` finds it busy and finishes it.
    buffers: Option<usize>,
    busy: VecDeque<i64>,
    /// Give pictures out a unit late, every pair swapped (I P B B as I B P B):
    /// display order, each with the time it was sent with, as real decoders do.
    reorder: bool,
    held: Option<i64>,
    units: usize,
    queue: VecDeque<i64>,
    opens: usize,
    closes: usize,
    flushes: usize,
    gets: usize,
    puts: usize,
    agains: usize,
    /// The first bytes of every unit sent.
    sent: Vec<Vec<u8>>,
}

static FAKE: Mutex<Option<Fake>> = Mutex::new(None);
static SERIAL: Mutex<()> = Mutex::new(());
static PLANE: [u8; W * H] = [GREY; W * H];
static HANDLE: u8 = 0;

fn with<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    // A failed assertion inside `f` poisons the lock; the fake is called from
    // `extern "C"` code, where a second panic would abort every test.
    let mut fake = FAKE.lock().unwrap_or_else(|e| e.into_inner());
    f(fake.get_or_insert_with(Fake::default))
}

fn is_headers(data: &[u8]) -> bool {
    data.len() > 4 && data[4] & 0x1F == 7 && !nal_types(data).iter().any(|&t| t == 1 || t == 5)
}

unsafe extern "C" fn query(_codec: c_int, caps: *mut Caps) -> c_int {
    if with(|f| f.no_hardware) {
        return -1;
    }
    unsafe {
        (*caps).max_width = 1920;
        (*caps).max_height = 1088;
        (*caps).max_bit_depth = 8;
        (*caps).backend = c"fake".as_ptr();
    }
    0
}

unsafe extern "C" fn open(_codec: c_int, _w: c_int, _h: c_int) -> *mut c_void {
    with(|f| f.opens += 1);
    &HANDLE as *const u8 as *mut c_void
}

unsafe extern "C" fn send(_d: *mut c_void, data: *const u8, size: c_int, pts: i64) -> c_int {
    if data.is_null() || size == 0 {
        with(|f| {
            let rest: Vec<i64> = f.busy.drain(..).chain(f.held.take()).collect();
            f.queue.extend(rest);
        });
        return OK;
    }
    let data = unsafe { std::slice::from_raw_parts(data, size as usize) };
    with(|f| {
        if is_headers(data) {
            return if f.refuse_headers { UNSUPPORTED } else { OK };
        }
        if f.buffers.is_some_and(|n| f.queue.len() + f.busy.len() >= n) {
            f.agains += 1;
            return AGAIN;
        }
        if f.fail_at == Some(f.units) {
            return UNSUPPORTED;
        }
        f.units += 1;
        f.sent.push(data[..data.len().min(5)].to_vec());
        if f.buffers.is_some() {
            f.busy.push_back(pts);
        } else if !f.reorder {
            f.queue.push_back(pts);
        } else if let Some(held) = f.held.take() {
            f.queue.push_back(pts);
            f.queue.push_back(held);
        } else {
            f.held = Some(pts);
        }
        OK
    })
}

unsafe extern "C" fn get(_d: *mut c_void, pic: *mut Picture) -> c_int {
    let next = with(|f| match f.queue.pop_front() {
        None => {
            let done = f.busy.pop_front();
            f.queue.extend(done);
            None
        }
        ready => ready,
    });
    let Some(pts) = next else {
        return 0;
    };
    with(|f| f.gets += 1);
    unsafe {
        let p = &mut *pic;
        p.width = W as c_int;
        p.height = H as c_int;
        p.format = 1;
        p.plane = [PLANE.as_ptr(), PLANE.as_ptr()];
        p.stride = [W as c_int, W as c_int];
        p.pts = pts;
    }
    1
}

unsafe extern "C" fn put(_d: *mut c_void, _pic: *mut Picture) {
    with(|f| f.puts += 1);
}

unsafe extern "C" fn flush(_d: *mut c_void) {
    with(|f| {
        f.flushes += 1;
        f.queue.clear();
        f.busy.clear();
        f.held = None;
    });
}

unsafe extern "C" fn close(_d: *mut c_void) {
    with(|f| f.closes += 1);
}

unsafe extern "C" fn backend_name(_d: *const c_void) -> *const c_char {
    c"fake".as_ptr()
}

static LIB: Lib = Lib {
    query,
    open,
    send,
    get,
    put,
    flush,
    close,
    backend_name,
};

/// A fresh fake, and the lock that keeps the tests sharing it apart.
fn fake(setup: impl FnOnce(&mut Fake)) -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = Fake::default();
    setup(&mut state);
    *FAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
    guard
}

/// NAL unit types of an Annex-B buffer.
fn nal_types(data: &[u8]) -> Vec<u8> {
    split_nals(data).iter().map(|nal| nal[0] & 0x1F).collect()
}

fn split_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let mut end = starts.get(n + 1).map_or(data.len(), |&next| next - 3);
        while end > start && data[end - 1] == 0 {
            end -= 1;
        }
        out.push(&data[start..end]);
    }
    out
}

/// The clip as (SPS+PPS, access units with their key frame flag), Annex-B
/// with 4-byte start codes like [`super::super::video`] makes them.
fn clip() -> (Vec<u8>, Vec<(Vec<u8>, bool)>) {
    let mut headers = Vec::new();
    let mut units = Vec::new();
    let mut pending = Vec::new();
    for nal in split_nals(CLIP) {
        let kind = nal[0] & 0x1F;
        let mut coded = vec![0, 0, 0, 1];
        coded.extend_from_slice(nal);
        match kind {
            // x264 repeats them at every key frame: those travel in-band.
            7 | 8 if units.is_empty() => headers.extend_from_slice(&coded),
            1 | 5 => {
                pending.extend_from_slice(&coded);
                units.push((std::mem::take(&mut pending), kind == 5));
            }
            _ => pending.extend_from_slice(&coded),
        }
    }
    (headers, units)
}

fn pts(unit: usize) -> i64 {
    unit as i64 * 33_333
}

struct Run {
    frames: Vec<Frame>,
}

impl Run {
    fn pts(&self) -> Vec<i64> {
        let mut out: Vec<i64> = self.frames.iter().map(|f| f.pts_us).collect();
        out.sort_unstable();
        out
    }

    fn grey(&self, pts: i64) -> bool {
        let frame = self.frames.iter().find(|f| f.pts_us == pts).expect("frame");
        frame.y.iter().all(|&b| b == GREY)
    }
}

fn decode_all(decoder: &mut H264Decoder, units: &[(Vec<u8>, bool)], first: usize) -> Run {
    let mut frames = Vec::new();
    for (n, (unit, key)) in units.iter().enumerate() {
        decoder
            .decode(unit, pts(first + n), *key, &mut |f| frames.push(f))
            .unwrap();
    }
    decoder.finish(&mut |f| frames.push(f));
    Run { frames }
}

fn all_pts(count: usize) -> Vec<i64> {
    (0..count).map(pts).collect()
}

#[test]
fn clip_is_what_the_tests_expect() {
    let (headers, units) = clip();
    assert_eq!(nal_types(&headers), [7, 8]);
    assert_eq!(units.len(), 14);
    let keys: Vec<usize> = (0..units.len()).filter(|&n| units[n].1).collect();
    assert_eq!(keys, [0, 6, 12]);
}

#[test]
fn hardware_decodes_when_it_takes_the_stream() {
    let _serial = fake(|_| {});
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    assert!(run.frames.iter().all(|f| f.width == W && f.height == H));
    assert!((0..units.len()).all(|n| run.grey(pts(n))));
    assert_eq!(decoder.backend().as_deref(), Some("fake"));
    drop(decoder);
    with(|f| {
        assert_eq!(f.opens, 1);
        assert_eq!(f.closes, 1);
        assert_eq!(f.gets, f.puts);
    });
}

/// The MP4 demuxer hands out decode-order times; pictures come back in display
/// order with the times their units were sent with. Each must be stamped with
/// its display time anyway, or the presenter shows B-frames out of order.
#[test]
fn reordered_pictures_are_stamped_in_display_order() {
    let _serial = fake(|f| f.reorder = true);
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    let given: Vec<i64> = run.frames.iter().map(|f| f.pts_us).collect();
    assert_eq!(given, all_pts(units.len()));
}

#[test]
fn no_hardware_means_openh264() {
    let _serial = fake(|f| f.no_hardware = true);
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    assert!((0..units.len()).all(|n| !run.grey(pts(n))));
    with(|f| assert_eq!(f.opens, 0));
}

#[test]
fn no_library_means_openh264() {
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, None);
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
}

#[test]
fn too_big_for_the_hardware_means_openh264() {
    let _serial = fake(|_| {});
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, 3840, 2160, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.frames.len(), units.len());
    with(|f| assert_eq!(f.opens, 0));
}

#[test]
fn refused_parameter_sets_go_to_openh264() {
    let _serial = fake(|f| f.refuse_headers = true);
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    assert!((0..units.len()).all(|n| !run.grey(pts(n))));
    with(|f| {
        assert_eq!(f.opens, 1);
        assert_eq!(f.closes, 1);
    });
}

/// Refused at unit 8, two units into the second GOP: openh264 decodes that
/// GOP again from its key frame, and no picture is lost or shown twice.
#[test]
fn hand_off_mid_stream_keeps_every_picture_once() {
    let _serial = fake(|f| f.fail_at = Some(8));
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    assert!((0..8).all(|n| run.grey(pts(n))), "the hardware's pictures");
    assert!((8..units.len()).all(|n| !run.grey(pts(n))), "openh264's");
    assert_eq!(decoder.backend().as_deref(), None, "openh264 ended with the stream");
    with(|f| {
        assert_eq!(f.closes, 1);
        assert_eq!(f.gets, f.puts);
    });
}

/// The hand-off at a key frame: nothing to decode again.
#[test]
fn hand_off_at_a_key_frame() {
    let _serial = fake(|f| f.fail_at = Some(6));
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    assert!((0..6).all(|n| run.grey(pts(n))));
    assert!((6..units.len()).all(|n| !run.grey(pts(n))));
}

#[test]
fn full_buffers_are_waited_out() {
    let _serial = fake(|f| f.buffers = Some(1));
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let run = decode_all(&mut decoder, &units, 0);
    assert_eq!(run.pts(), all_pts(units.len()));
    with(|f| assert!(f.agains > 0));
}

/// A seek keeps the hardware decoder: flushed, units skipped up to a key
/// frame, the parameter sets sent again in front of it.
#[test]
fn seek_flushes_and_sends_the_parameter_sets_again() {
    let _serial = fake(|_| {});
    let (headers, units) = clip();
    let mut decoder = H264Decoder::new(headers, W as u32, H as u32, Some(&LIB));
    let mut frames = Vec::new();
    for (n, (unit, key)) in units[..4].iter().enumerate() {
        decoder.decode(unit, pts(n), *key, &mut |f| frames.push(f)).unwrap();
    }
    decoder.reset();
    let sent_before = with(|f| f.sent.len());
    decoder.decode(&units[7].0, pts(7), false, &mut |f| frames.push(f)).unwrap();
    assert_eq!(with(|f| f.sent.len()), sent_before, "no key frame yet");
    decoder.decode(&units[6].0, pts(6), true, &mut |f| frames.push(f)).unwrap();
    with(|f| {
        assert_eq!(f.flushes, 1);
        assert_eq!(f.opens, 1);
        let last = f.sent.last().unwrap();
        assert_eq!(last[4] & 0x1F, 7, "SPS in front of the key frame");
    });
}

#[test]
fn a_library_without_the_api_is_not_used() {
    let library = unsafe { libloading::Library::new("libc.so.6") }.unwrap();
    assert!(crate::media::player::hwdec::load_from(library).is_none());
}
