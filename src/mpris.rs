//! What is playing on this computer, through MPRIS on the session bus:
//! browsers (YouTube and friends), Spotify, mpv, any media player. This
//! module only talks D-Bus: it tracks the players, their metadata, status
//! and position, and sends the transport commands. No UI in here.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPS: &str = "org.freedesktop.DBus.Properties";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct Player {
    /// Well-known bus name, e.g. `org.mpris.MediaPlayer2.firefox.instance_1_42`.
    pub bus: String,
    pub owner: String,
    pub identity: String,
    pub desktop_entry: String,
    pub status: Status,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub track_id: String,
    /// Track length in microseconds, 0 when unknown (live streams).
    pub length_us: i64,
    position_us: i64,
    position_at: Instant,
    pub rate: f64,
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_control: bool,
    /// When this player last started playing; the most recent one is "active".
    pub played_at: Option<Instant>,
}

impl Player {
    fn new(bus: &str) -> Player {
        Player {
            bus: bus.to_string(),
            owner: String::new(),
            identity: pretty_name(bus),
            desktop_entry: String::new(),
            status: Status::Stopped,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art_url: String::new(),
            track_id: String::new(),
            length_us: 0,
            position_us: 0,
            position_at: Instant::now(),
            rate: 1.0,
            volume: None,
            shuffle: None,
            loop_status: None,
            can_next: false,
            can_previous: false,
            can_seek: false,
            can_control: true,
            played_at: None,
        }
    }

    /// Position right now: the last reported one, advanced by the clock while playing.
    pub fn position_now(&self) -> i64 {
        let mut pos = self.position_us;
        if self.status == Status::Playing {
            pos += (self.position_at.elapsed().as_micros() as f64 * self.rate) as i64;
        }
        if self.length_us > 0 {
            pos = pos.min(self.length_us);
        }
        pos.max(0)
    }

    pub fn fraction(&self) -> f64 {
        if self.length_us <= 0 {
            0.0
        } else {
            (self.position_now() as f64 / self.length_us as f64).clamp(0.0, 1.0)
        }
    }

    /// "Title" or the player's name when the track has none.
    pub fn display_title(&self) -> String {
        if self.title.is_empty() { self.identity.clone() } else { self.title.clone() }
    }

    /// "Artist · Album", whatever exists.
    pub fn display_subtitle(&self) -> String {
        [self.artist.as_str(), self.album.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("  ·  ")
    }
}

/// `org.mpris.MediaPlayer2.firefox.instance_1_23` → `Firefox`.
pub fn pretty_name(bus: &str) -> String {
    let short = bus.strip_prefix(PREFIX).unwrap_or(bus).split('.').next().unwrap_or("");
    let mut c = short.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Player".into(),
    }
}

/// `3:07`, `1:02:45`.
pub fn fmt_time(us: i64) -> String {
    let s = (us / 1_000_000).max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn status_from(s: &str) -> Status {
    match s {
        "Playing" => Status::Playing,
        "Paused" => Status::Paused,
        _ => Status::Stopped,
    }
}

fn as_i64(v: &glib::Variant) -> Option<i64> {
    v.get::<i64>().or_else(|| v.get::<u64>().map(|x| x as i64)).or_else(|| v.get::<i32>().map(i64::from)).or_else(|| v.get::<u32>().map(i64::from)).or_else(|| v.get::<f64>().map(|x| x as i64))
}

fn as_string_list(v: &glib::Variant) -> Vec<String> {
    if let Some(list) = v.get::<Vec<String>>() {
        list
    } else if let Some(s) = v.str() {
        vec![s.to_string()]
    } else {
        Vec::new()
    }
}

/// Apply one property of `org.mpris.MediaPlayer2.Player` (or the root interface) to `p`.
pub fn apply_property(p: &mut Player, key: &str, value: &glib::Variant) {
    match key {
        "PlaybackStatus" => {
            let new = status_from(value.str().unwrap_or(""));
            if new != p.status {
                // Freeze or restart the position clock at the change.
                p.position_us = p.position_now();
                p.position_at = Instant::now();
                if new == Status::Playing {
                    p.played_at = Some(Instant::now());
                }
                p.status = new;
            }
        }
        "Metadata" => {
            let dict = glib::VariantDict::new(Some(value));
            let text = |k: &str| dict.lookup_value(k, None).map(|v| as_string_list(&v).join(", ")).unwrap_or_default();
            let old_track = (p.track_id.clone(), p.title.clone());
            p.title = text("xesam:title");
            p.artist = text("xesam:artist");
            if p.artist.is_empty() {
                p.artist = text("xesam:albumArtist");
            }
            p.album = text("xesam:album");
            p.art_url = text("mpris:artUrl");
            p.track_id = dict.lookup_value("mpris:trackid", None).and_then(|v| v.str().map(String::from)).unwrap_or_default();
            p.length_us = dict.lookup_value("mpris:length", None).and_then(|v| as_i64(&v)).unwrap_or(0);
            if old_track != (p.track_id.clone(), p.title.clone()) {
                // A new track starts at zero unless the player says otherwise right after.
                p.position_us = 0;
                p.position_at = Instant::now();
            }
        }
        "Position" => {
            if let Some(pos) = as_i64(value) {
                p.position_us = pos;
                p.position_at = Instant::now();
            }
        }
        "Rate" => p.rate = value.get::<f64>().filter(|r| *r > 0.0).unwrap_or(1.0),
        "Volume" => p.volume = value.get::<f64>(),
        "Shuffle" => p.shuffle = value.get::<bool>(),
        "LoopStatus" => p.loop_status = value.str().map(String::from),
        "CanGoNext" => p.can_next = value.get::<bool>().unwrap_or(false),
        "CanGoPrevious" => p.can_previous = value.get::<bool>().unwrap_or(false),
        "CanSeek" => p.can_seek = value.get::<bool>().unwrap_or(false),
        "CanControl" => p.can_control = value.get::<bool>().unwrap_or(true),
        "Identity" => {
            if let Some(s) = value.str().filter(|s| !s.is_empty()) {
                p.identity = s.to_string();
            }
        }
        "DesktopEntry" => p.desktop_entry = value.str().unwrap_or("").to_string(),
        _ => {}
    }
}

fn apply_dict(p: &mut Player, dict: &glib::Variant) {
    for i in 0..dict.n_children() {
        let entry = dict.child_value(i);
        let key = entry.child_value(0);
        let value = entry.child_value(1);
        if let (Some(k), Some(v)) = (key.str(), value.as_variant()) {
            apply_property(p, k, &v);
        }
    }
}

/// The player to show: the chosen one if it still exists, else the one that
/// started playing last, else any playing one, else the first.
pub fn pick_active<'a>(players: &'a [Player], chosen: Option<&str>) -> Option<&'a Player> {
    if let Some(p) = chosen.and_then(|c| players.iter().find(|p| p.bus == c)) {
        return Some(p);
    }
    let playing = players.iter().filter(|p| p.status == Status::Playing).max_by_key(|p| p.played_at);
    playing.or_else(|| players.iter().filter(|p| !p.title.is_empty()).max_by_key(|p| p.played_at)).or_else(|| players.first())
}

pub struct Mpris {
    conn: gio::DBusConnection,
    players: RefCell<Vec<Player>>,
    chosen: RefCell<Option<String>>,
    on_change: RefCell<Option<Box<dyn Fn()>>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    notify_pending: Cell<bool>,
}

impl Mpris {
    /// Connect to the session bus and start tracking. `on_change` runs (coalesced) on every change.
    pub async fn start(on_change: Box<dyn Fn()>) -> Option<Rc<Mpris>> {
        let conn = gio::bus_get_future(gio::BusType::Session).await.ok()?;
        let this = Rc::new(Mpris { conn, players: RefCell::new(Vec::new()), chosen: RefCell::new(None), on_change: RefCell::new(Some(on_change)), subscriptions: RefCell::new(Vec::new()), notify_pending: Cell::new(false) });

        // Players coming and going.
        let weak = Rc::downgrade(&this);
        let sub = this.conn.subscribe_to_signal(Some("org.freedesktop.DBus"), Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), Some("/org/freedesktop/DBus"), Some(ROOT), gio::DBusSignalFlags::MATCH_ARG0_NAMESPACE, move |signal| {
            let Some(m) = weak.upgrade() else { return };
            let Some((name, _old, new)) = signal.parameters.get::<(String, String, String)>() else { return };
            if !name.starts_with(PREFIX) {
                return;
            }
            if new.is_empty() {
                m.players.borrow_mut().retain(|p| p.bus != name);
                m.notify();
            } else {
                m.clone().refresh(name);
            }
        });
        this.subscriptions.borrow_mut().push(sub);

        // Property changes of any player.
        let weak = Rc::downgrade(&this);
        let sub = this.conn.subscribe_to_signal(None, Some(PROPS), Some("PropertiesChanged"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |signal| {
            let Some(m) = weak.upgrade() else { return };
            let sender = signal.sender_name.to_string();
            let changed = signal.parameters.child_value(1);
            let known = {
                let mut players = m.players.borrow_mut();
                match players.iter_mut().find(|p| p.owner == sender) {
                    Some(p) => {
                        apply_dict(p, &changed);
                        true
                    }
                    None => false,
                }
            };
            if known { m.notify() } else { m.clone().rescan() }
        });
        this.subscriptions.borrow_mut().push(sub);

        // Seeks are not property changes.
        let weak = Rc::downgrade(&this);
        let sub = this.conn.subscribe_to_signal(None, Some(PLAYER), Some("Seeked"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |signal| {
            let Some(m) = weak.upgrade() else { return };
            let sender = signal.sender_name.to_string();
            if let Some(pos) = signal.parameters.child_value(0).get::<i64>() {
                if let Some(p) = m.players.borrow_mut().iter_mut().find(|p| p.owner == sender) {
                    p.position_us = pos;
                    p.position_at = Instant::now();
                }
                m.notify();
            }
        });
        this.subscriptions.borrow_mut().push(sub);

        this.clone().rescan();
        Some(this)
    }

    fn notify(self: &Rc<Self>) {
        // Coalesce bursts (a track change sends several signals).
        if self.notify_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(m) = weak.upgrade() {
                m.notify_pending.set(false);
                if let Some(f) = m.on_change.borrow().as_ref() {
                    f();
                }
            }
        });
    }

    /// Look for every player on the bus.
    pub fn rescan(self: Rc<Self>) {
        glib::spawn_future_local(async move {
            let reply = self.conn.call_future(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "ListNames", None, None, gio::DBusCallFlags::NONE, 2000).await;
            let Ok(reply) = reply else { return };
            let names: Vec<String> = reply.child_value(0).get::<Vec<String>>().unwrap_or_default().into_iter().filter(|n| n.starts_with(PREFIX)).collect();
            self.players.borrow_mut().retain(|p| names.contains(&p.bus));
            for name in names {
                self.clone().refresh(name);
            }
            self.notify();
        });
    }

    /// Read everything about one player.
    fn refresh(self: Rc<Self>, bus: String) {
        glib::spawn_future_local(async move {
            let owner = self.conn.call_future(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner", Some(&(bus.as_str(),).to_variant()), None, gio::DBusCallFlags::NONE, 2000).await.ok().and_then(|v| v.child_value(0).str().map(String::from)).unwrap_or_default();
            let mut player = self.players.borrow().iter().find(|p| p.bus == bus).cloned().unwrap_or_else(|| Player::new(&bus));
            player.owner = owner;
            for iface in [ROOT, PLAYER] {
                if let Ok(reply) = self.conn.call_future(Some(&bus), PATH, PROPS, "GetAll", Some(&(iface,).to_variant()), None, gio::DBusCallFlags::NONE, 2000).await {
                    apply_dict(&mut player, &reply.child_value(0));
                }
            }
            {
                let mut players = self.players.borrow_mut();
                match players.iter_mut().find(|p| p.bus == bus) {
                    Some(existing) => *existing = player,
                    None => players.push(player),
                }
            }
            self.notify();
        });
    }

    /// Ask the active player where it is (players do not signal the position while playing).
    pub fn sync_position(self: &Rc<Self>) {
        let Some(bus) = self.active().map(|p| p.bus) else { return };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let reply = this.conn.call_future(Some(&bus), PATH, PROPS, "Get", Some(&(PLAYER, "Position").to_variant()), None, gio::DBusCallFlags::NONE, 1500).await;
            if let Some(pos) = reply.ok().and_then(|v| v.child_value(0).as_variant()).and_then(|v| as_i64(&v)) {
                if let Some(p) = this.players.borrow_mut().iter_mut().find(|p| p.bus == bus) {
                    p.position_us = pos;
                    p.position_at = Instant::now();
                }
            }
        });
    }

    pub fn players(&self) -> Vec<Player> {
        self.players.borrow().clone()
    }

    pub fn active(&self) -> Option<Player> {
        pick_active(&self.players.borrow(), self.chosen.borrow().as_deref()).cloned()
    }

    pub fn choose(self: &Rc<Self>, bus: Option<String>) {
        self.chosen.replace(bus);
        self.notify();
    }

    fn call(&self, bus: &str, iface: &str, method: &str, params: Option<glib::Variant>) {
        self.conn.call(Some(bus), PATH, iface, method, params.as_ref(), None, gio::DBusCallFlags::NONE, 2000, gio::Cancellable::NONE, |_| {});
    }

    fn set(&self, bus: &str, property: &str, value: glib::Variant) {
        self.call(bus, PROPS, "Set", Some((PLAYER, property, value).to_variant()));
    }

    pub fn play_pause(&self) {
        if let Some(p) = self.active() {
            self.call(&p.bus, PLAYER, "PlayPause", None);
        }
    }

    pub fn next(&self) {
        if let Some(p) = self.active() {
            self.call(&p.bus, PLAYER, "Next", None);
        }
    }

    pub fn previous(&self) {
        if let Some(p) = self.active() {
            self.call(&p.bus, PLAYER, "Previous", None);
        }
    }

    /// Jump to an absolute position (microseconds).
    pub fn seek_to(self: &Rc<Self>, us: i64) {
        let Some(p) = self.active() else { return };
        if !p.can_seek {
            return;
        }
        let track = if p.track_id.is_empty() { "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string() } else { p.track_id.clone() };
        match glib::variant::ObjectPath::try_from(track) {
            Ok(path) => self.call(&p.bus, PLAYER, "SetPosition", Some((path, us).to_variant())),
            // Some players hand out odd track ids: fall back to a relative seek.
            Err(_) => self.call(&p.bus, PLAYER, "Seek", Some((us - p.position_now(),).to_variant())),
        }
        if let Some(pl) = self.players.borrow_mut().iter_mut().find(|x| x.bus == p.bus) {
            pl.position_us = us;
            pl.position_at = Instant::now();
        }
        self.notify();
    }

    pub fn seek_by(self: &Rc<Self>, delta_us: i64) {
        if let Some(p) = self.active() {
            let target = (p.position_now() + delta_us).max(0);
            self.seek_to(if p.length_us > 0 { target.min(p.length_us) } else { target });
        }
    }

    pub fn set_volume(&self, volume: f64) {
        if let Some(p) = self.active().filter(|p| p.volume.is_some()) {
            self.set(&p.bus, "Volume", volume.clamp(0.0, 1.0).to_variant());
        }
    }

    pub fn toggle_shuffle(&self) {
        if let Some((bus, on)) = self.active().and_then(|p| p.shuffle.map(|s| (p.bus, s))) {
            self.set(&bus, "Shuffle", (!on).to_variant());
        }
    }

    /// None → Playlist → Track → None.
    pub fn cycle_loop(&self) {
        if let Some((bus, current)) = self.active().and_then(|p| p.loop_status.clone().map(|s| (p.bus, s))) {
            let next = match current.as_str() {
                "None" => "Playlist",
                "Playlist" => "Track",
                _ => "None",
            };
            self.set(&bus, "LoopStatus", next.to_variant());
        }
    }

    /// Bring the player's window to the front.
    pub fn raise(&self) {
        if let Some(p) = self.active() {
            self.call(&p.bus, ROOT, "Raise", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(title: &str, length: i64) -> glib::Variant {
        let dict = glib::VariantDict::new(None);
        dict.insert_value("xesam:title", &title.to_variant());
        dict.insert_value("xesam:artist", &vec!["Nils Frahm".to_string(), "Ólafur Arnalds".to_string()].to_variant());
        dict.insert_value("xesam:album", &"Loon".to_variant());
        dict.insert_value("mpris:artUrl", &"file:///tmp/cover.png".to_variant());
        dict.insert_value("mpris:length", &length.to_variant());
        dict.end()
    }

    #[test]
    fn applies_properties() {
        let mut p = Player::new("org.mpris.MediaPlayer2.firefox.instance_1_42");
        assert_eq!(p.identity, "Firefox");
        apply_property(&mut p, "Metadata", &metadata("Four", 240_000_000));
        apply_property(&mut p, "PlaybackStatus", &"Playing".to_variant());
        apply_property(&mut p, "Position", &60_000_000i64.to_variant());
        apply_property(&mut p, "CanSeek", &true.to_variant());
        assert_eq!(p.title, "Four");
        assert_eq!(p.artist, "Nils Frahm, Ólafur Arnalds");
        assert_eq!(p.display_subtitle(), "Nils Frahm, Ólafur Arnalds  ·  Loon");
        assert_eq!(p.status, Status::Playing);
        assert!(p.played_at.is_some() && p.can_seek);
        assert!((p.fraction() - 0.25).abs() < 0.01);
        // A new track restarts the clock.
        apply_property(&mut p, "Metadata", &metadata("Loon", 100_000_000));
        assert!(p.position_now() < 1_000_000);
        // Pausing freezes it.
        apply_property(&mut p, "Position", &30_000_000i64.to_variant());
        apply_property(&mut p, "PlaybackStatus", &"Paused".to_variant());
        let frozen = p.position_now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(p.position_now(), frozen);
    }

    #[test]
    fn picks_the_player_that_started_last() {
        let mut a = Player::new("org.mpris.MediaPlayer2.spotify");
        let mut b = Player::new("org.mpris.MediaPlayer2.chromium.instance9");
        apply_property(&mut a, "PlaybackStatus", &"Playing".to_variant());
        std::thread::sleep(std::time::Duration::from_millis(5));
        apply_property(&mut b, "PlaybackStatus", &"Playing".to_variant());
        let players = vec![a.clone(), b.clone()];
        assert_eq!(pick_active(&players, None).unwrap().bus, b.bus);
        assert_eq!(pick_active(&players, Some(&a.bus)).unwrap().bus, a.bus);
        assert_eq!(pick_active(&players, Some("gone")).unwrap().bus, b.bus);
        assert!(pick_active(&[], None).is_none());
    }

    #[test]
    fn formats_times() {
        assert_eq!(fmt_time(187_000_000), "3:07");
        assert_eq!(fmt_time(3_765_000_000), "1:02:45");
        assert_eq!(fmt_time(-5), "0:00");
    }
}
