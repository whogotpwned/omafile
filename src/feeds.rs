//! News feeds (RSS 2.0, RSS 1.0 / RDF, Atom) for the sidebar: the list of feeds, a
//! small XML reader, dates, and turning an article's HTML into text a label can show.
//! Nothing here touches the network or GTK; `feedsui` does that.

use std::path::PathBuf;

// ----- the list of feeds ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Feed {
    pub title: String,
    pub url: String,
}

fn registry() -> PathBuf {
    gtk::glib::user_config_dir().join("omafile/feeds")
}

/// `title<TAB>url` per line; blank lines and `#` comments are skipped, repeated urls dropped.
pub fn parse_list(text: &str) -> Vec<Feed> {
    let mut out: Vec<Feed> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let (title, url) = match line.split_once('\t') {
            Some((t, u)) => (t.trim(), u.trim()),
            None => ("", line.trim()),
        };
        if url.contains("://") && !out.iter().any(|f| f.url == url) {
            out.push(Feed { title: if title.is_empty() { host_of(url) } else { title.to_string() }, url: url.to_string() });
        }
    }
    out
}

pub fn list() -> Vec<Feed> {
    std::fs::read_to_string(registry()).map(|t| parse_list(&t)).unwrap_or_default()
}

fn save(feeds: &[Feed]) {
    let path = registry();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = feeds.iter().map(|f| format!("{}\t{}\n", f.title.replace(['\t', '\n'], " "), f.url)).collect();
    let _ = std::fs::write(path, text);
}

/// Add a feed or give the one with this address a new title; false when nothing changed.
pub fn add(title: &str, url: &str) -> bool {
    let mut feeds = list();
    match feeds.iter_mut().find(|f| f.url == url) {
        Some(f) if f.title == title => return false,
        Some(f) => f.title = title.to_string(),
        None => feeds.push(Feed { title: title.to_string(), url: url.to_string() }),
    }
    save(&feeds);
    true
}

pub fn remove(url: &str) {
    let feeds: Vec<Feed> = list().into_iter().filter(|f| f.url != url).collect();
    save(&feeds);
    let _ = std::fs::remove_file(cache_path(url));
}

pub fn host_of(url: &str) -> String {
    url.split_once("://").map(|(_, rest)| rest).unwrap_or(url).split('/').next().unwrap_or(url).trim_start_matches("www.").to_string()
}

/// FNV-1a, for file names and read marks: stable across runs, no crate needed.
pub fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Where the last downloaded copy of a feed is kept, so it can be read offline.
pub fn cache_path(url: &str) -> PathBuf {
    gtk::glib::user_cache_dir().join("omafile/feeds").join(format!("{}.xml", hash(url)))
}

pub fn cached(url: &str) -> Option<(String, Vec<Article>)> {
    let bytes = std::fs::read(cache_path(url)).ok()?;
    parse_feed(&String::from_utf8_lossy(&bytes))
}

// ----- what has been read ---------------------------------------------------------------

fn read_path() -> PathBuf {
    gtk::glib::user_state_dir().join("omafile/feeds-read")
}

/// The mark kept for a read article: a hash of feed address and article id.
pub fn mark_of(feed_url: &str, article_id: &str) -> String {
    hash(&format!("{feed_url}\n{article_id}"))
}

pub fn read_marks() -> std::collections::HashSet<String> {
    std::fs::read_to_string(read_path()).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

pub fn save_read_marks(marks: &std::collections::HashSet<String>) {
    let path = read_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut lines: Vec<&str> = marks.iter().map(String::as_str).collect();
    lines.sort_unstable();
    let _ = std::fs::write(path, lines.join("\n") + "\n");
}

// ----- a small XML reader ---------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Xml {
    /// Name, attributes, and whether the tag closes itself.
    Open(String, Vec<(String, String)>, bool),
    Close(String),
    Text(String),
}

/// Decode the character references that turn up in feeds and web pages.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(';').map(|e| e + 1).filter(|e| *e <= 12);
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "rsquo" => Some('’'),
            "lsquo" => Some('‘'),
            "rdquo" => Some('”'),
            "ldquo" => Some('“'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            "euro" => Some('€'),
            "copy" => Some('©'),
            "auml" => Some('ä'),
            "ouml" => Some('ö'),
            "uuml" => Some('ü'),
            "Auml" => Some('Ä'),
            "Ouml" => Some('Ö'),
            "Uuml" => Some('Ü'),
            "szlig" => Some('ß'),
            "eacute" => Some('é'),
            "egrave" => Some('è'),
            "agrave" => Some('à'),
            "ccedil" => Some('ç'),
            _ => name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")).and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let name_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let name = rest[..name_end].trim().to_string();
        rest = rest[name_end..].trim_start();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (value, tail) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => match after[1..].find(q) {
                    Some(e) => (&after[1..1 + e], &after[e + 2..]),
                    None => (&after[1..], ""),
                },
                _ => {
                    let e = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..e], &after[e..])
                }
            };
            if !name.is_empty() {
                out.push((name, decode_entities(value)));
            }
            rest = tail.trim_start();
        } else if !name.is_empty() {
            out.push((name, String::new()));
        } else {
            break;
        }
    }
    out
}

/// Cut markup into tags and text. Forgiving: it is used for feeds and for the HTML inside them.
fn tokens(text: &str) -> Vec<Xml> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Xml::Text(decode_entities(rest)));
            break;
        };
        if lt > 0 {
            out.push(Xml::Text(decode_entities(&rest[..lt])));
        }
        rest = &rest[lt..];
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").unwrap_or(body.len());
            out.push(Xml::Text(body[..end].to_string()));
            rest = body.get(end + 3..).unwrap_or("");
        } else if let Some(body) = rest.strip_prefix("<!--") {
            rest = body.find("-->").map(|e| &body[e + 3..]).unwrap_or("");
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.find('>').map(|e| &rest[e + 1..]).unwrap_or("");
        } else if !rest[1..].starts_with(|c: char| c.is_alphabetic() || c == '/') {
            // A stray "<" in running text ("1 < 2").
            out.push(Xml::Text("<".into()));
            rest = &rest[1..];
        } else {
            let Some(gt) = rest.find('>') else { break };
            let inner = &rest[1..gt];
            rest = &rest[gt + 1..];
            if let Some(name) = inner.strip_prefix('/') {
                out.push(Xml::Close(name.trim().to_lowercase()));
            } else {
                let closed = inner.ends_with('/');
                let inner = inner.trim_end_matches('/');
                let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
                let name = inner[..name_end].to_lowercase();
                out.push(Xml::Open(name, parse_attributes(&inner[name_end..]), closed));
            }
        }
    }
    out
}

fn attr<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

// ----- feeds ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Article {
    /// What identifies it within its feed (guid, id, or the link).
    pub id: String,
    pub title: String,
    pub link: String,
    /// Seconds since 1970, 0 when the feed gives no date.
    pub date: i64,
    /// The article as the feed carries it: HTML.
    pub html: String,
    pub image: Option<String>,
    pub author: String,
    /// The sound that comes with it: a podcast episode's file.
    pub audio: Option<String>,
}

/// Whether an enclosure is sound: by its type, or by its ending when the feed names none.
fn is_audio(kind: Option<&str>, url: &str) -> bool {
    match kind {
        Some(t) if !t.trim().is_empty() => t.starts_with("audio/"),
        _ => {
            let path = url.split(['?', '#']).next().unwrap_or("").to_lowercase();
            [".mp3", ".m4a", ".aac", ".ogg", ".oga", ".opus", ".flac"].iter().any(|e| path.ends_with(e))
        }
    }
}

/// Read a feed: its title and its articles, newest first. None when this is not a feed.
pub fn parse_feed(xml: &str) -> Option<(String, Vec<Article>)> {
    let toks = tokens(xml);
    let is_feed = toks.iter().any(|t| matches!(t, Xml::Open(n, _, _) if n == "rss" || n == "feed" || n == "rdf:rdf"));
    if !is_feed {
        return None;
    }
    let mut feed_title = String::new();
    let mut articles: Vec<Article> = Vec::new();
    let mut current: Option<Article> = None;
    // Which element's text is being collected, and the text so far.
    let mut field: Option<(String, String)> = None;
    let mut atom_content = false;
    let mut depth_in_content = 0usize;
    let mut encoded = false;
    for tok in toks {
        // Atom may carry XHTML as real child elements: put them back together as markup.
        if atom_content {
            match &tok {
                Xml::Close(n) if n == "content" && depth_in_content == 0 => atom_content = false,
                Xml::Open(n, attrs, closed) => {
                    if let Some((_, text)) = field.as_mut() {
                        let attributes: String = attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", v.replace('"', "&quot;"))).collect();
                        text.push_str(&format!("<{n}{attributes}{}>", if *closed { "/" } else { "" }));
                    }
                    if !closed {
                        depth_in_content += 1;
                    }
                    continue;
                }
                Xml::Close(n) => {
                    if let Some((_, text)) = field.as_mut() {
                        text.push_str(&format!("</{n}>"));
                    }
                    depth_in_content = depth_in_content.saturating_sub(1);
                    continue;
                }
                Xml::Text(t) => {
                    if let Some((_, text)) = field.as_mut() {
                        text.push_str(t);
                    }
                    continue;
                }
            }
        }
        match tok {
            Xml::Open(name, attrs, closed) => match name.as_str() {
                "item" | "entry" => current = Some(Article::default()),
                "link" => {
                    // Atom: <link href="…" rel="alternate">; RSS: <link>text</link>.
                    if let Some(href) = attr(&attrs, "href") {
                        let rel = attr(&attrs, "rel").unwrap_or("alternate");
                        if let Some(a) = current.as_mut() {
                            if rel == "alternate" || a.link.is_empty() {
                                a.link = href.to_string();
                            }
                            if rel == "enclosure" && attr(&attrs, "type").map(|t| t.starts_with("image/")).unwrap_or(false) && a.image.is_none() {
                                a.image = Some(href.to_string());
                            }
                            if rel == "enclosure" && is_audio(attr(&attrs, "type"), href) && a.audio.is_none() {
                                a.audio = Some(href.to_string());
                            }
                        }
                    } else if !closed {
                        field = Some((name, String::new()));
                    }
                }
                "enclosure" | "media:content" | "media:thumbnail" => {
                    let is_image = name == "media:thumbnail" || attr(&attrs, "type").map(|t| t.starts_with("image/")).unwrap_or(false) || attr(&attrs, "medium") == Some("image");
                    if let (Some(a), Some(url), true) = (current.as_mut(), attr(&attrs, "url"), is_image) {
                        if a.image.is_none() {
                            a.image = Some(url.to_string());
                        }
                    }
                    if let (Some(a), Some(url)) = (current.as_mut(), attr(&attrs, "url")) {
                        if name != "media:thumbnail" && !is_image && a.audio.is_none() && (is_audio(attr(&attrs, "type"), url) || attr(&attrs, "medium") == Some("audio")) {
                            a.audio = Some(url.to_string());
                        }
                    }
                }
                "content" if !closed => {
                    atom_content = true;
                    depth_in_content = 0;
                    field = Some((name, String::new()));
                }
                "title" | "guid" | "id" | "pubdate" | "dc:date" | "updated" | "published" | "description" | "summary" | "content:encoded" | "dc:creator" | "name" if !closed => field = Some((name, String::new())),
                _ => {}
            },
            Xml::Text(text) => {
                if let Some((_, collected)) = field.as_mut() {
                    collected.push_str(&text);
                }
            }
            Xml::Close(name) => {
                if let Some((open, text)) = field.take() {
                    if open != name {
                        // Not ours (a nested element closed): keep collecting.
                        field = Some((open, text));
                    } else {
                        let text = text.trim().to_string();
                        match (current.as_mut(), open.as_str()) {
                            (Some(a), "title") => a.title = strip_tags(&text),
                            (Some(a), "link") => a.link = text,
                            (Some(a), "guid" | "id") => a.id = text,
                            (Some(a), "pubdate" | "dc:date" | "updated" | "published") => {
                                let parsed = parse_date(&text);
                                // "published" beats "updated" when both are given.
                                if parsed != 0 && (a.date == 0 || open == "published" || open == "pubdate") {
                                    a.date = parsed;
                                }
                            }
                            (Some(a), "content:encoded") => {
                                a.html = text;
                                encoded = true;
                            }
                            (Some(a), "content") => a.html = text,
                            (Some(a), "description" | "summary") => {
                                // The full text wins over the teaser, whichever comes first.
                                if a.html.is_empty() || (!encoded && text.len() > a.html.len() && open == "description") {
                                    a.html = text;
                                }
                            }
                            (Some(a), "dc:creator" | "name") => {
                                if a.author.is_empty() {
                                    a.author = strip_tags(&text);
                                }
                            }
                            (None, "title") if feed_title.is_empty() => feed_title = strip_tags(&text),
                            _ => {}
                        }
                    }
                }
                if name == "item" || name == "entry" {
                    encoded = false;
                    if let Some(mut a) = current.take() {
                        if a.id.is_empty() {
                            a.id = if a.link.is_empty() { a.title.clone() } else { a.link.clone() };
                        }
                        if a.image.is_none() {
                            a.image = first_image(&a.html);
                        }
                        if !a.title.is_empty() || !a.html.is_empty() {
                            articles.push(a);
                        }
                    }
                }
            }
        }
    }
    articles.sort_by_key(|a| std::cmp::Reverse(a.date));
    Some((feed_title, articles))
}

/// The address of a feed a web page points to (`<link rel="alternate" type="application/rss+xml">`).
pub fn discover(html: &str, base: &str) -> Option<String> {
    for tok in tokens(html) {
        if let Xml::Open(name, attrs, _) = tok {
            if name == "link" {
                let kind = attr(&attrs, "type").unwrap_or("").to_lowercase();
                if kind.contains("rss+xml") || kind.contains("atom+xml") {
                    if let Some(href) = attr(&attrs, "href").filter(|h| !h.is_empty()) {
                        return Some(resolve(base, href));
                    }
                }
            } else if name == "body" {
                break;
            }
        }
    }
    None
}

/// Make a link from a page absolute.
pub fn resolve(base: &str, href: &str) -> String {
    if href.contains("://") {
        return href.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    let host = rest.split('/').next().unwrap_or(rest);
    if let Some(h) = href.strip_prefix("//") {
        return format!("{scheme}://{h}");
    }
    if href.starts_with('/') {
        return format!("{scheme}://{host}{href}");
    }
    let dir = match rest.rfind('/') {
        Some(i) => &rest[..i],
        None => rest,
    };
    format!("{scheme}://{dir}/{href}")
}

// ----- dates ----------------------------------------------------------------------------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// "2026-11-11 12:00:00" in UTC for a time in seconds since 1970, as snapper writes dates.
pub fn utc_stamp(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86400));
    let s = unix.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

fn zone_offset(zone: &str) -> i64 {
    let zone = zone.trim();
    let sign = match zone.chars().next() {
        Some('+') => 1,
        Some('-') => -1,
        _ => {
            return match zone.to_ascii_uppercase().as_str() {
                "EST" => -5 * 3600,
                "EDT" => -4 * 3600,
                "CST" => -6 * 3600,
                "CDT" => -5 * 3600,
                "MST" => -7 * 3600,
                "MDT" => -6 * 3600,
                "PST" => -8 * 3600,
                "PDT" => -7 * 3600,
                "CET" => 3600,
                "CEST" => 7200,
                _ => 0,
            }
        }
    };
    let digits: String = zone[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return 0;
    }
    let (h, m) = (digits[..2].parse::<i64>().unwrap_or(0), digits[2..4].parse::<i64>().unwrap_or(0));
    sign * (h * 3600 + m * 60)
}

/// Seconds since 1970 for the two date forms feeds use: RFC 822 ("Tue, 10 Nov 2026 14:30:00
/// +0100") and RFC 3339 ("2026-11-10T14:30:00Z"). 0 when it cannot be read.
pub fn parse_date(text: &str) -> i64 {
    let text = text.trim();
    let clock = |t: &str| -> Option<i64> {
        let mut parts = t.split(':');
        let h = parts.next()?.trim().parse::<i64>().ok()?;
        let m = parts.next()?.trim().parse::<i64>().ok()?;
        let s = parts.next().and_then(|s| s.split('.').next()).and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
        Some(h * 3600 + m * 60 + s)
    };
    // RFC 3339
    if text.len() >= 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-' {
        let (y, m, d) = (text[..4].parse::<i64>().ok(), text[5..7].parse::<i64>().ok(), text[8..10].parse::<i64>().ok());
        let (Some(y), Some(m), Some(d)) = (y, m, d) else { return 0 };
        let mut seconds = days_from_civil(y, m, d) * 86400;
        if let Some(time) = text.get(11..) {
            let zone_at = time.find(['Z', 'z', '+', '-']).unwrap_or(time.len());
            seconds += clock(&time[..zone_at]).unwrap_or(0);
            let zone = &time[zone_at..];
            if !zone.is_empty() && !zone.eq_ignore_ascii_case("z") {
                seconds -= zone_offset(zone);
            }
        }
        return seconds;
    }
    // RFC 822: optional weekday, day, month name, year, time, zone.
    let body = text.split_once(',').map(|(_, r)| r).unwrap_or(text);
    let mut words = body.split_whitespace();
    let (Some(d), Some(mon), Some(y)) = (words.next().and_then(|w| w.parse::<i64>().ok()), words.next(), words.next().and_then(|w| w.parse::<i64>().ok())) else { return 0 };
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let Some(m) = months.iter().position(|n| mon.to_lowercase().starts_with(n)) else { return 0 };
    let y = if y < 100 { y + 2000 } else { y };
    let time = words.next().and_then(clock).unwrap_or(0);
    let zone = words.next().map(zone_offset).unwrap_or(0);
    days_from_civil(y, m as i64 + 1, d) * 86400 + time - zone
}

/// "14:30" for today, "Mon 14:30" within a week, "10 Nov" this year, "10 Nov 2025" before.
/// `now` and `offset` (seconds east of UTC) are passed in so this can be tested.
pub fn friendly_date(date: i64, now: i64, offset: i64) -> String {
    if date <= 0 {
        return String::new();
    }
    let (local, local_now) = (date + offset, now + offset);
    let (day, today) = (local.div_euclid(86400), local_now.div_euclid(86400));
    let (h, m) = (local.rem_euclid(86400) / 3600, local.rem_euclid(3600) / 60);
    let (y, mon, d) = civil_from_days(day);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if day == today {
        format!("{h:02}:{m:02}")
    } else if today - day < 7 && today > day {
        let weekdays = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        format!("{} {h:02}:{m:02}", weekdays[day.rem_euclid(7) as usize])
    } else if y == civil_from_days(today).0 {
        format!("{d} {}", months[(mon - 1) as usize])
    } else {
        format!("{d} {} {y}", months[(mon - 1) as usize])
    }
}

// ----- HTML for reading -----------------------------------------------------------------

/// Text without any markup, white space folded: for titles and the teaser under them.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut skip = 0;
    for tok in tokens(html) {
        match tok {
            Xml::Open(n, _, closed) if (n == "script" || n == "style") && !closed => skip += 1,
            Xml::Close(n) if n == "script" || n == "style" => skip = (skip as i32 - 1).max(0) as usize,
            Xml::Text(t) if skip == 0 => {
                out.push_str(&t);
                out.push(' ');
            }
            Xml::Open(n, _, _) | Xml::Close(n) if skip == 0 && matches!(n.as_str(), "p" | "br" | "div" | "li" | "h1" | "h2" | "h3" | "tr") => out.push(' '),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn teaser(html: &str, max: usize) -> String {
    let text = strip_tags(html);
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.rsplit_once(' ').map(|(a, _)| a).unwrap_or(&cut))
}

pub fn first_image(html: &str) -> Option<String> {
    tokens(html).into_iter().find_map(|t| match t {
        Xml::Open(n, attrs, _) if n == "img" => attr(&attrs, "src").filter(|s| s.starts_with("http")).map(str::to_string),
        _ => None,
    })
}

/// Markup with its `<a href>` tags taken out, the text between them kept.
pub fn without_links(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(i) = rest.find("<a href=\"") {
        out.push_str(&rest[..i]);
        rest = match rest[i..].find('>') {
            Some(end) => &rest[i + end + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out.replace("</a>", "")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// An article's HTML as Pango markup: paragraphs, headings, emphasis, links, lists, quotes
/// and code survive; scripts, styles, forms and pictures do not.
pub fn to_markup(html: &str, base: &str) -> String {
    let mut out = String::new();
    // Tags opened in the markup, to close them in order whatever the HTML does.
    let mut open: Vec<&'static str> = Vec::new();
    let mut skip = 0usize;
    let mut pre = 0usize;
    let mut list: Vec<Option<u32>> = Vec::new();
    let mut quote = 0usize;
    let close_all = |out: &mut String, open: &mut Vec<&'static str>| {
        while let Some(tag) = open.pop() {
            out.push_str(&format!("</{tag}>"));
        }
    };
    let paragraph = |out: &mut String, open: &mut Vec<&'static str>| {
        while let Some(tag) = open.pop() {
            out.push_str(&format!("</{tag}>"));
        }
        let trimmed = out.trim_end_matches([' ', '\t']).len();
        out.truncate(trimmed);
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
        }
    };
    for tok in tokens(html) {
        match tok {
            Xml::Open(name, attrs, closed) => {
                if matches!(name.as_str(), "script" | "style" | "form" | "iframe" | "svg" | "noscript" | "head" | "figcaption") {
                    if !closed {
                        skip += 1;
                    }
                    continue;
                }
                if skip > 0 {
                    continue;
                }
                match name.as_str() {
                    "p" | "div" | "section" | "article" | "table" | "tr" | "figure" | "ul" | "ol" | "blockquote" | "pre" | "hr" => {
                        paragraph(&mut out, &mut open);
                        match name.as_str() {
                            "ul" => list.push(None),
                            "ol" => list.push(Some(1)),
                            "blockquote" => quote += 1,
                            "pre" => {
                                pre += 1;
                                out.push_str("<tt>");
                                open.push("tt");
                            }
                            "hr" => out.push_str("———\n\n"),
                            _ => {}
                        }
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        paragraph(&mut out, &mut open);
                        out.push_str(if name == "h1" || name == "h2" { "<span size=\"large\"><b>" } else { "<span><b>" });
                        open.push("span");
                        open.push("b");
                    }
                    "br" => out.push('\n'),
                    "li" => {
                        close_all(&mut out, &mut open);
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str(&"    ".repeat(list.len().saturating_sub(1)));
                        match list.last_mut() {
                            Some(Some(n)) => {
                                out.push_str(&format!("{n}. "));
                                *n += 1;
                            }
                            _ => out.push_str("•  "),
                        }
                    }
                    "b" | "strong" => {
                        out.push_str("<b>");
                        open.push("b");
                    }
                    "i" | "em" | "cite" => {
                        out.push_str("<i>");
                        open.push("i");
                    }
                    "code" | "kbd" | "samp" if pre == 0 => {
                        out.push_str("<tt>");
                        open.push("tt");
                    }
                    "a" => {
                        if let Some(href) = attr(&attrs, "href").filter(|h| !h.starts_with('#') && !h.to_lowercase().starts_with("javascript:")) {
                            out.push_str(&format!("<a href=\"{}\">", escape(&resolve(base, href)).replace('"', "&quot;")));
                            open.push("a");
                        }
                    }
                    "td" | "th" => out.push_str("  "),
                    _ => {}
                }
            }
            Xml::Close(name) => {
                if matches!(name.as_str(), "script" | "style" | "form" | "iframe" | "svg" | "noscript" | "head" | "figcaption") {
                    skip = skip.saturating_sub(1);
                    continue;
                }
                if skip > 0 {
                    continue;
                }
                let markup = match name.as_str() {
                    "b" | "strong" => Some("b"),
                    "i" | "em" | "cite" => Some("i"),
                    "code" | "kbd" | "samp" if pre == 0 => Some("tt"),
                    "a" => Some("a"),
                    _ => None,
                };
                if let Some(tag) = markup {
                    // Close up to and including the matching tag, so the markup stays well formed.
                    if let Some(at) = open.iter().rposition(|t| *t == tag) {
                        while open.len() > at {
                            out.push_str(&format!("</{}>", open.pop().unwrap()));
                        }
                    }
                    continue;
                }
                match name.as_str() {
                    "p" | "div" | "section" | "article" | "table" | "tr" | "figure" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => paragraph(&mut out, &mut open),
                    "ul" | "ol" => {
                        list.pop();
                        paragraph(&mut out, &mut open);
                    }
                    "blockquote" => {
                        quote = quote.saturating_sub(1);
                        paragraph(&mut out, &mut open);
                    }
                    "pre" => {
                        pre = pre.saturating_sub(1);
                        paragraph(&mut out, &mut open);
                    }
                    "li" => {
                        close_all(&mut out, &mut open);
                        out.push('\n');
                    }
                    _ => {}
                }
            }
            Xml::Text(text) => {
                if skip > 0 {
                    continue;
                }
                if pre > 0 {
                    out.push_str(&escape(&text));
                    continue;
                }
                // White space is folded like a browser does.
                let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if folded.is_empty() {
                    if !out.is_empty() && !out.ends_with([' ', '\n']) {
                        out.push(' ');
                    }
                    continue;
                }
                let at_line_start = out.is_empty() || out.ends_with('\n');
                if at_line_start && quote > 0 {
                    out.push_str(&"▎ ".repeat(quote));
                }
                if text.starts_with(char::is_whitespace) && !at_line_start && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(&escape(&folded));
                if text.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
        }
    }
    close_all(&mut out, &mut open);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podcast_episodes_carry_their_sound() {
        let rss = r#"<rss><channel><title>Risky Talk</title>
            <item><title>Episode 2</title><guid>2</guid><pubDate>Tue, 10 Nov 2026 10:00:00 GMT</pubDate>
              <enclosure url="https://cdn.example.org/ep2.mp3?x=1" length="123" type="audio/mpeg"/>
              <media:thumbnail url="https://cdn.example.org/ep2.jpg"/></item>
            <item><title>Episode 1</title><guid>1</guid><pubDate>Tue, 03 Nov 2026 10:00:00 GMT</pubDate>
              <enclosure url="https://cdn.example.org/ep1.m4a"/></item>
            <item><title>Only a picture</title><guid>0</guid><enclosure url="https://cdn.example.org/p.png" type="image/png"/></item>
          </channel></rss>"#;
        let (title, items) = parse_feed(rss).unwrap();
        assert_eq!(title, "Risky Talk");
        assert_eq!(items[0].audio.as_deref(), Some("https://cdn.example.org/ep2.mp3?x=1"));
        assert_eq!(items[0].image.as_deref(), Some("https://cdn.example.org/ep2.jpg"));
        assert_eq!(items[1].audio.as_deref(), Some("https://cdn.example.org/ep1.m4a"), "no type: the ending says it is sound");
        assert!(items.iter().find(|a| a.title == "Only a picture").unwrap().audio.is_none());
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Cast</title><entry><title>One</title><id>a</id>
            <link rel="alternate" href="https://example.org/one"/><link rel="enclosure" type="audio/ogg" href="https://example.org/one.ogg"/></entry></feed>"#;
        assert_eq!(parse_feed(atom).unwrap().1[0].audio.as_deref(), Some("https://example.org/one.ogg"));
    }

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:media="http://search.yahoo.com/mrss/">
<channel>
  <title>Omarchy &amp; Friends</title>
  <link>https://example.org/</link>
  <description>News</description>
  <image><title>logo title</title><url>https://example.org/logo.png</url></image>
  <item>
    <title><![CDATA[First <em>post</em>]]></title>
    <link>https://example.org/first</link>
    <guid isPermaLink="false">post-1</guid>
    <pubDate>Tue, 10 Nov 2026 14:30:00 +0100</pubDate>
    <dc:creator>Nora</dc:creator>
    <description>Short teaser.</description>
    <content:encoded><![CDATA[<p>Hello <b>world</b>.</p><img src="https://example.org/a.jpg">]]></content:encoded>
  </item>
  <item>
    <title>Second</title>
    <link>https://example.org/second</link>
    <pubDate>Wed, 11 Nov 2026 08:00:00 GMT</pubDate>
    <description>&lt;p&gt;Escaped &amp;amp; fine&lt;/p&gt;</description>
    <media:thumbnail url="https://example.org/thumb.png"/>
  </item>
</channel>
</rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Feed</title>
  <link href="https://example.com/feed.xml" rel="self"/>
  <link href="https://example.com/"/>
  <entry>
    <title>Entry one</title>
    <link rel="alternate" href="https://example.com/one"/>
    <id>urn:uuid:1</id>
    <updated>2026-11-10T12:00:00Z</updated>
    <published>2026-11-09T10:00:00+02:00</published>
    <author><name>Ada</name></author>
    <summary>Summary text</summary>
    <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Real <a href="/x">markup</a></p></div></content>
  </entry>
</feed>"#;

    #[test]
    fn reads_rss() {
        let (title, items) = parse_feed(RSS).unwrap();
        assert_eq!(title, "Omarchy & Friends", "the channel's title, not the logo's");
        assert_eq!(items.len(), 2);
        // Newest first.
        assert_eq!(items[0].title, "Second");
        assert_eq!(items[0].html, "<p>Escaped &amp; fine</p>");
        assert_eq!(items[0].image.as_deref(), Some("https://example.org/thumb.png"));
        assert_eq!(items[0].id, "https://example.org/second", "no guid: the link identifies it");
        let first = &items[1];
        assert_eq!(first.title, "First post");
        assert_eq!(first.id, "post-1");
        assert_eq!(first.author, "Nora");
        assert_eq!(first.html, "<p>Hello <b>world</b>.</p><img src=\"https://example.org/a.jpg\">", "content:encoded beats the teaser");
        assert_eq!(first.image.as_deref(), Some("https://example.org/a.jpg"));
        assert_eq!(first.date, parse_date("2026-11-10T13:30:00Z"));
    }

    #[test]
    fn reads_atom() {
        let (title, items) = parse_feed(ATOM).unwrap();
        assert_eq!(title, "Atom Feed");
        assert_eq!(items.len(), 1);
        let e = &items[0];
        assert_eq!((e.title.as_str(), e.link.as_str(), e.id.as_str(), e.author.as_str()), ("Entry one", "https://example.com/one", "urn:uuid:1", "Ada"));
        assert_eq!(e.date, parse_date("2026-11-09T08:00:00Z"), "published wins over updated");
        assert!(e.html.contains("<p>Real <a href=\"/x\">markup</a></p>"), "{}", e.html);
        assert!(parse_feed("<html><body>no feed</body></html>").is_none());
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("Thu, 01 Jan 1970 00:00:00 GMT"), 0);
        assert_eq!(parse_date("1970-01-02T00:00:00Z"), 86400);
        assert_eq!(parse_date("Tue, 10 Nov 2026 14:30:00 +0100"), parse_date("2026-11-10T13:30:00Z"));
        assert_eq!(parse_date("10 Nov 2026 08:30 EST"), parse_date("2026-11-10T13:30:00Z"));
        assert_eq!(parse_date("2026-11-10T14:30:00.123+01:00"), parse_date("2026-11-10T13:30:00Z"));
        assert_eq!(parse_date("2026-11-10"), parse_date("2026-11-10T00:00:00Z"));
        assert_eq!(parse_date("yesterday"), 0);
        assert_eq!(utc_stamp(parse_date("2026-11-10T13:30:05Z")), "2026-11-10 13:30:05");
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00");
        let now = parse_date("2026-11-11T12:00:00Z");
        assert_eq!(friendly_date(parse_date("2026-11-11T08:05:00Z"), now, 3600), "09:05");
        assert_eq!(friendly_date(parse_date("2026-11-09T08:05:00Z"), now, 0), "Mon 08:05");
        assert_eq!(friendly_date(parse_date("2026-03-02T08:05:00Z"), now, 0), "2 Mar");
        assert_eq!(friendly_date(parse_date("2025-12-24T08:05:00Z"), now, 0), "24 Dec 2025");
        assert_eq!(friendly_date(0, now, 0), "");
    }

    #[test]
    fn html_becomes_markup() {
        let html = "<h2>Title</h2><p>One <b>bold <i>both</i></b> &amp; <a href=\"/rel\">a link</a>.</p><script>alert(1)</script><ul><li>first</li><li>second</li></ul><blockquote><p>quoted</p></blockquote><pre>a  &lt; b\n  c</pre><p>1 < 2</p>";
        let m = to_markup(html, "https://example.org/post/1");
        assert_eq!(m, "<span size=\"large\"><b>Title</b></span>\n\nOne <b>bold <i>both</i></b> &amp; <a href=\"https://example.org/rel\">a link</a>.\n\n•  first\n•  second\n\n▎ quoted\n\n<tt>a  &lt; b\n  c</tt>\n\n1 &lt; 2");
        // Sloppy nesting still gives well-formed markup.
        assert_eq!(to_markup("<b>a<i>b</b>c</i>", ""), "<b>a<i>b</i></b>c");
        assert_eq!(to_markup("<a href=\"javascript:x()\">no</a> <a href='#top'>top</a>", ""), "no top");
        assert_eq!(without_links("see <a href=\"https://x.org/?a=1&amp;b=2\">this <b>page</b></a> now"), "see this <b>page</b> now");
        assert_eq!(strip_tags("<p>Hello&nbsp;<b>world</b></p>\n<style>p{}</style><p>again</p>"), "Hello world again");
        assert_eq!(teaser("<p>one two three four</p>", 9), "one two…");
    }

    #[test]
    fn finds_feeds_and_resolves_links() {
        let page = "<html><head><link rel=\"stylesheet\" href=\"a.css\"><link rel=\"alternate\" type=\"application/rss+xml\" title=\"RSS\" href=\"/feed.xml\"></head><body></body></html>";
        assert_eq!(discover(page, "https://blog.example.org/posts/"), Some("https://blog.example.org/feed.xml".into()));
        assert_eq!(discover("<html><head></head></html>", "https://x"), None);
        assert_eq!(resolve("https://a.org/dir/page.html", "img.png"), "https://a.org/dir/img.png");
        assert_eq!(resolve("https://a.org/dir/", "//cdn.org/x"), "https://cdn.org/x");
        assert_eq!(resolve("https://a.org", "https://b.org/y"), "https://b.org/y");
    }

    #[test]
    fn list_and_marks() {
        let feeds = parse_list("# feeds\nOmarchy\thttps://example.org/feed.xml\nhttps://www.blog.example.com/rss\n\nOmarchy again\thttps://example.org/feed.xml\nnot a url\n");
        assert_eq!(feeds, vec![Feed { title: "Omarchy".into(), url: "https://example.org/feed.xml".into() }, Feed { title: "blog.example.com".into(), url: "https://www.blog.example.com/rss".into() }]);
        assert_eq!(hash("a"), hash("a"));
        assert_ne!(mark_of("feed1", "id"), mark_of("feed2", "id"));
        assert_eq!(decode_entities("a &amp; b &#8212; &#x2014; &bogus; &"), "a & b — — &bogus; &");
    }
}
