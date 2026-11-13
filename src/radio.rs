//! Internet radio: the stations.
//!
//! A station is a name, an address and a genre. The address is either a live stream (or a
//! `.pls` / `.m3u` playlist that names one) or a podcast's feed, of which the newest
//! episode is played: tech and security news come as podcasts far more often than as a
//! stream that runs all day. A handful of stations is there from the start; any other can
//! be added, and every one can be removed.
//!
//! The list is `~/.config/omafile/radio`, one station a line:
//! `kind<TAB>genre<TAB>name<TAB>address`. Until something is changed there is no file and
//! the list is the built-in one.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A stream that plays as long as one listens.
    Live,
    /// A feed with episodes; the newest one is played.
    Podcast,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Live => "live",
            Kind::Podcast => "podcast",
        }
    }

    pub fn from_id(id: &str) -> Kind {
        if id.trim() == "podcast" { Kind::Podcast } else { Kind::Live }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub kind: Kind,
    pub genre: String,
    pub name: String,
    pub url: String,
}

pub const ELECTRONIC: &str = "Electronic";
pub const NEWS: &str = "Tech and hacking news";
pub const OWN: &str = "My stations";

/// The stations omafile starts with. SomaFM is listener-supported radio without
/// commercials and publishes these playlists for players to use; the podcasts are the
/// public feeds of their makers. `scripts/check-radio.py` asks every one of these
/// addresses for sound, in CI, so a station that has moved is noticed.
pub const DEFAULTS: &[(Kind, &str, &str, &str)] = &[
    (Kind::Live, ELECTRONIC, "SomaFM Groove Salad", "https://somafm.com/groovesalad.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM DEF CON Radio", "https://somafm.com/defcon.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Space Station Soma", "https://somafm.com/spacestation.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Deep Space One", "https://somafm.com/deepspaceone.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Drone Zone", "https://somafm.com/dronezone.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Beat Blender", "https://somafm.com/beatblender.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM cliqhop idm", "https://somafm.com/cliqhop.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM The Trip", "https://somafm.com/thetrip.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Vaporwaves", "https://somafm.com/vaporwaves.pls"),
    (Kind::Live, ELECTRONIC, "SomaFM Dub Step Beyond", "https://somafm.com/dubstep.pls"),
    (Kind::Podcast, NEWS, "Security Now", "https://feeds.twit.tv/sn.xml"),
    (Kind::Podcast, NEWS, "Darknet Diaries", "https://feeds.megaphone.fm/darknetdiaries"),
    (Kind::Podcast, NEWS, "Risky Business", "https://risky.biz/feeds/risky-business/"),
    (Kind::Podcast, NEWS, "SANS Internet Stormcast", "https://isc.sans.edu/dailypodcast.xml"),
    (Kind::Podcast, NEWS, "Hacker Public Radio", "https://hackerpublicradio.org/hpr_rss.php"),
    (Kind::Podcast, NEWS, "Changelog News", "https://changelog.com/news/feed"),
    (Kind::Podcast, NEWS, "Logbuch:Netzpolitik", "https://feeds.metaebene.me/lnp/mp3"),
    (Kind::Podcast, NEWS, "Chaosradio", "https://chaosradio.de/feed/mp3"),
];

pub fn defaults() -> Vec<Station> {
    DEFAULTS.iter().map(|(kind, genre, name, url)| Station { kind: *kind, genre: genre.to_string(), name: name.to_string(), url: url.to_string() }).collect()
}

/// Only what a browser would open: nothing local, nothing another program would be
/// started for.
pub fn valid_url(url: &str) -> bool {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/') && !url.contains(char::is_whitespace) && !url.contains(char::is_control))
}

fn clean(field: &str) -> String {
    field.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

pub fn parse(text: &str) -> Vec<Station> {
    let mut out: Vec<Station> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [kind, genre, name, url] = fields[..] else { continue };
        let station = Station { kind: Kind::from_id(kind), genre: clean(genre), name: clean(name), url: url.trim().to_string() };
        if valid_url(&station.url) && !station.name.is_empty() && !out.iter().any(|s| s.url == station.url) {
            out.push(station);
        }
    }
    out
}

pub fn serialize(stations: &[Station]) -> String {
    let mut out = String::from("# omafile radio stations: kind, genre, name, address (tab separated)\n");
    for s in stations {
        out.push_str(&format!("{}\t{}\t{}\t{}\n", s.kind.id(), clean(&s.genre), clean(&s.name), s.url.trim()));
    }
    out
}

pub fn path() -> PathBuf {
    gtk::glib::user_config_dir().join("omafile/radio")
}

/// The stations: the saved list, or the built-in one while nothing was changed.
pub fn load() -> Vec<Station> {
    match std::fs::read_to_string(path()) {
        Ok(text) => parse(&text),
        Err(_) => defaults(),
    }
}

pub fn save(stations: &[Station]) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serialize(stations));
}

/// Add a station, or change the one that has this address already. Returns false when the
/// address is not one that can be played.
pub fn put(stations: &mut Vec<Station>, station: Station) -> bool {
    let station = Station { name: clean(&station.name), genre: if clean(&station.genre).is_empty() { OWN.to_string() } else { clean(&station.genre) }, url: station.url.trim().to_string(), kind: station.kind };
    if !valid_url(&station.url) || station.name.is_empty() {
        return false;
    }
    match stations.iter_mut().find(|s| s.url == station.url) {
        Some(existing) => *existing = station,
        None => stations.push(station),
    }
    true
}

/// The built-in stations that are missing from a list, appended. Returns how many came back.
pub fn restore_defaults(stations: &mut Vec<Station>) -> usize {
    let missing: Vec<Station> = defaults().into_iter().filter(|d| !stations.iter().any(|s| s.url == d.url)).collect();
    let n = missing.len();
    stations.extend(missing);
    n
}

/// The genres in the order they are shown: the built-in two first, then the others as
/// they first appear.
pub fn genres(stations: &[Station]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for genre in [ELECTRONIC, NEWS].iter().map(|g| g.to_string()).chain(stations.iter().map(|s| s.genre.clone())) {
        if !out.contains(&genre) && stations.iter().any(|s| s.genre == genre) {
            out.push(genre);
        }
    }
    out
}

/// The station `step` places after this one among those of the same kind (wrapping), for
/// the player's next and previous buttons.
pub fn neighbour(stations: &[Station], url: &str, step: i32) -> Option<Station> {
    let current = stations.iter().find(|s| s.url == url)?;
    let same: Vec<&Station> = stations.iter().filter(|s| s.kind == current.kind).collect();
    let at = same.iter().position(|s| s.url == url)? as i32;
    let n = same.len() as i32;
    Some(same[(at + step).rem_euclid(n) as usize].clone())
}

/// Whether what an address returned is a feed (then it is a podcast) rather than sound or
/// a playlist.
pub fn looks_like_feed(head: &str) -> bool {
    let head = head.trim_start_matches('\u{feff}').trim_start();
    let start: String = head.chars().take(600).collect::<String>().to_lowercase();
    start.starts_with("<?xml") && (start.contains("<rss") || start.contains("<feed") || start.contains("<rdf")) || start.starts_with("<rss") || start.starts_with("<feed")
}

/// What a stream says is playing, as artist and title: stations send "Artist - Title".
pub fn split_title(stream_title: &str) -> (String, String) {
    let text = stream_title.trim();
    match text.split_once(" - ") {
        Some((artist, title)) if !artist.trim().is_empty() && !title.trim().is_empty() => (artist.trim().to_string(), title.trim().to_string()),
        _ => (String::new(), text.to_string()),
    }
}

/// A name for a station from its address, for when none was typed.
pub fn name_from_url(url: &str) -> String {
    let rest = url.trim().trim_start_matches("https://").trim_start_matches("http://");
    let host = rest.split(['/', ':', '?']).next().unwrap_or("").trim_start_matches("www.");
    host.to_string()
}

/// A colour for a station's picture, the same every time: its hue, from its name.
pub fn hue(name: &str) -> f32 {
    let mut h: u32 = 2166136261;
    for b in name.to_lowercase().bytes() {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    (h % 360) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_stations_are_sound_and_news() {
        let all = defaults();
        assert!(all.len() >= 15);
        assert!(all.iter().all(|s| valid_url(&s.url) && s.url.starts_with("https://") && !s.name.is_empty()), "every address is https");
        let mut urls: Vec<&str> = all.iter().map(|s| s.url.as_str()).collect();
        urls.sort();
        urls.dedup();
        assert_eq!(urls.len(), all.len(), "no station twice");
        assert!(all.iter().filter(|s| s.genre == ELECTRONIC && s.kind == Kind::Live).count() >= 8);
        assert!(all.iter().filter(|s| s.genre == NEWS && s.kind == Kind::Podcast).count() >= 6);
        assert_eq!(genres(&all), vec![ELECTRONIC.to_string(), NEWS.to_string()]);
        // What is written is read back the same.
        assert_eq!(parse(&serialize(&all)), all);
    }

    #[test]
    fn only_web_addresses_are_stations() {
        for good in ["https://somafm.com/groovesalad.pls", "http://stream.example.org:8000/live", "https://example.org/feed?format=mp3"] {
            assert!(valid_url(good), "{good}");
        }
        for bad in ["", "file:///etc/passwd", "ytdl://x", "https://", "https:///x", "ftp://example.org/a.mp3", "https://exa mple.org/x", "javascript:alert(1)", "/home/me/a.mp3", "https://example.org/a\nb"] {
            assert!(!valid_url(bad), "{bad}");
        }
        let text = "live\tElectronic\tGood\thttps://example.org/a\nlive\tElectronic\tSame again\thttps://example.org/a\nlive\tX\tLocal\tfile:///etc/passwd\nbroken line\npodcast\tNews\t\thttps://example.org/noname\n# comment\npodcast\tNews\tCast\thttps://example.org/feed\n";
        let list = parse(text);
        assert_eq!(list.iter().map(|s| (s.name.as_str(), s.kind)).collect::<Vec<_>>(), vec![("Good", Kind::Live), ("Cast", Kind::Podcast)]);
    }

    #[test]
    fn stations_are_added_changed_and_brought_back() {
        let mut list = defaults();
        let n = list.len();
        assert!(put(&mut list, Station { kind: Kind::Live, genre: "  ".into(), name: " Night\tDrive ".into(), url: " https://radio.example.org/night ".into() }));
        assert_eq!(list.len(), n + 1);
        let added = list.last().unwrap();
        assert_eq!((added.name.as_str(), added.genre.as_str(), added.url.as_str()), ("Night Drive", OWN, "https://radio.example.org/night"));
        // The same address again changes that station instead of adding another.
        assert!(put(&mut list, Station { kind: Kind::Live, genre: "Synth".into(), name: "Nightride".into(), url: "https://radio.example.org/night".into() }));
        assert_eq!((list.len(), list.last().unwrap().name.as_str()), (n + 1, "Nightride"));
        assert!(!put(&mut list, Station { kind: Kind::Live, genre: "x".into(), name: "Bad".into(), url: "file:///x".into() }));
        assert!(!put(&mut list, Station { kind: Kind::Live, genre: "x".into(), name: "".into(), url: "https://example.org/x".into() }));
        assert_eq!(genres(&list), vec![ELECTRONIC.to_string(), NEWS.to_string(), "Synth".to_string()]);
        list.retain(|s| s.genre != ELECTRONIC);
        assert_eq!(genres(&list)[0], NEWS);
        let back = restore_defaults(&mut list);
        assert!(back >= 8 && list.iter().any(|s| s.name == "Nightride"), "the built-in ones return, the own one stays");
        assert_eq!(restore_defaults(&mut list), 0);
    }

    #[test]
    fn next_and_previous_stay_within_live_or_podcasts() {
        let list = defaults();
        let first = &list[0];
        let next = neighbour(&list, &first.url, 1).unwrap();
        assert_eq!(next, list[1]);
        let last_live = list.iter().rfind(|s| s.kind == Kind::Live).unwrap();
        assert_eq!(neighbour(&list, &first.url, -1).as_ref(), Some(last_live), "before the first comes the last");
        assert_eq!(neighbour(&list, &last_live.url, 1).as_ref(), Some(first));
        let cast = list.iter().find(|s| s.kind == Kind::Podcast).unwrap();
        assert_eq!(neighbour(&list, &cast.url, -1).unwrap().kind, Kind::Podcast);
        assert_eq!(neighbour(&list, "https://not.in/the/list", 1), None);
    }

    #[test]
    fn feeds_titles_and_names_are_recognised() {
        assert!(looks_like_feed("<?xml version=\"1.0\"?>\n<rss version=\"2.0\"><channel>"));
        assert!(looks_like_feed("\u{feff}<?xml version='1.0' encoding='UTF-8'?><feed xmlns='http://www.w3.org/2005/Atom'>"));
        assert!(looks_like_feed("<rss>"));
        assert!(!looks_like_feed("[playlist]\nFile1=https://ice1.example.org/x"));
        assert!(!looks_like_feed("#EXTM3U\nhttps://example.org/stream"));
        assert!(!looks_like_feed("<?xml version=\"1.0\"?><html>"));
        assert!(!looks_like_feed("ID3\u{3}\u{0}binary"));
        assert_eq!(split_title("Boards of Canada - Roygbiv"), ("Boards of Canada".to_string(), "Roygbiv".to_string()));
        assert_eq!(split_title("  Station ident  "), (String::new(), "Station ident".to_string()));
        assert_eq!(split_title("A - B - C"), ("A".to_string(), "B - C".to_string()));
        assert_eq!(split_title(" - "), (String::new(), "-".to_string()));
        assert_eq!(name_from_url("https://www.example.org:8443/live?x=1"), "example.org");
        assert_eq!(hue("SomaFM Groove Salad"), hue("somafm groove salad"));
        assert!((0.0..360.0).contains(&hue("x")));
    }
}
