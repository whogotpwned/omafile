#!/usr/bin/env python3
"""Ask every built-in radio station for sound.

The stations omafile starts with are in src/radio.rs. A stream must answer with audio
(through its playlist, when the address is one); a podcast's feed must hold an episode
whose sound can be fetched. Addresses on the internet move: this is run in CI and once a
week, so a station that is gone is noticed and replaced.

    scripts/check-radio.py            check all, exit 1 when one fails
    scripts/check-radio.py --list     only print the stations
"""
import http.client
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENT = "omafile-radio-check"
TIMEOUT = 25
TRIES = 3
AUDIO_ENDINGS = (".mp3", ".m4a", ".aac", ".ogg", ".oga", ".opus", ".flac", ".wav", ".m4b")


def stations():
    source = (ROOT / "src/radio.rs").read_text()
    block = source[source.index("pub const DEFAULTS"):]
    block = block[: block.index("];")]
    found = re.findall(r'\(Kind::(Live|Podcast),\s*\w+,\s*"([^"]+)",\s*"([^"]+)"\)', block)
    if not found:
        sys.exit("no stations found in src/radio.rs")
    return found


def get(url, limit, headers=None):
    """The status, the content type, the final address and the first `limit` bytes."""
    request = urllib.request.Request(url, headers={"User-Agent": AGENT, **(headers or {})})
    with urllib.request.urlopen(request, timeout=TIMEOUT) as answer:
        return answer.status, (answer.headers.get("Content-Type") or "").lower(), answer.geturl(), answer.read(limit)


def looks_like_sound(kind, data):
    if kind.startswith(("audio/", "video/")) or kind.startswith("application/ogg"):
        return True
    # Some servers call everything a download: then the bytes decide.
    return data[:3] == b"ID3" or data[:4] == b"OggS" or data[:4] == b"fLaC" or (len(data) > 1 and data[0] == 0xFF and data[1] & 0xE0 == 0xE0) or data[4:8] == b"ftyp"


def playlist_entries(url, text):
    entries = []
    for line in text.splitlines():
        line = line.strip()
        match = re.match(r"(?i)file\d+\s*=\s*(\S+)", line)
        if match:
            entries.append(match.group(1))
        elif line.startswith(("http://", "https://")):
            entries.append(line)
    return [urllib.parse.urljoin(url, e) for e in entries]


def check_live(url):
    target = url
    if urllib.parse.urlparse(url).path.lower().endswith((".pls", ".m3u", ".m3u8")):
        _, _, final, data = get(url, 20000)
        entries = playlist_entries(final, data.decode("utf-8", "replace"))
        if not entries:
            return False, "the playlist names no stream"
        target = entries[0]
    try:
        status, kind, _, data = get(target, 16000, {"Icy-MetaData": "1"})
    except http.client.BadStatusLine as error:
        # Old SHOUTcast servers answer "ICY 200 OK", which is a stream all the same.
        if "ICY 200" in str(error):
            return True, f"{target} (ICY)"
        raise
    if status != 200 or len(data) < 2000:
        return False, f"{target}: status {status}, {len(data)} bytes"
    if not looks_like_sound(kind, data):
        return False, f"{target}: {kind or 'no content type'} is not sound"
    return True, f"{target} ({kind})"


def local(tag):
    return tag.rsplit("}", 1)[-1].lower()


def check_podcast(url):
    _, _, _, data = get(url, 12_000_000)
    try:
        tree = ET.fromstring(data)
    except ET.ParseError as error:
        return False, f"not a feed: {error}"
    episodes = []
    for item in tree.iter():
        if local(item.tag) not in ("item", "entry"):
            continue
        for child in item:
            address = child.get("url") or child.get("href") or ""
            kind = (child.get("type") or "").lower()
            enclosure = local(child.tag) == "enclosure" or (local(child.tag) == "link" and child.get("rel") == "enclosure") or local(child.tag) == "content"
            if enclosure and address and (kind.startswith("audio/") or urllib.parse.urlparse(address).path.lower().endswith(AUDIO_ENDINGS)):
                episodes.append(address)
                break
    if not episodes:
        return False, "no episode with sound in the feed"
    newest = urllib.parse.urljoin(url, episodes[0])
    status, kind, _, sound = get(newest, 16000, {"Range": "bytes=0-15999"})
    if status not in (200, 206) or len(sound) < 2000:
        return False, f"{newest}: status {status}, {len(sound)} bytes"
    if not looks_like_sound(kind, sound):
        return False, f"{newest}: {kind or 'no content type'} is not sound"
    return True, f"{len(episodes)} episodes, newest {newest[:90]}"


def check(kind, url):
    problem = ""
    for _ in range(TRIES):
        try:
            ok, detail = check_live(url) if kind == "Live" else check_podcast(url)
        except (urllib.error.URLError, http.client.HTTPException, OSError, ValueError) as error:
            ok, detail = False, f"{type(error).__name__}: {error}"
        if ok:
            return True, detail
        problem = detail
    return False, problem


def main():
    found = stations()
    if "--list" in sys.argv:
        for kind, name, url in found:
            print(f"{kind:8} {name:28} {url}")
        return 0
    failed = 0
    for kind, name, url in found:
        ok, detail = check(kind, url)
        failed += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {kind:8} {name:28} {url}\n       {detail}")
    print(f"\n{len(found) - failed} of {len(found)} stations answer with sound")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
