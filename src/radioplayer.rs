//! Playing a station.
//!
//! mpv does the listening: it is part of Omarchy, plays every stream format and playlist,
//! reconnects by itself and tells what a station is playing. omafile starts one mpv
//! without a window, talks to it over its JSON socket, and presents the result to the
//! desktop as a media player of its own (`org.mpris.MediaPlayer2.omafile`). That is all
//! it takes for the radio to appear in omafile's Now Playing view, in the sidebar's mini
//! player, on the media keys and in Omarchy's own bar: they show whatever is on MPRIS.
//!
//! Next and Previous move between stations (between episodes for a podcast), and while
//! something plays omafile stays alive without a window, so closing the file manager does
//! not stop the music; Stop does.

use crate::feeds;
use crate::i18n::tr;
use crate::json::{self, Json};
use crate::radio::{self, Kind, Station};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Stopped,
    /// Asked for, not sounding yet.
    Loading,
    Playing,
    Paused,
    /// See `Now::error`.
    Failed,
}

/// A podcast episode that can be played.
#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub title: String,
    pub audio: String,
    pub date: i64,
}

#[derive(Clone, Debug)]
pub struct Now {
    pub station: Option<Station>,
    pub state: State,
    /// What plays: the song a stream names, or the episode.
    pub title: String,
    pub artist: String,
    pub error: String,
    /// Seconds; 0 for a stream, which has no beginning and no end.
    pub position: f64,
    pub duration: f64,
    /// 0 to 1.
    pub volume: f64,
    pub episodes: Vec<Episode>,
    pub episode: usize,
}

impl Default for Now {
    fn default() -> Self {
        Now { station: None, state: State::Stopped, title: String::new(), artist: String::new(), error: String::new(), position: 0.0, duration: 0.0, volume: 1.0, episodes: Vec::new(), episode: 0 }
    }
}

impl Now {
    pub fn active(&self) -> bool {
        matches!(self.state, State::Loading | State::Playing | State::Paused)
    }

    /// Whether this station is the one that plays (or is about to).
    pub fn is_on(&self, url: &str) -> bool {
        self.active() && self.station.as_ref().is_some_and(|s| s.url == url)
    }

    /// Title and artist as a player shows them: the song over the station's name, the
    /// station's name alone while it has not said what it plays.
    pub fn shown(&self) -> (String, String) {
        let station = self.station.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        if self.title.trim().is_empty() {
            (station, self.artist.clone())
        } else if self.artist.trim().is_empty() {
            (self.title.clone(), station)
        } else {
            (self.title.clone(), self.artist.clone())
        }
    }
}

/// The episodes of a feed that have sound, newest first.
pub fn episodes_of(xml: &str) -> Vec<Episode> {
    let Some((_, articles)) = feeds::parse_feed(xml) else { return Vec::new() };
    articles.into_iter().filter_map(|a| a.audio.filter(|u| radio::valid_url(u)).map(|audio| Episode { title: a.title, audio, date: a.date })).collect()
}

/// What omafile starts: mpv without a window, waiting for commands on a socket. No user
/// scripts (Omarchy's mpv would announce itself on MPRIS a second time), no youtube-dl
/// (an address is played as it is or not at all). The shell around it ends mpv when
/// omafile is gone, however it went.
pub fn mpv_argv(owner_pid: u32, socket: &str, extra: &str) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "sh".into(),
        "-c".into(),
        "mpv \"$@\" & m=$!; while kill -0 \"$0\" 2>/dev/null && kill -0 $m 2>/dev/null; do sleep 2; done; kill $m 2>/dev/null".into(),
        owner_pid.to_string(),
        "--idle=yes".into(),
        "--no-video".into(),
        "--no-terminal".into(),
        "--really-quiet".into(),
        "--load-scripts=no".into(),
        "--ytdl=no".into(),
        "--audio-display=no".into(),
        "--force-window=no".into(),
        "--network-timeout=20".into(),
        "--title=omafile radio".into(),
        format!("--input-ipc-server={socket}"),
    ];
    argv.extend(extra.split_whitespace().map(str::to_string));
    argv
}

fn command(parts: &[Json]) -> String {
    format!("{}\n", Json::obj(vec![("command", Json::Array(parts.to_vec()))]))
}

/// The number on the question "where are you?", by which its answer is known.
const POSITION: f64 = 7.0;

fn question(id: f64, parts: &[Json]) -> String {
    format!("{}\n", Json::obj(vec![("command", Json::Array(parts.to_vec())), ("request_id", Json::Number(id))]))
}

/// A line mpv sent, as what it means here.
#[derive(Debug, PartialEq)]
pub enum Said {
    Pause(bool),
    /// What the stream calls the current song.
    Title(String),
    Position(f64),
    Duration(f64),
    Volume(f64),
    Loaded,
    /// Playback ended by itself; the text is mpv's reason when it was an error.
    Ended(Option<String>),
    Nothing,
}

pub fn understand(line: &str) -> Said {
    let Some(msg) = json::parse(line) else { return Said::Nothing };
    if msg.num("request_id") == Some(POSITION) {
        return msg.num("data").map(Said::Position).unwrap_or(Said::Nothing);
    }
    match msg.str("event") {
        Some("property-change") => match (msg.str("name"), msg.get("data")) {
            (Some("pause"), Some(Json::Bool(b))) => Said::Pause(*b),
            // A stream says what it plays as `icy-title` ("Artist - Title"), or with tags.
            (Some("metadata"), Some(tags)) => {
                let tag = |wanted: &str| tags.entries().into_iter().find(|(key, _)| key.eq_ignore_ascii_case(wanted)).and_then(|(_, value)| if let Json::String(text) = value { Some(text.trim().to_string()) } else { None }).filter(|text| !text.is_empty());
                match (tag("icy-title"), tag("title"), tag("artist")) {
                    (Some(icy), _, _) => Said::Title(icy),
                    (None, Some(title), Some(artist)) => Said::Title(format!("{artist} - {title}")),
                    (None, Some(title), None) => Said::Title(title),
                    _ => Said::Nothing,
                }
            }
            (Some("duration"), Some(Json::Number(n))) => Said::Duration(*n),
            (Some("volume"), Some(Json::Number(n))) => Said::Volume(*n / 100.0),
            _ => Said::Nothing,
        },
        Some("file-loaded") => Said::Loaded,
        Some("end-file") => match msg.str("reason") {
            Some("error") => Said::Ended(Some(msg.str("file_error").unwrap_or("cannot be played").to_string())),
            Some("eof") => Said::Ended(None),
            // "stop", "quit", "redirect": something omafile or mpv did on purpose.
            _ => Said::Nothing,
        },
        _ => Said::Nothing,
    }
}

struct Mpv {
    out: gio::OutputStream,
    socket: PathBuf,
}

type Listener = Box<dyn Fn(&Now)>;

pub struct RadioPlayer {
    now: RefCell<Now>,
    mpv: RefCell<Option<Mpv>>,
    starting: Cell<bool>,
    listeners: RefCell<Vec<Listener>>,
    /// Counts every "play this": an answer that comes back for an older request is dropped.
    request: Cell<u64>,
    bus: RefCell<Option<Served>>,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
}

thread_local! {
    static PLAYER: Rc<RadioPlayer> = Rc::new(RadioPlayer {
        now: RefCell::new(Now::default()),
        mpv: RefCell::new(None),
        starting: Cell::new(false),
        listeners: RefCell::new(Vec::new()),
        request: Cell::new(0),
        bus: RefCell::new(None),
        hold: RefCell::new(None),
    });
}

/// The one radio of this omafile, shared by all its windows.
pub fn player() -> Rc<RadioPlayer> {
    PLAYER.with(|p| p.clone())
}

pub fn mpv_installed() -> bool {
    glib::find_program_in_path("mpv").is_some()
}

fn last_path() -> PathBuf {
    glib::user_state_dir().join("omafile/radio-last")
}

impl RadioPlayer {
    pub fn now(&self) -> Now {
        self.now.borrow().clone()
    }

    pub fn connect_changed(&self, f: impl Fn(&Now) + 'static) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    fn changed(self: &Rc<Self>) {
        let now = self.now();
        self.serve(&now);
        // While something plays, omafile stays alive without a window.
        let app = gio::Application::default();
        let mut hold = self.hold.borrow_mut();
        match (now.active(), hold.is_some(), app) {
            (true, false, Some(app)) => *hold = Some(app.hold()),
            (false, true, _) => *hold = None,
            _ => {}
        }
        drop(hold);
        for listener in self.listeners.borrow().iter() {
            listener(&now);
        }
    }

    fn fail(self: &Rc<Self>, error: String) {
        {
            let mut now = self.now.borrow_mut();
            now.state = State::Failed;
            now.error = error;
        }
        self.changed();
    }

    fn send(&self, parts: &[Json]) -> bool {
        match self.mpv.borrow().as_ref() {
            Some(mpv) => mpv.out.write_all(command(parts).as_bytes(), gio::Cancellable::NONE).is_ok(),
            None => false,
        }
    }

    fn send_asking(&self, id: f64, parts: &[Json]) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            let _ = mpv.out.write_all(question(id, parts).as_bytes(), gio::Cancellable::NONE);
        }
    }

    /// Start mpv if it is not running and connect to its socket.
    async fn ensure_mpv(self: &Rc<Self>) -> Result<(), String> {
        if self.mpv.borrow().is_some() {
            return Ok(());
        }
        if !mpv_installed() {
            return Err(tr("mpv is needed to play radio, and it is not installed"));
        }
        if self.starting.replace(true) {
            // Another request is starting it: wait for that one.
            for _ in 0..60 {
                glib::timeout_future(Duration::from_millis(75)).await;
                if self.mpv.borrow().is_some() {
                    return Ok(());
                }
            }
            return Err(tr("mpv did not start"));
        }
        let result = self.start_mpv().await;
        self.starting.set(false);
        result
    }

    async fn start_mpv(self: &Rc<Self>) -> Result<(), String> {
        let socket = glib::user_runtime_dir().join(format!("omafile-radio-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let extra = std::env::var("OMAFILE_RADIO_MPV_ARGS").unwrap_or_default();
        let argv = mpv_argv(std::process::id(), &socket.to_string_lossy(), &extra);
        let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
        let process = gio::Subprocess::newv(&args, gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE).map_err(|e| e.message().to_string())?;
        // The socket appears a moment after mpv has started.
        let client = gio::SocketClient::new();
        let address = gio::UnixSocketAddress::new(&socket);
        let mut connection = None;
        for _ in 0..60 {
            glib::timeout_future(Duration::from_millis(75)).await;
            if let Ok(c) = client.connect_future(&address).await {
                connection = Some(c);
                break;
            }
        }
        let Some(connection) = connection else {
            process.force_exit();
            return Err(tr("mpv did not start"));
        };
        let out = connection.output_stream();
        let input = gio::DataInputStream::new(&connection.input_stream());
        self.mpv.replace(Some(Mpv { out, socket }));
        for (id, name) in ["pause", "metadata", "duration", "volume"].iter().enumerate() {
            self.send(&[Json::s("observe_property"), Json::Number(id as f64 + 1.0), Json::s(*name)]);
        }
        // Where an episode is, asked once a second: mpv would report it many times as often.
        let this = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(player) = this.upgrade() else { return glib::ControlFlow::Break };
            if player.mpv.borrow().is_none() {
                return glib::ControlFlow::Break;
            }
            let asks = {
                let now = player.now.borrow();
                now.state == State::Playing && now.duration > 0.0
            };
            if asks {
                player.send_asking(POSITION, &[Json::s("get_property"), Json::s("time-pos")]);
            }
            glib::ControlFlow::Continue
        });
        // Everything mpv says, a line at a time, for as long as it lives.
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let _keep = connection;
            loop {
                let line = input.read_line_utf8_future(glib::Priority::DEFAULT).await;
                let Some(player) = this.upgrade() else { break };
                match line {
                    Ok(Some(text)) => player.heard(understand(&text)),
                    // The socket closed: mpv is gone.
                    _ => {
                        player.mpv_gone();
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    fn mpv_gone(self: &Rc<Self>) {
        if let Some(mpv) = self.mpv.borrow_mut().take() {
            let _ = std::fs::remove_file(&mpv.socket);
        }
        let was_active = self.now.borrow().active();
        if was_active {
            self.fail(tr("mpv stopped"));
        }
    }

    fn heard(self: &Rc<Self>, said: Said) {
        let mut tell = true;
        {
            let mut now = self.now.borrow_mut();
            if now.station.is_none() {
                return;
            }
            let live = now.station.as_ref().is_some_and(|s| s.kind == Kind::Live);
            match said {
                Said::Pause(paused) if matches!(now.state, State::Playing | State::Paused) => now.state = if paused { State::Paused } else { State::Playing },
                Said::Loaded => {
                    now.state = State::Playing;
                    now.error.clear();
                }
                // A stream names its songs; an episode has its title from the feed.
                Said::Title(title) if live => {
                    let (artist, song) = radio::split_title(&title);
                    tell = now.title != song || now.artist != artist;
                    now.title = song;
                    now.artist = artist;
                }
                // Told to nobody: whoever shows the time asks for it.
                Said::Position(seconds) => {
                    now.position = seconds;
                    tell = false;
                }
                // Only an episode has a length: for a stream mpv counts what has arrived.
                Said::Duration(seconds) if !live => {
                    tell = (now.duration - seconds).abs() > 0.5;
                    now.duration = seconds;
                }
                Said::Volume(volume) => now.volume = volume.clamp(0.0, 1.0),
                // mpv's own words ("loading failed") say less than this.
                Said::Ended(Some(_)) if now.active() => {
                    now.state = State::Failed;
                    now.error = tr("The address does not answer or sends no sound");
                }
                // An episode that is over is over; a stream that ends has dropped out.
                Said::Ended(None) if now.active() => {
                    if live {
                        now.state = State::Failed;
                        now.error = tr("The stream ended");
                    } else {
                        now.state = State::Stopped;
                    }
                }
                _ => tell = false,
            }
        }
        if tell {
            self.changed();
        }
    }

    async fn load(self: &Rc<Self>, request: u64, url: String) {
        if let Err(e) = self.ensure_mpv().await {
            if self.request.get() == request {
                self.fail(e);
            }
            return;
        }
        if self.request.get() != request {
            return;
        }
        self.send(&[Json::s("set_property"), Json::s("pause"), Json::Bool(false)]);
        if !self.send(&[Json::s("loadfile"), Json::s(url), Json::s("replace")]) {
            self.mpv_gone();
            self.fail(tr("mpv stopped"));
        }
    }

    /// Play a station from its beginning: the stream, or the newest episode.
    pub fn play(self: &Rc<Self>, station: &Station) {
        let request = self.request.get() + 1;
        self.request.set(request);
        {
            let mut now = self.now.borrow_mut();
            let volume = now.volume;
            *now = Now { station: Some(station.clone()), state: State::Loading, volume, ..Default::default() };
        }
        if let Some(dir) = last_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(last_path(), &station.url);
        self.changed();
        let (this, station) = (self.clone(), station.clone());
        glib::spawn_future_local(async move {
            match station.kind {
                Kind::Live => this.load(request, station.url.clone()).await,
                Kind::Podcast => {
                    let fetched = crate::feedsui::fetch(&station.url).await;
                    if this.request.get() != request {
                        return;
                    }
                    let episodes = match fetched {
                        Ok(bytes) => episodes_of(&String::from_utf8_lossy(&bytes)),
                        Err(e) => return this.fail(e),
                    };
                    if episodes.is_empty() {
                        return this.fail(tr("This feed has no episode with sound"));
                    }
                    this.now.borrow_mut().episodes = episodes;
                    this.play_episode(request, 0).await;
                }
            }
        });
    }

    async fn play_episode(self: &Rc<Self>, request: u64, index: usize) {
        let Some(episode) = self.now.borrow().episodes.get(index).cloned() else { return };
        {
            let mut now = self.now.borrow_mut();
            now.episode = index;
            now.title = episode.title.clone();
            now.artist.clear();
            now.position = 0.0;
            now.duration = 0.0;
            now.state = State::Loading;
        }
        self.changed();
        self.load(request, episode.audio).await;
    }

    /// The station that was on last, else the first of the list.
    pub fn resume_station() -> Option<Station> {
        let stations = radio::load();
        let last = std::fs::read_to_string(last_path()).unwrap_or_default();
        stations.iter().find(|s| s.url == last.trim()).or_else(|| stations.first()).cloned()
    }

    pub fn toggle(self: &Rc<Self>) {
        let state = self.now.borrow().state;
        match state {
            State::Playing => {
                self.send(&[Json::s("set_property"), Json::s("pause"), Json::Bool(true)]);
            }
            State::Paused => {
                self.send(&[Json::s("set_property"), Json::s("pause"), Json::Bool(false)]);
            }
            State::Loading => {}
            State::Stopped | State::Failed => {
                let station = self.now.borrow().station.clone().or_else(Self::resume_station);
                if let Some(station) = station {
                    self.play(&station);
                }
            }
        }
    }

    pub fn pause(self: &Rc<Self>) {
        if self.now.borrow().state == State::Playing {
            self.toggle();
        }
    }

    pub fn stop(self: &Rc<Self>) {
        self.request.set(self.request.get() + 1);
        self.send(&[Json::s("stop")]);
        {
            let mut now = self.now.borrow_mut();
            now.state = State::Stopped;
            now.position = 0.0;
        }
        self.changed();
    }

    /// The next station (`step` 1) or the one before (-1); within a podcast, the older or
    /// the newer episode while there is one.
    pub fn skip(self: &Rc<Self>, step: i32) {
        let now = self.now();
        let Some(station) = now.station else { return };
        if station.kind == Kind::Podcast && !now.episodes.is_empty() {
            let target = now.episode as i32 + step;
            if (0..now.episodes.len() as i32).contains(&target) {
                let request = self.request.get() + 1;
                self.request.set(request);
                let this = self.clone();
                glib::spawn_future_local(async move { this.play_episode(request, target as usize).await });
                return;
            }
        }
        if let Some(next) = radio::neighbour(&radio::load(), &station.url, step) {
            self.play(&next);
        }
    }

    pub fn set_volume(self: &Rc<Self>, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        self.now.borrow_mut().volume = volume;
        self.send(&[Json::s("set_property"), Json::s("volume"), Json::Number((volume * 100.0).round())]);
        self.changed();
    }

    /// Jump within an episode; a stream has nowhere to jump to.
    pub fn seek_to(self: &Rc<Self>, seconds: f64) {
        let now = self.now();
        if now.duration > 0.0 && now.active() {
            let target = seconds.clamp(0.0, (now.duration - 1.0).max(0.0));
            self.now.borrow_mut().position = target;
            self.send(&[Json::s("seek"), Json::Number(target), Json::s("absolute")]);
            self.seeked(target);
        }
    }

    /// Stop the music and end mpv: omafile is closing for good.
    pub fn shutdown(&self) {
        if let Some(mpv) = self.mpv.borrow_mut().take() {
            let _ = mpv.out.write_all(command(&[Json::s("quit")]).as_bytes(), gio::Cancellable::NONE);
            let _ = std::fs::remove_file(&mpv.socket);
            // Should mpv not listen any more, the shell around it ends it with omafile.
        }
    }
}

// ----- the radio as a media player on the session bus ------------------------------------------

const BUS_NAME: &str = "org.mpris.MediaPlayer2.omafile";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const XML: &str = r#"<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg type="x" name="Offset" direction="in"/></method>
    <method name="SetPosition"><arg type="o" name="TrackId" direction="in"/><arg type="x" name="Position" direction="in"/></method>
    <method name="OpenUri"><arg type="s" name="Uri" direction="in"/></method>
    <signal name="Seeked"><arg type="x" name="Position"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="readwrite"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="readwrite"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"#;

struct Served {
    connection: gio::DBusConnection,
    owner: Option<gio::OwnerId>,
    objects: Vec<gio::RegistrationId>,
}

/// What MPRIS calls the state.
pub fn status_word(state: State) -> &'static str {
    match state {
        State::Playing | State::Loading => "Playing",
        State::Paused => "Paused",
        State::Stopped | State::Failed => "Stopped",
    }
}

/// What a property of the player is right now. None for a name MPRIS does not have.
pub fn property(now: &Now, interface: &str, name: &str, art: Option<&str>) -> Option<glib::Variant> {
    let seekable = now.duration > 0.0;
    Some(match (interface, name) {
        (ROOT, "CanQuit") | (ROOT, "CanRaise") => true.to_variant(),
        (ROOT, "HasTrackList") => false.to_variant(),
        (ROOT, "Identity") => "omafile Radio".to_variant(),
        (ROOT, "DesktopEntry") => "io.github.whogotpwned.Omafile".to_variant(),
        (ROOT, "SupportedUriSchemes") => vec!["http".to_string(), "https".to_string()].to_variant(),
        (ROOT, "SupportedMimeTypes") => Vec::<String>::new().to_variant(),
        (PLAYER_IFACE, "PlaybackStatus") => status_word(now.state).to_variant(),
        (PLAYER_IFACE, "Rate") | (PLAYER_IFACE, "MinimumRate") | (PLAYER_IFACE, "MaximumRate") => 1.0f64.to_variant(),
        (PLAYER_IFACE, "Volume") => now.volume.to_variant(),
        (PLAYER_IFACE, "Position") => ((now.position * 1e6) as i64).to_variant(),
        (PLAYER_IFACE, "CanGoNext") | (PLAYER_IFACE, "CanGoPrevious") | (PLAYER_IFACE, "CanPlay") | (PLAYER_IFACE, "CanPause") | (PLAYER_IFACE, "CanControl") => true.to_variant(),
        (PLAYER_IFACE, "CanSeek") => seekable.to_variant(),
        (PLAYER_IFACE, "Metadata") => {
            let (title, artist) = now.shown();
            let station = now.station.as_ref();
            let dict = glib::VariantDict::new(None);
            let track = glib::variant::ObjectPath::try_from(format!("/io/github/whogotpwned/Omafile/radio/{}", now.episode)).ok()?;
            dict.insert_value("mpris:trackid", &track.to_variant());
            dict.insert_value("xesam:title", &title.to_variant());
            dict.insert_value("xesam:artist", &vec![artist].to_variant());
            // Under a song stands its station; under a station's bare name, what kind it is.
            let name = station.map(|s| s.name.clone()).unwrap_or_default();
            let album = if title == name { station.map(|s| crate::radioui::genre_title(&s.genre)).unwrap_or_default() } else { name };
            dict.insert_value("xesam:album", &album.to_variant());
            dict.insert_value("xesam:url", &station.map(|s| s.url.clone()).unwrap_or_default().to_variant());
            if seekable {
                dict.insert_value("mpris:length", &((now.duration * 1e6) as i64).to_variant());
            }
            if let Some(art) = art {
                dict.insert_value("mpris:artUrl", &art.to_variant());
            }
            dict.end()
        }
        _ => return None,
    })
}

impl RadioPlayer {
    fn art(now: &Now) -> Option<String> {
        now.station.as_ref().and_then(crate::radioui::art_file).map(|p| gio::File::for_path(p).uri().to_string())
    }

    /// Be a media player on the bus while there is something to control, and say what changed.
    fn serve(self: &Rc<Self>, now: &Now) {
        // A station that failed stays up, so its message can be read and Play tried again;
        // only Stop takes the player away.
        let wanted = now.station.is_some() && now.state != State::Stopped;
        let served = self.bus.borrow().is_some();
        if wanted && !served {
            self.own_name();
        } else if !wanted && served {
            if let Some(mut served) = self.bus.borrow_mut().take() {
                for id in served.objects.drain(..) {
                    let _ = served.connection.unregister_object(id);
                }
                if let Some(owner) = served.owner.take() {
                    gio::bus_unown_name(owner);
                }
            }
            return;
        }
        let Some(connection) = self.bus.borrow().as_ref().map(|s| s.connection.clone()) else { return };
        let art = Self::art(now);
        let changed = glib::VariantDict::new(None);
        for name in ["PlaybackStatus", "Metadata", "Volume", "CanSeek"] {
            if let Some(value) = property(now, PLAYER_IFACE, name, art.as_deref()) {
                changed.insert_value(name, &value);
            }
        }
        // Built by hand: a dictionary put into a Rust tuple would be wrapped once more.
        let body = glib::Variant::tuple_from_iter([PLAYER_IFACE.to_variant(), changed.end(), Vec::<String>::new().to_variant()]);
        let _ = connection.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", Some(&body));
    }

    fn seeked(&self, seconds: f64) {
        if let Some(served) = self.bus.borrow().as_ref() {
            let _ = served.connection.emit_signal(None, PATH, PLAYER_IFACE, "Seeked", Some(&(((seconds * 1e6) as i64),).to_variant()));
        }
    }

    fn own_name(self: &Rc<Self>) {
        let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
        let Ok(node) = gio::DBusNodeInfo::for_xml(XML) else { return };
        let mut objects = Vec::new();
        for interface in [ROOT, PLAYER_IFACE] {
            let Some(info) = node.lookup_interface(interface) else { continue };
            let (weak, weak2, weak3) = (Rc::downgrade(self), Rc::downgrade(self), Rc::downgrade(self));
            let registered = connection
                .register_object(PATH, &info)
                .method_call(move |_, _, _, _, method, params, invocation| {
                    if let Some(player) = weak.upgrade() {
                        player.called(method, &params);
                    }
                    invocation.return_value(None);
                })
                .property(move |_, _, _, interface, name| {
                    let now = weak2.upgrade().map(|p| p.now()).unwrap_or_default();
                    let art = Self::art(&now);
                    property(&now, interface, name, art.as_deref()).unwrap_or_else(|| "".to_variant())
                })
                .set_property(move |_, _, _, _, name, value| {
                    if let (Some(player), "Volume", Some(volume)) = (weak3.upgrade(), name, value.get::<f64>()) {
                        player.set_volume(volume);
                    }
                    true
                })
                .build();
            if let Ok(id) = registered {
                objects.push(id);
            }
        }
        let owner = gio::bus_own_name_on_connection(&connection, BUS_NAME, gio::BusNameOwnerFlags::NONE, |_, _| {}, |_, _| {});
        self.bus.replace(Some(Served { connection, owner: Some(owner), objects }));
    }

    /// A call from the desktop: the media keys, Omarchy's bar, omafile's own Now Playing.
    fn called(self: &Rc<Self>, method: &str, params: &glib::Variant) {
        match method {
            "Next" => self.skip(1),
            "Previous" => self.skip(-1),
            "Pause" => self.pause(),
            "PlayPause" => self.toggle(),
            "Play" => {
                if self.now.borrow().state != State::Playing {
                    self.toggle();
                }
            }
            "Stop" | "Quit" => self.stop(),
            "Seek" => {
                if let Some(offset) = params.try_child_value(0).and_then(|v| v.get::<i64>()) {
                    let position = self.now.borrow().position;
                    self.seek_to(position + offset as f64 / 1e6);
                }
            }
            "SetPosition" => {
                if let Some(position) = params.try_child_value(1).and_then(|v| v.get::<i64>()) {
                    self.seek_to(position as f64 / 1e6);
                }
            }
            "Raise" => {
                if let Some(app) = gio::Application::default() {
                    app.activate();
                }
            }
            // OpenUri: stations are chosen in omafile, not handed in from outside.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpv_is_started_quiet_and_on_a_leash() {
        let argv = mpv_argv(4242, "/run/user/1000/omafile-radio-4242.sock", "--ao=null  --volume=50");
        assert_eq!(&argv[..2], ["sh", "-c"]);
        assert!(argv[2].starts_with("mpv \"$@\" &") && argv[2].contains("kill -0 \"$0\""), "the shell ends mpv when omafile is gone");
        assert_eq!(argv[3], "4242");
        for flag in ["--idle=yes", "--no-video", "--load-scripts=no", "--ytdl=no", "--input-ipc-server=/run/user/1000/omafile-radio-4242.sock"] {
            assert!(argv.contains(&flag.to_string()), "{flag}");
        }
        assert_eq!(&argv[argv.len() - 2..], ["--ao=null", "--volume=50"]);
        assert_eq!(command(&[Json::s("loadfile"), Json::s("https://example.org/a \"b\".mp3"), Json::s("replace")]), "{\"command\":[\"loadfile\",\"https://example.org/a \\\"b\\\".mp3\",\"replace\"]}\n");
    }

    #[test]
    fn what_mpv_says_is_understood() {
        assert_eq!(understand(r#"{"event":"property-change","id":1,"name":"pause","data":true}"#), Said::Pause(true));
        assert_eq!(understand(r#"{"event":"property-change","id":2,"name":"metadata","data":{"icy-name":"Test FM","icy-title":"Aphex Twin - Xtal"}}"#), Said::Title("Aphex Twin - Xtal".into()));
        assert_eq!(understand(r#"{"event":"property-change","id":2,"name":"metadata","data":{"ARTIST":"Autechre","Title":"Bike"}}"#), Said::Title("Autechre - Bike".into()), "tags of an Ogg stream");
        assert_eq!(understand(r#"{"event":"property-change","id":2,"name":"metadata","data":{"icy-name":"Test FM"}}"#), Said::Nothing, "nothing said about the song");
        assert_eq!(understand(r#"{"event":"property-change","id":2,"name":"metadata"}"#), Said::Nothing);
        assert_eq!(understand(r#"{"data":12.5,"request_id":7,"error":"success"}"#), Said::Position(12.5));
        assert_eq!(understand(r#"{"request_id":7,"error":"property unavailable"}"#), Said::Nothing);
        assert_eq!(question(POSITION, &[Json::s("get_property"), Json::s("time-pos")]), "{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":7}\n");
        assert_eq!(understand(r#"{"event":"property-change","id":4,"name":"duration","data":3600.0}"#), Said::Duration(3600.0));
        assert_eq!(understand(r#"{"event":"property-change","id":5,"name":"volume","data":80.0}"#), Said::Volume(0.8));
        assert_eq!(understand(r#"{"event":"property-change","id":3,"name":"duration"}"#), Said::Nothing, "no data: nothing is loaded");
        assert_eq!(understand(r#"{"event":"file-loaded"}"#), Said::Loaded);
        assert_eq!(understand(r#"{"event":"end-file","reason":"error","file_error":"loading failed"}"#), Said::Ended(Some("loading failed".into())));
        assert_eq!(understand(r#"{"event":"end-file","reason":"eof"}"#), Said::Ended(None));
        assert_eq!(understand(r#"{"event":"end-file","reason":"stop"}"#), Said::Nothing, "stopped on purpose");
        assert_eq!(understand(r#"{"request_id":0,"error":"success"}"#), Said::Nothing);
        assert_eq!(understand("not json"), Said::Nothing);
    }

    #[test]
    fn the_desktop_sees_a_media_player() {
        let station = Station { kind: Kind::Live, genre: "Electronic".into(), name: "SomaFM Groove Salad".into(), url: "https://somafm.com/groovesalad.pls".into() };
        let mut now = Now { station: Some(station.clone()), state: State::Loading, ..Default::default() };
        // Before the station says what it plays: its name.
        assert_eq!(now.shown(), ("SomaFM Groove Salad".to_string(), String::new()));
        assert_eq!(status_word(now.state), "Playing", "about to play counts as playing");
        now.state = State::Playing;
        now.title = "Xtal".into();
        now.artist = "Aphex Twin".into();
        assert_eq!(now.shown(), ("Xtal".to_string(), "Aphex Twin".to_string()));
        assert!(now.is_on(&station.url) && !now.is_on("https://other.example.org/x"));
        let get = |n: &Now, name: &str| property(n, PLAYER_IFACE, name, Some("file:///tmp/art.png")).unwrap();
        assert_eq!(get(&now, "PlaybackStatus").get::<String>().unwrap(), "Playing");
        assert_eq!(get(&now, "CanSeek").get::<bool>(), Some(false), "a stream cannot be searched");
        let meta = glib::VariantDict::new(Some(&get(&now, "Metadata")));
        assert_eq!(meta.lookup::<String>("xesam:title").unwrap().as_deref(), Some("Xtal"));
        assert_eq!(meta.lookup::<Vec<String>>("xesam:artist").unwrap(), Some(vec!["Aphex Twin".to_string()]));
        assert_eq!(meta.lookup::<String>("xesam:album").unwrap().as_deref(), Some("SomaFM Groove Salad"));
        let silent = Now { station: now.station.clone(), state: State::Loading, ..Default::default() };
        assert_eq!(glib::VariantDict::new(Some(&get(&silent, "Metadata"))).lookup::<String>("xesam:album").unwrap().as_deref(), Some("Electronic"), "not the name twice");
        assert_eq!(meta.lookup::<String>("mpris:artUrl").unwrap().as_deref(), Some("file:///tmp/art.png"));
        assert!(meta.lookup_value("mpris:length", None).is_none());
        // An episode has a length and can be searched; a song without an artist shows the station.
        let cast = Now { station: Some(Station { kind: Kind::Podcast, name: "Risky Business".into(), ..station }), state: State::Paused, title: "Episode 800".into(), duration: 3600.0, position: 90.5, ..Default::default() };
        assert_eq!(cast.shown(), ("Episode 800".to_string(), "Risky Business".to_string()));
        assert_eq!(get(&cast, "PlaybackStatus").get::<String>().unwrap(), "Paused");
        assert_eq!(get(&cast, "CanSeek").get::<bool>(), Some(true));
        assert_eq!(get(&cast, "Position").get::<i64>(), Some(90_500_000));
        assert_eq!(glib::VariantDict::new(Some(&get(&cast, "Metadata"))).lookup::<i64>("mpris:length").unwrap(), Some(3_600_000_000));
        assert_eq!(property(&cast, ROOT, "Identity", None).unwrap().get::<String>().unwrap(), "omafile Radio");
        assert!(property(&cast, PLAYER_IFACE, "NoSuchThing", None).is_none());
        // Every property the interface declares has a value.
        for line in XML.lines().filter(|l| l.contains("<property ")) {
            let name = line.split("name=\"").nth(1).unwrap().split('"').next().unwrap();
            assert!(property(&cast, ROOT, name, None).or_else(|| property(&cast, PLAYER_IFACE, name, None)).is_some(), "{name}");
        }
    }

    #[test]
    fn a_feed_s_episodes_with_sound_are_found() {
        let rss = r#"<rss><channel><title>Cast</title>
          <item><title>Two</title><guid>2</guid><pubDate>Tue, 10 Nov 2026 10:00:00 GMT</pubDate><enclosure url="https://cdn.example.org/2.mp3" type="audio/mpeg"/></item>
          <item><title>Text only</title><guid>t</guid><pubDate>Mon, 09 Nov 2026 10:00:00 GMT</pubDate></item>
          <item><title>Local</title><guid>l</guid><enclosure url="file:///etc/passwd" type="audio/mpeg"/></item>
          <item><title>One</title><guid>1</guid><pubDate>Tue, 03 Nov 2026 10:00:00 GMT</pubDate><enclosure url="https://cdn.example.org/1.mp3" type="audio/mpeg"/></item>
        </channel></rss>"#;
        let episodes = episodes_of(rss);
        assert_eq!(episodes.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), vec!["Two", "One"], "newest first; only sound from the web");
        assert!(episodes_of("<html></html>").is_empty());
    }
}
