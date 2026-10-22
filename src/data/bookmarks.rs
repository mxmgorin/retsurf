//! Bookmarks: one `bookmarks/<id>.toml` per bookmark in the user data dir,
//! kept in one manual order.
//!
//! The id is the file stem, slugged from the name at creation and never changed
//! after, so a rename edits `name` and anything keyed by the id survives it. A
//! file that does not parse costs only its own bookmark. Every change writes the
//! one or two files it touches, through a temp file and a rename.
//!
//! A missing directory is a first run, which seeds the shipped sites. An empty
//! directory is the user's own choice and is left empty.

use crate::clock;
use crate::config::token_enum::token_enum;
use crate::config::{overlay, HomeSort};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use toml::value::{Date, Datetime, Offset, Time};

/// Where the per-bookmark files live, under the user data dir.
const DIR: &str = "bookmarks";
/// The stem of a bookmark whose name and host keep no letter or digit.
const FALLBACK_ID: &str = "bookmark";

/// Seeded on a first run, so the start page is not empty.
const DEFAULTS: &[(&str, &str)] = &[
    ("Wikipedia", "https://en.wikipedia.org"),
    ("Retro Handhelds", "https://retrohandhelds.gg"),
    // The old front end: server-rendered, where the new one grows its feed
    // through fragment-parsed shadow roots that Servo 0.4 drops.
    ("r/SBCGaming", "https://old.reddit.com/r/SBCGaming/"),
    ("retsurf", "https://retsurf.app/"),
    ("Servo", "https://servo.org"),
];

/// Separators a page title puts between its own name and the site's.
const TITLE_SEPARATORS: &[&str] = &[" | ", " - ", " — ", " – ", " · "];

token_enum! {
    /// What opening a bookmark does.
    pub enum Launch {
        default Browser;
        /// Navigate the tab, chrome and all.
        Browser => "browser", "Site",
        /// Open it without the chrome, like a program.
        App => "app", "App",
        /// Open it without the chrome, in Game Mode.
        Game => "game", "Game",
    }
}

/// Which bookmarks a view shows: all, those that open one way, or one tag's.
#[derive(Clone, Eq, Debug)]
pub enum Filter {
    All,
    Launch(Launch),
    Tag(String),
}

/// A tag reads the same in either case.
impl PartialEq for Filter {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Filter::All, Filter::All) => true,
            (Filter::Launch(a), Filter::Launch(b)) => a == b,
            (Filter::Tag(a), Filter::Tag(b)) => same_tag(a, b),
            (Filter::All | Filter::Launch(_) | Filter::Tag(_), _) => false,
        }
    }
}

/// `(label, token)` of each filter that needs no tag: All, then one per
/// [`FILTER_LAUNCHES`] entry in its order. A tag spelled like a token reads as
/// the filter.
pub const FILTER_CHOICES: &[(&str, &str)] = &[
    ("All", "all"),
    ("Sites", "sites"),
    ("Apps", "apps"),
    ("Games", "games"),
];
/// What each filter after All admits.
const FILTER_LAUNCHES: [Launch; 3] = [Launch::Browser, Launch::App, Launch::Game];
const _: () = assert!(FILTER_CHOICES.len() == FILTER_LAUNCHES.len() + 1);

impl Filter {
    /// The filter a config token names; an empty one is [`Filter::All`].
    pub fn parse(token: &str) -> Self {
        let token = token.trim();
        if token.is_empty() {
            return Filter::All;
        }
        match FILTER_CHOICES
            .iter()
            .position(|(_, t)| token.eq_ignore_ascii_case(t))
        {
            Some(0) => Filter::All,
            Some(i) => Filter::Launch(FILTER_LAUNCHES[i - 1]),
            None => Filter::Tag(token.to_string()),
        }
    }

    /// The config token [`Self::parse`] reads back.
    pub fn token(&self) -> String {
        match self.choice() {
            Some((_, token)) => token.to_string(),
            None => self.label(),
        }
    }

    /// Its display name.
    pub fn label(&self) -> String {
        match (self, self.choice()) {
            (_, Some((label, _))) => label.to_string(),
            (Filter::Tag(tag), None) => tag.clone(),
            (Filter::All | Filter::Launch(_), None) => {
                unreachable!("FILTER_CHOICES lists All and every Launch")
            }
        }
    }

    pub fn admits(&self, bookmark: &Bookmark) -> bool {
        match self {
            Filter::All => true,
            Filter::Launch(launch) => bookmark.launch == *launch,
            Filter::Tag(tag) => bookmark.tags.iter().any(|t| same_tag(t, tag)),
        }
    }

    /// Its entry in [`FILTER_CHOICES`]; `None` for a tag.
    fn choice(&self) -> Option<(&'static str, &'static str)> {
        let at = match self {
            Filter::All => 0,
            Filter::Launch(launch) => FILTER_LAUNCHES.iter().position(|l| l == launch)? + 1,
            Filter::Tag(_) => return None,
        };
        Some(FILTER_CHOICES[at])
    }
}

/// The filters worth offering over `all`: All, each launch and then each tag by
/// how many bookmarks carry it, every one narrower than All and not empty, plus
/// `current` wherever it would be missing. Only All when nothing narrows it.
pub fn filters(all: &[Bookmark], current: &Filter) -> Vec<Filter> {
    let narrows = |filter: &Filter| {
        let n = all.iter().filter(|b| filter.admits(b)).count();
        n > 0 && n < all.len()
    };
    let mut out = vec![Filter::All];
    out.extend(
        FILTER_LAUNCHES
            .map(Filter::Launch)
            .into_iter()
            .filter(narrows),
    );
    let mut tags: Vec<(&str, usize)> = Vec::new();
    for tag in all.iter().flat_map(|b| &b.tags) {
        match tags.iter_mut().find(|(t, _)| same_tag(t, tag)) {
            Some((_, n)) => *n += 1,
            None => tags.push((tag, 1)),
        }
    }
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    out.extend(
        tags.into_iter()
            .map(|(tag, _)| Filter::parse(tag))
            .filter(|f| matches!(f, Filter::Tag(_)) && narrows(f)),
    );
    if !out.contains(current) {
        out.push(current.clone());
    }
    out
}

/// Indices into `all` of the bookmarks `filter` admits, in `sort` order;
/// `label` is what a bookmark is shown as, for the name sort.
pub fn view(
    all: &[Bookmark],
    filter: &Filter,
    sort: HomeSort,
    label: impl Fn(&Bookmark) -> String,
) -> Vec<usize> {
    let mut out: Vec<usize> = (0..all.len()).filter(|&i| filter.admits(&all[i])).collect();
    match sort {
        // `all` is in manual order already.
        HomeSort::Manual => {}
        HomeSort::Recent => out.sort_by_key(|&i| std::cmp::Reverse(all[i].last_opened)),
        HomeSort::Name => out.sort_by_cached_key(|&i| label(&all[i]).to_lowercase()),
    }
    out
}

/// A bookmark as its file spells it. Keys at their default are left out.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Raw {
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "is_default_launch")]
    launch: Launch,
    #[serde(skip_serializing_if = "is_false")]
    reader: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    order: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_opened: Option<Datetime>,
    /// The config sections it overrides, at the file's top level.
    #[serde(flatten)]
    settings: toml::Table,
}

/// A URL as a site: its host without `www.` and its path without the trailing
/// slash, for http(s) only.
struct SiteKey {
    host: String,
    path: String,
}

impl SiteKey {
    fn of(url: &str) -> Option<Self> {
        let host = crate::data::page_icons::host_key(url)?;
        let parsed = url::Url::parse(url).ok()?;
        let path = parsed.path().trim_end_matches('/').to_string();
        Some(Self { host, path })
    }

    /// Whether `page`'s path is this one's or under it, at a segment boundary.
    fn contains(&self, page: &SiteKey) -> bool {
        page.path
            .strip_prefix(&self.path)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }
}

/// The [`overlay::SITE_KEYS`] part of bookmark `id`'s extra tables; the rest
/// is logged and dropped.
fn site_settings(id: &str, extra: toml::Table) -> toml::Table {
    let kept = overlay::pick(&extra, overlay::SITE_KEYS);
    let dropped = overlay::changes(&extra, &kept);
    if !dropped.is_empty() {
        let keys: Vec<&String> = dropped.keys().collect();
        log::warn!("bookmark `{id}`: no site setting under {keys:?}; ignored");
    }
    kept
}

fn is_default_launch(launch: &Launch) -> bool {
    *launch == Launch::default()
}

fn is_false(on: &bool) -> bool {
    !on
}

/// One saved page.
pub struct Bookmark {
    /// The file stem; fixed for the bookmark's life.
    pub id: String,
    /// What the bookmark is shown as; empty shows the site.
    pub name: String,
    pub url: String,
    /// Groups it belongs to; tags never change what opening it does.
    pub tags: Vec<String>,
    pub launch: Launch,
    /// Open it in reader view.
    pub reader: bool,
    /// Manual position: a lower one comes first, none comes after every one.
    pub order: Option<i64>,
    /// Unix seconds it was last opened, `0` for never.
    pub last_opened: u64,
    /// Overrides of the [`overlay::SITE_KEYS`] settings while its site is shown,
    /// in the config's shape.
    pub settings: toml::Table,
}

impl Bookmark {
    /// Whether removing it would lose more than the address.
    pub fn has_settings(&self) -> bool {
        !self.tags.is_empty()
            || self.launch != Launch::default()
            || self.reader
            || !self.settings.is_empty()
    }

    fn from_raw(id: String, raw: Raw) -> Self {
        let settings = site_settings(&id, raw.settings);
        Self {
            id,
            name: raw.name,
            url: normalize(&raw.url),
            tags: clean_tags(raw.tags),
            launch: raw.launch,
            reader: raw.reader,
            order: raw.order,
            last_opened: raw.last_opened.as_ref().map_or(0, from_datetime),
            settings,
        }
    }

    fn to_raw(&self) -> Raw {
        Raw {
            name: self.name.clone(),
            url: self.url.clone(),
            tags: self.tags.clone(),
            launch: self.launch,
            reader: self.reader,
            order: self.order,
            last_opened: (self.last_opened != 0).then(|| to_datetime(self.last_opened)),
            settings: self.settings.clone(),
        }
    }

    /// Write its file; failures are logged, the bookmark stays in memory.
    fn save(&self) {
        let text = match toml::to_string_pretty(&self.to_raw()) {
            Ok(text) => text,
            Err(e) => {
                log::warn!("bookmark: could not serialize `{}`: {e}", self.id);
                return;
            }
        };
        let _ = std::fs::create_dir_all(dir());
        let path = file(&self.id);
        match super::write_atomic(&path, &text) {
            Ok(()) => log::info!("bookmark: wrote `{path}`"),
            Err(e) => log::warn!("bookmark: could not write `{path}`: {e}"),
        }
    }

    fn delete(&self) {
        let path = file(&self.id);
        match std::fs::remove_file(&path) {
            Ok(()) => log::info!("bookmark: removed `{path}`"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("bookmark: could not remove `{path}`: {e}"),
        }
    }
}

/// Every bookmark, with the highlighted row of their list.
pub struct Bookmarks {
    /// In manual order (see [`manual_order`]).
    items: Vec<Bookmark>,
    /// Highlighted row in the menu's Bookmarks section.
    cursor: super::ListCursor,
}

impl Bookmarks {
    pub fn load() -> Self {
        let mut items = match std::fs::read_dir(dir()) {
            Ok(entries) => read_files(entries),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => first_run(),
            // Not a first run: seeding would write over what is there.
            Err(e) => {
                log::error!("bookmark: could not read `{}`: {e}", dir());
                Vec::new()
            }
        };
        items.sort_by(manual_order);
        Self {
            items,
            cursor: super::ListCursor::new(0),
        }
    }

    /// Every bookmark, in manual order.
    pub fn all(&self) -> &[Bookmark] {
        &self.items
    }

    /// The bookmark of `url`, by exact match.
    pub fn find(&self, url: &str) -> Option<&Bookmark> {
        self.items.iter().find(|b| b.url == url)
    }

    pub fn contains(&self, url: &str) -> bool {
        self.find(url).is_some()
    }

    /// Bookmark `url` at the end of the manual order, under `name` (empty shows
    /// the site), opening as `launch`. Writes its file.
    pub fn add(&mut self, url: &str, name: String, launch: Launch) {
        let taken: Vec<String> = self.items.iter().map(|b| b.id.clone()).collect();
        let stem = match name.is_empty() {
            true => crate::data::page_icons::host_key(url).unwrap_or_default(),
            false => name.clone(),
        };
        let order = self.items.iter().filter_map(|b| b.order).max().unwrap_or(0) + 1;
        let bookmark = Bookmark {
            id: super::new_id(&stem, &taken, FALLBACK_ID),
            name,
            url: normalize(url),
            tags: Vec::new(),
            launch,
            reader: false,
            order: Some(order),
            last_opened: 0,
            settings: toml::Table::new(),
        };
        bookmark.save();
        self.items.push(bookmark);
        self.items.sort_by(manual_order);
    }

    /// Remove the bookmark at `index` in [`Self::all`] and its file.
    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index).delete();
            self.cursor.clamp(self.items.len());
        }
    }

    /// The index in [`Self::all`] of bookmark `id`.
    pub fn position(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|b| b.id == id)
    }

    /// Rename the bookmark at `index`; empty shows the site. Writes its file.
    pub fn rename(&mut self, index: usize, name: String) {
        if let Some(bookmark) = self.items.get_mut(index) {
            bookmark.name = name;
            bookmark.save();
            self.items.sort_by(manual_order);
        }
    }

    /// Set how the bookmark at `index` opens. Writes its file.
    pub fn set_launch(&mut self, index: usize, launch: Launch) {
        if let Some(bookmark) = self.items.get_mut(index) {
            bookmark.launch = launch;
            bookmark.save();
        }
    }

    /// Set whether the bookmark at `index` opens in reader view. Writes its file.
    pub fn set_reader(&mut self, index: usize, reader: bool) {
        if let Some(bookmark) = self.items.get_mut(index) {
            bookmark.reader = reader;
            bookmark.save();
        }
    }

    /// Replace the setting overrides of bookmark `id`, writing its file when
    /// they changed.
    pub fn set_settings(&mut self, id: &str, settings: toml::Table) {
        if let Some(bookmark) = self.items.iter_mut().find(|b| b.id == id) {
            if bookmark.settings != settings {
                bookmark.settings = settings;
                bookmark.save();
            }
        }
    }

    /// The bookmark whose site `url` is on: the same host, the longest path
    /// prefix, else the first of that host in manual order.
    pub fn for_url(&self, url: &str) -> Option<&Bookmark> {
        let page = SiteKey::of(url)?;
        let mut host_match = None;
        let mut best: Option<(&Bookmark, usize)> = None;
        for bookmark in &self.items {
            let Some(site) = SiteKey::of(&bookmark.url) else {
                continue;
            };
            if site.host != page.host {
                continue;
            }
            host_match.get_or_insert(bookmark);
            if site.contains(&page) && best.is_none_or(|(_, len)| site.path.len() > len) {
                best = Some((bookmark, site.path.len()));
            }
        }
        best.map(|(bookmark, _)| bookmark).or(host_match)
    }

    /// The bookmark of id `id`.
    pub fn get(&self, id: &str) -> Option<&Bookmark> {
        self.items.iter().find(|b| b.id == id)
    }

    /// Replace the tags of the bookmark at `index`. Writes its file.
    pub fn set_tags(&mut self, index: usize, tags: Vec<String>) {
        if let Some(bookmark) = self.items.get_mut(index) {
            bookmark.tags = clean_tags(tags);
            bookmark.save();
        }
    }

    /// Swap the manual positions of the bookmarks at `a` and `b`. Writes their
    /// files, and first gives a position to every bookmark that lacks one.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a == b || a >= self.items.len() || b >= self.items.len() {
            return;
        }
        let mut next = self.items.iter().filter_map(|b| b.order).max().unwrap_or(0);
        for bookmark in self.items.iter_mut().filter(|b| b.order.is_none()) {
            next += 1;
            bookmark.order = Some(next);
            bookmark.save();
        }
        let (order_a, order_b) = (self.items[a].order, self.items[b].order);
        self.items[a].order = order_b;
        self.items[b].order = order_a;
        self.items[a].save();
        self.items[b].save();
        self.items.sort_by(manual_order);
    }

    /// Stamp `url`'s bookmark as opened now. Writes its file, unless it was
    /// stamped this same second: an open and the visit it lands are one opening.
    pub fn opened(&mut self, url: &str) {
        let Some(bookmark) = self.items.iter_mut().find(|b| b.url == url) else {
            return;
        };
        let now = clock::now_unix();
        if bookmark.last_opened != now {
            bookmark.last_opened = now;
            bookmark.save();
        }
    }

    pub fn selected(&self) -> usize {
        self.cursor.selected()
    }

    /// The highlighted row's index in [`Self::all`].
    pub fn selected_index(&self) -> Option<usize> {
        self.cursor.entry_index().filter(|&i| i < self.items.len())
    }

    /// Reset the highlight to the top (called when the menu opens).
    pub fn reset(&mut self) {
        self.cursor.reset(self.items.len());
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        self.cursor.move_sel(dy, self.items.len());
    }
}

/// Whether the bookmark of id `id` is on disk and reads as one; the reason
/// when not. An id that would reach outside the directory names none.
pub fn check(id: &str) -> Result<(), String> {
    let inside = !id.is_empty() && !id.starts_with('.') && !id.contains(['/', '\\']);
    let path = file(id);
    if !inside || !Path::new(&path).is_file() {
        return Err(format!("no bookmark `{id}`"));
    }
    read_raw(Path::new(&path))
        .map(|_| ())
        .map_err(|why| format!("bookmark `{id}` {why}"))
}

/// A bookmark's name from its page's title: the part before the site's name,
/// empty when the title is unknown and the site should show instead.
pub fn name_from_title(title: &str, url: &str) -> String {
    let title = title.trim();
    if title.is_empty() || title == url {
        return String::new();
    }
    let cut = TITLE_SEPARATORS
        .iter()
        .filter_map(|sep| title.find(sep))
        .min()
        .unwrap_or(title.len());
    match title[..cut].trim() {
        "" => title.to_string(),
        head => head.to_string(),
    }
}

/// Positioned bookmarks by position, then the rest by name; the id settles ties
/// so the order does not depend on the directory listing.
fn manual_order(a: &Bookmark, b: &Bookmark) -> std::cmp::Ordering {
    let key = |b: &Bookmark| (b.order.is_none(), b.order);
    key(a)
        .cmp(&key(b))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.id.cmp(&b.id))
}

/// `url` as the engine reports it once loaded (a bare origin gains its `/`),
/// so an exact match finds a page's bookmark; as given when it does not parse.
fn normalize(url: &str) -> String {
    url::Url::parse(url).map_or_else(|_| url.to_string(), |parsed| parsed.to_string())
}

/// Tags typed as one comma-separated line.
pub fn split_tags(text: &str) -> Vec<String> {
    clean_tags(text.split(',').map(str::to_string).collect())
}

fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|t| same_tag(t, tag)) {
            out.push(tag.to_string());
        }
    }
    out
}

/// Whether `a` and `b` are one tag, spelled in either case.
fn same_tag(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

fn dir() -> String {
    super::data_path(DIR)
}

fn file(id: &str) -> String {
    format!("{}/{id}.toml", dir())
}

/// The bookmark file at `path` as its table; why not, when it is not one.
fn read_raw(path: &Path) -> Result<Raw, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not be read: {e}"))?;
    let raw: Raw = toml::from_str(&text).map_err(|e| format!("is invalid: {e}"))?;
    match raw.url.is_empty() {
        true => Err("has no url".to_string()),
        false => Ok(raw),
    }
}

/// Every `*.toml` in the directory; a file that does not read as a bookmark is
/// logged and skipped, and of two with one url the first is kept.
fn read_files(entries: std::fs::ReadDir) -> Vec<Bookmark> {
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let mut items: Vec<Bookmark> = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let raw = match read_raw(&path) {
            Ok(raw) => raw,
            Err(why) => {
                log::error!("bookmark `{}` {why}; ignored", path.display());
                continue;
            }
        };
        let bookmark = Bookmark::from_raw(id.to_string(), raw);
        if items.iter().any(|b| b.url == bookmark.url) {
            log::warn!("bookmark `{}` repeats a url; ignored", path.display());
            continue;
        }
        items.push(bookmark);
    }
    items
}

/// Create the directory and seed it with [`DEFAULTS`].
fn first_run() -> Vec<Bookmark> {
    if let Err(e) = std::fs::create_dir_all(dir()) {
        log::warn!("bookmark: could not create `{}`: {e}", dir());
    }
    let mut store = Bookmarks {
        items: Vec::new(),
        cursor: super::ListCursor::new(0),
    };
    for (name, url) in DEFAULTS {
        store.add(url, name.to_string(), Launch::Browser);
    }
    store.items
}

/// Unix seconds as a TOML datetime in UTC.
fn to_datetime(secs: u64) -> Datetime {
    let (year, month, day) = clock::civil_from_days((secs / SECS_PER_DAY) as i64);
    let tod = secs % SECS_PER_DAY;
    Datetime {
        date: Some(Date {
            year: year as u16,
            month: month as u8,
            day: day as u8,
        }),
        time: Some(Time {
            hour: (tod / SECS_PER_HOUR) as u8,
            minute: (tod % SECS_PER_HOUR / SECS_PER_MINUTE) as u8,
            second: Some((tod % SECS_PER_MINUTE) as u8),
            nanosecond: None,
        }),
        offset: Some(Offset::Z),
    }
}

/// A TOML datetime as unix seconds, its offset applied; a bare time or a date
/// before the epoch reads as never.
fn from_datetime(at: &Datetime) -> u64 {
    let Some(date) = at.date else {
        return 0;
    };
    let days = clock::days_from_civil(date.year as i64, date.month as u32, date.day as u32);
    let tod = at.time.map_or(0, |t| {
        t.hour as i64 * SECS_PER_HOUR as i64
            + t.minute as i64 * SECS_PER_MINUTE as i64
            + t.second.unwrap_or(0) as i64
    });
    let offset = match at.offset {
        Some(Offset::Custom { minutes }) => minutes as i64 * SECS_PER_MINUTE as i64,
        Some(Offset::Z) | None => 0,
    };
    u64::try_from(days * SECS_PER_DAY as i64 + tod - offset).unwrap_or(0)
}

const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: u64 = 60 * SECS_PER_MINUTE;
const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_round_trips() {
        for secs in [0, 1, 1_760_805_997, 4_102_444_799] {
            assert_eq!(from_datetime(&to_datetime(secs)), secs);
        }
        assert_eq!(
            to_datetime(1_760_805_997).to_string(),
            "2025-10-18T16:46:37Z"
        );
        let local: Datetime = "2025-10-18T19:46:37+03:00".parse().unwrap();
        assert_eq!(from_datetime(&local), 1_760_805_997);
    }

    #[test]
    fn a_title_names_its_page_not_the_site() {
        let url = "https://example.com/";
        assert_eq!(
            name_from_title("Half-Life 2 - Wikipedia", url),
            "Half-Life 2"
        );
        assert_eq!(
            name_from_title("Vampire Survivors | itch.io", url),
            "Vampire Survivors"
        );
        assert_eq!(name_from_title("r/SBCGaming", url), "r/SBCGaming");
        assert_eq!(name_from_title(" - leading", url), "- leading");
        assert_eq!(name_from_title(url, url), "");
        assert_eq!(name_from_title("  ", url), "");
    }

    /// The engine reports a bare origin with its `/`; a bookmark spelled
    /// without one must still match the page.
    #[test]
    fn a_url_reads_as_the_engine_spells_it() {
        let raw = Raw {
            url: "https://en.wikipedia.org".to_string(),
            ..Raw::default()
        };
        let bookmark = Bookmark::from_raw("wiki".to_string(), raw);
        assert_eq!(bookmark.url, "https://en.wikipedia.org/");
        assert_eq!(normalize("retsurf:home"), "retsurf:home");
        assert_eq!(normalize("not a url"), "not a url");
    }

    #[test]
    fn defaults_are_left_out_of_the_file() {
        let bookmark = Bookmark::from_raw(
            "wiki".to_string(),
            Raw {
                name: "Wikipedia".to_string(),
                url: "https://en.wikipedia.org".to_string(),
                order: Some(1),
                ..Raw::default()
            },
        );
        let text = toml::to_string_pretty(&bookmark.to_raw()).unwrap();
        assert_eq!(
            text,
            "name = \"Wikipedia\"\nurl = \"https://en.wikipedia.org/\"\norder = 1\n"
        );
        assert!(!bookmark.has_settings());
    }

    #[test]
    fn a_game_file_reads_back() {
        let text = r#"
            name = "Vampire Survivors"
            url = "https://poncle.itch.io/vampire-survivors"
            tags = ["roguelike", " roguelike ", ""]
            launch = "game"
            order = 7
            last_opened = 2025-10-18T16:46:37Z

            [game_mode]
            input_map = "keys"

            [game_mode.view]
            scaling = "fit"
        "#;
        let raw: Raw = toml::from_str(text).unwrap();
        let bookmark = Bookmark::from_raw("vs".to_string(), raw);
        assert_eq!(bookmark.tags, ["roguelike"]);
        assert_eq!(bookmark.launch, Launch::Game);
        assert_eq!(bookmark.last_opened, 1_760_805_997);
        assert!(bookmark.settings["game_mode"]
            .as_table()
            .is_some_and(|t| t.contains_key("view")));
        assert!(bookmark.has_settings());
    }

    fn tagged(id: &str, launch: Launch, tags: &[&str], last_opened: u64) -> Bookmark {
        Bookmark {
            launch,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            last_opened,
            ..Bookmark::from_raw(id.to_string(), Raw::default())
        }
    }

    fn at(id: &str, url: &str) -> Bookmark {
        Bookmark {
            url: url.to_string(),
            ..Bookmark::from_raw(id.to_string(), Raw::default())
        }
    }

    fn store(items: Vec<Bookmark>) -> Bookmarks {
        Bookmarks {
            items,
            cursor: crate::data::ListCursor::new(0),
        }
    }

    #[test]
    fn a_page_takes_the_bookmark_of_its_site() {
        let store = store(vec![
            at("itch", "https://itch.io/"),
            at("html5", "https://itch.io/games/html5"),
            at("dev", "https://berzee.itch.io/"),
            at("wiki", "https://www.wikipedia.org/wiki"),
        ]);
        let site = |url| store.for_url(url).map(|b| b.id.as_str());
        assert_eq!(site("https://itch.io/games/html5/top"), Some("html5"));
        assert_eq!(site("https://itch.io/games/html5"), Some("html5"));
        assert_eq!(site("https://itch.io/games/html5x"), Some("itch"));
        assert_eq!(site("https://itch.io/jams"), Some("itch"));
        assert_eq!(site("https://berzee.itch.io/a-game"), Some("dev"));
        assert_eq!(site("https://wikipedia.org/w/index.php"), Some("wiki"));
        assert_eq!(site("https://example.com/"), None);
        assert_eq!(site("retsurf:home"), None);
    }

    #[test]
    fn site_settings_read_back_and_the_rest_is_dropped() {
        let text = r#"
            name = "itch.io"
            url = "https://itch.io/"

            [browser]
            user_agent = "mobile"
            restore_tabs = false

            [adblock]
            enabled = false

            [interface]
            scale = 2.0
        "#;
        let raw: Raw = toml::from_str(text).unwrap();
        let bookmark = Bookmark::from_raw("itch".to_string(), raw);
        let expected: toml::Table =
            toml::from_str("[browser]\nuser_agent = \"mobile\"\n[adblock]\nenabled = false\n")
                .unwrap();
        assert_eq!(bookmark.settings, expected);
        let written = toml::to_string_pretty(&bookmark.to_raw()).unwrap();
        let again = Bookmark::from_raw("itch".to_string(), toml::from_str(&written).unwrap());
        assert_eq!(again.settings, expected);
        assert!(
            written.starts_with("name = \"itch.io\"\nurl = "),
            "{written}"
        );
    }

    #[test]
    fn a_filter_token_reads_back() {
        for token in ["all", "sites", "apps", "games", "roguelike"] {
            assert_eq!(Filter::parse(token).token(), token);
        }
        assert_eq!(Filter::parse(""), Filter::All);
        assert_eq!(Filter::parse(" Games "), Filter::Launch(Launch::Game));
    }

    #[test]
    fn only_narrowing_filters_are_offered() {
        let all = [
            tagged("a", Launch::Browser, &["news"], 0),
            tagged("b", Launch::Game, &["arcade", "news"], 0),
            tagged("c", Launch::Game, &["arcade"], 0),
            tagged("d", Launch::Game, &["games"], 0),
        ];
        let offered = filters(&all, &Filter::All);
        assert_eq!(
            offered,
            [
                Filter::All,
                Filter::Launch(Launch::Browser),
                Filter::Launch(Launch::Game),
                Filter::Tag("arcade".to_string()),
                Filter::Tag("news".to_string()),
            ]
        );
        let missing = Filter::Launch(Launch::App);
        assert_eq!(filters(&all, &missing).last(), Some(&missing));
        assert_eq!(filters(&all[..1], &Filter::All), [Filter::All]);
    }

    #[test]
    fn a_view_filters_then_sorts() {
        let all = [
            tagged("b", Launch::Game, &[], 5),
            tagged("a", Launch::Browser, &[], 9),
            tagged("c", Launch::Game, &[], 7),
        ];
        let id = |b: &Bookmark| b.id.clone();
        let games = Filter::Launch(Launch::Game);
        assert_eq!(view(&all, &games, HomeSort::Manual, id), [0, 2]);
        assert_eq!(view(&all, &Filter::All, HomeSort::Recent, id), [1, 2, 0]);
        assert_eq!(view(&all, &Filter::All, HomeSort::Name, id), [1, 0, 2]);
    }

    /// A tag reads the same in either case: one tab, spelled as first seen,
    /// admitting every spelling.
    #[test]
    fn a_tag_filter_ignores_case() {
        let all = [
            tagged("a", Launch::Browser, &["News"], 0),
            tagged("b", Launch::Browser, &["news"], 0),
            tagged("c", Launch::Browser, &[], 0),
        ];
        let news = Filter::parse("NEWS");
        let id = |b: &Bookmark| b.id.clone();
        assert_eq!(view(&all, &news, HomeSort::Manual, id), [0, 1]);
        let offered = filters(&all, &news);
        assert_eq!(offered.len(), 2);
        assert_eq!(offered[1].label(), "News");
        assert_eq!(split_tags("News, news"), ["News"]);
    }

    #[test]
    fn tags_split_on_commas() {
        assert_eq!(split_tags(" arcade, ,co-op ,arcade"), ["arcade", "co-op"]);
        assert!(split_tags("  ").is_empty());
    }

    #[test]
    fn positioned_bookmarks_lead_by_position() {
        let at = |id: &str, order: Option<i64>| Bookmark {
            order,
            ..Bookmark::from_raw(id.to_string(), Raw::default())
        };
        let mut items = [
            at("c", None),
            at("b", Some(9)),
            at("a", Some(2)),
            at("d", None),
        ];
        items.sort_by(manual_order);
        let ids: Vec<&str> = items.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d"]);
    }
}
