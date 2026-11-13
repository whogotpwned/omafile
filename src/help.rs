//! Help (F1): what omafile can do, every shortcut, and what is installed here.
//!
//! Three pages share one search field: Features (a curated tour, each entry with
//! the way to reach it and, where it is safe, a button that opens it), Shortcuts
//! (the full list from `bindings`) and About (version, what is new in it, which
//! optional tools were found on this computer, where settings are kept).

use crate::bindings;
use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const CHANGELOG: &str = include_str!("../CHANGELOG.md");
const WEBSITE: &str = "https://github.com/whogotpwned/omafile";

pub struct Feature {
    pub area: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub text: &'static str,
    /// The window action behind it ("" when there is none): its shortcut is shown.
    pub action: &'static str,
    /// Offer an "Open" button: only for actions that need no selection and change nothing.
    pub open: bool,
    /// Where to find it when there is no single shortcut.
    pub place: &'static str,
}

const fn f(area: &'static str, icon: &'static str, title: &'static str, text: &'static str, action: &'static str, open: bool, place: &'static str) -> Feature {
    Feature { area, icon, title, text, action, open, place }
}

pub const FEATURES: &[Feature] = &[
    // ----- looking at files
    f("Browsing", "view-grid-symbolic", "Four views", "List, grid, columns and gallery. Every folder remembers its view, sort order and icon size.", "", false, "Ctrl+1 … Ctrl+4"),
    f("Browsing", "tab-new-symbolic", "Tabs and two panes", "Tabs like in a browser, and a second pane beside the first. One key copies or moves the selection to the other side.", "split-view", true, ""),
    f("Browsing", "view-reveal-symbolic", "Preview and Quick Look", "The preview pane shows pictures, PDFs page by page, text with syntax colours, music, video, archives and eBooks. Space opens a large view.", "show-preview", true, ""),
    f("Browsing", "system-search-symbolic", "Find anything", "Type to filter the folder, search names below it, or search inside files. A search can be saved as a smart folder.", "deep-search", true, ""),
    f("Browsing", "edit-find-symbolic", "Command palette", "Every command, folder and place by name: type a few letters, press Enter.", "palette", true, ""),
    f("Browsing", "drive-harddisk-symbolic", "Where the space went", "Folder sizes in the list, a map of the disk usage to click through, and a finder for duplicate files.", "treemap", true, ""),
    // ----- keeping order
    f("Organising", "starred-symbolic", "Tags", "Seven colours for files and folders. Each colour in use becomes a listing in the sidebar.", "", false, "Ctrl+Alt+1 … Ctrl+Alt+7"),
    f("Organising", "folder-saved-search-symbolic", "Smart folders", "Saved searches that live in the sidebar and list their matches whenever you open them.", "save-smart", false, ""),
    f("Organising", "emblem-system-symbolic", "Folder rules", "Tidy a folder automatically: move, copy, tag, trash or sort files by name, type and age.", "rules", true, ""),
    f("Organising", "folder-open-symbolic", "Shelf", "A floating place to collect files from several folders, then copy or move them in one go.", "shelf-show", true, ""),
    f("Organising", "edit-undo-symbolic", "Undo for file operations", "Move, copy, rename and trash can be undone and redone, also after a mistake in a batch.", "undo", false, ""),
    f("Organising", "document-open-recent-symbolic", "Earlier versions", "Hourly Btrfs snapshots of your home folder: bring a file back as it was, compare it with now, or restore what was deleted.", "versions", true, ""),
    f("Organising", "document-edit-symbolic", "Rename many at once", "Patterns, find and replace, numbering and metadata, with a preview before anything changes.", "", false, "F2 with several files selected"),
    f("Organising", "accessories-text-editor-symbolic", "Notes on files", "A note on any file or folder, shown as a badge next to its name.", "edit-note", false, ""),
    // ----- documents and media
    f("Documents and media", "x-office-document-symbolic", "PDF tools", "Merge, split, extract pages, or make a PDF from pictures.", "", false, "Context menu › PDF"),
    f("Documents and media", "camera-photo-symbolic", "Captures", "The screenshots and screen recordings Omarchy makes, as a film strip by day. Drag one straight into another program, copy it, draw on it, make a GIF of a recording; a new capture appears by itself.", "captures", true, ""),
    f("Documents and media", "audio-x-generic-symbolic", "Radio", "Internet radio and podcasts: electronic music and tech and hacking news are there from the start, any other station is added by its address. It plays in Now Playing and on the media keys, and goes on when the window is closed.", "radio", true, ""),
    f("Documents and media", "image-x-generic-symbolic", "Picture and video tools", "Resize, rotate and convert pictures; trim videos, make GIFs, extract or convert audio.", "", false, "Context menu › Image / Video / Audio"),
    f("Documents and media", "x-office-address-book-symbolic", "eBooks", "Covers and details for EPUB and comics, rename from metadata, send to an eReader, convert with Calibre.", "", false, "Context menu › eBook"),
    f("Documents and media", "package-x-generic-symbolic", "Archives", "Look inside archives like folders, extract here, compress a selection.", "", false, "Context menu › Extract / Compress"),
    f("Documents and media", "audio-x-generic-symbolic", "Now Playing", "Whatever plays on this computer, with cover, live spectrum and full controls; a mini player sits in the sidebar.", "now-playing", true, ""),
    f("Documents and media", "application-rss+xml-symbolic", "Feeds", "RSS and Atom news to read where the files are, with unread counts in the sidebar.", "feeds", true, ""),
    // ----- devices and servers
    f("Devices and servers", "drive-removable-media-symbolic", "Drives and phones", "USB sticks, cards and phones appear in the sidebar when plugged in, mount themselves and eject with one click.", "", false, "Sidebar › Devices"),
    f("Devices and servers", "phone-symbolic", "iPhone and nearby devices", "Send and receive files with phones and computers on the same network (LocalSend), and import the iPhone's photos.", "nearby", true, ""),
    f("Devices and servers", "send-to-symbolic", "Share on the local network", "A page for any phone or computer: scan the code, download the selection or send files back.", "share-lan", false, "Context menu › Share on Local Network…"),
    f("Devices and servers", "network-server-symbolic", "Servers in two panes", "SFTP and SCP with local files on the left and the server on the right. Connections are saved in the sidebar, passwords in the system keyring.", "connect", true, ""),
    // ----- privacy
    f("Privacy", "changes-prevent-symbolic", "Vaults", "Encrypted folders with their own master password in the sidebar. A padlock guards them; one file takes them to another computer.", "vault-add", true, "Sidebar › Vaults"),
    f("Privacy", "document-revert-symbolic", "Backup to another drive", "Complete, browsable copies of your folders on an external disk or stick, made by themselves at the time you choose. Unchanged files take no extra space; the sidebar shows when the last backup ran. Omarchy's system snapshots stay as they are: this adds your own files, on another drive.", "backup", true, ""),
    f("Privacy", "dialog-password-symbolic", "Secrets stay out of sight", "Keys and credential files are never shown in previews or thumbnails.", "", false, "Preview pane"),
    f("Privacy", "emblem-ok-symbolic", "Checksums", "Verify a download against its SHA-256 sum.", "verify-checksum", false, "Context menu › Verify Checksum"),
    // ----- development
    f("For developers", "utilities-terminal-symbolic", "Terminal below the files", "A shell that follows the folder on show, so the files and the command line never drift apart.", "terminal-panel", true, ""),
    f("For developers", "media-playlist-shuffle-symbolic", "Git inside", "Status badges on files, stage, commit, switch branches and see diffs without leaving the folder.", "git-commit", false, ""),
    f("For developers", "applications-science-symbolic", "Python environments", "Finds the virtual environment of a project, activates it in the terminal and lists its packages.", "py-activate", false, ""),
    f("For developers", "applications-engineering-symbolic", "Project actions", "Knows Rust, Node, Python, Make and more: build, test and run from the project's folder.", "", false, "Command palette in a project folder"),
    f("For developers", "edit-copy-symbolic", "Compare", "Two files side by side, or two folders with what differs.", "compare", false, ""),
    // ----- making it yours
    f("Make it yours", "applications-graphics-symbolic", "Theme atelier", "Every Omarchy theme as a card with its background and colours; one click turns it on. Any picture becomes a theme of its own: omafile takes the colours out of it, shows the desktop they would make, and saves a theme Omarchy builds the rest of.", "themes", true, "Context menu of a picture › Create Theme from Picture…"),
    f("Make it yours", "applications-graphics-symbolic", "Follows your Omarchy theme", "Colours, accent, light or dark and the icon set come from the active theme and change with it at once.", "", false, "Sidebar › Omarchy Themes"),
    f("Make it yours", "preferences-desktop-locale-symbolic", "Your language", "English, German, Spanish, French, Italian, Portuguese and Dutch, chosen from the system.", "", false, "Omarchy › Setup › Language"),
    f("Make it yours", "input-keyboard-symbolic", "Keyboard first", "Every command has a shortcut; Vim keys can be switched on in groups.", "preferences", true, ""),
    f("Make it yours", "folder-symbolic", "The system's file manager", "Folders from other programs and “Show in folder” in browsers open in omafile; long transfers keep the computer awake.", "", false, "Preferences › Default file manager"),
    f("Make it yours", "view-restore-symbolic", "Remembers where you were", "Tabs, panes and window size come back on the next start.", "", false, "Preferences › Restore tabs on start"),
    f("Make it yours", "view-app-grid-symbolic", "On the Omarchy bar", "The shell plugin puts omafile on the bar: places, running transfers, unlocked vaults, connected phones.", "", false, "omarchy plugin add"),
];

/// Programs omafile uses when they are there: (commands, any of which will do; what for).
const TOOLS: &[(&[&str], &str)] = &[
    (&["gocryptfs"], "Vaults and encrypted folders"),
    (&["rg"], "Searching inside files"),
    (&["bat"], "Syntax colours in previews"),
    (&["pdftoppm"], "PDF previews and thumbnails"),
    (&["ffmpeg"], "Video and audio tools, cover art"),
    (&["ffmpegthumbnailer"], "Video thumbnails"),
    (&["bsdtar"], "Archives and eBook covers"),
    (&["git"], "Git status, commits and branches"),
    (&["uv"], "Fast Python environments"),
    (&["curl"], "News feeds"),
    (&["snapper"], "Earlier versions of files"),
    (&["parec", "pw-record"], "Live spectrum in Now Playing"),
    (&["heif-convert", "magick"], "HEIC photos from the iPhone"),
    (&["ebook-convert"], "Converting eBooks (Calibre)"),
    (&["docker", "podman"], "Containers in the sidebar"),
];

/// Inline Markdown as it appears in the changelog (**bold**, *italic*, `code`) as Pango markup.
pub fn inline_markup(text: &str) -> String {
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut out = String::with_capacity(escaped.len() + 16);
    let (mut bold, mut italic, mut code) = (false, false, false);
    let mut chars = escaped.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                out.push_str(if code { "</tt>" } else { "<tt>" });
                code = !code;
            }
            '*' if !code => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    out.push_str(if bold { "</b>" } else { "<b>" });
                    bold = !bold;
                } else {
                    out.push_str(if italic { "</i>" } else { "<i>" });
                    italic = !italic;
                }
            }
            c => out.push(c),
        }
    }
    // Whatever was left open is closed, innermost first, so the markup always parses.
    for (open, tag) in [(code, "</tt>"), (italic, "</i>"), (bold, "</b>")] {
        if open {
            out.push_str(tag);
        }
    }
    out
}

/// The changelog entry of one version: its headings ("Added", …) with their items.
pub fn whats_new(changelog: &str, version: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut inside = false;
    for line in changelog.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            inside = heading.split_whitespace().next() == Some(version);
        } else if inside {
            if let Some(section) = line.strip_prefix("### ") {
                out.push((section.trim().to_string(), Vec::new()));
            } else if let Some(item) = line.strip_prefix("- ") {
                if out.is_empty() {
                    out.push((String::new(), Vec::new()));
                }
                if let Some((_, items)) = out.last_mut() {
                    items.push(item.trim().to_string());
                }
            }
        }
    }
    out
}

fn features_list(window: &Rc<Window>, dialog: &adw::Dialog) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.add_css_class("omafile-bindings");
    list.set_selection_mode(gtk::SelectionMode::None);
    for feature in FEATURES {
        let (area, title, text) = (tr(feature.area), tr(feature.title), tr(feature.text));
        let icon = gtk::Image::from_icon_name(feature.icon);
        icon.set_pixel_size(22);
        icon.set_valign(gtk::Align::Start);
        icon.set_margin_top(3);
        icon.add_css_class("omafile-help-icon");
        let name = gtk::Label::builder().label(&title).xalign(0.0).wrap(true).css_classes(["omafile-help-title"]).build();
        let what = gtk::Label::builder().label(&text).xalign(0.0).wrap(true).css_classes(["omafile-dim"]).build();
        let words = gtk::Box::new(gtk::Orientation::Vertical, 2);
        words.set_hexpand(true);
        words.append(&name);
        words.append(&what);
        // How to get there: the shortcut of its action, or a place.
        let keys: Vec<String> = if feature.action.is_empty() { Vec::new() } else { bindings::accels_for(feature.action) };
        let how = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        how.set_margin_top(4);
        if let Some(key) = keys.first() {
            let cap = gtk::Label::new(Some(key));
            cap.add_css_class("omafile-keycap");
            how.append(&cap);
        }
        // Key names and commands stay as they are; places are sentences and get translated.
        let literal = feature.place.starts_with("Ctrl") || feature.place.starts_with("omarchy ");
        let place = if feature.place.is_empty() || literal { feature.place.to_string() } else { tr(feature.place) };
        if !place.is_empty() {
            let label = gtk::Label::builder().label(&place).xalign(0.0).wrap(true).css_classes(["omafile-dim", "caption"]).build();
            how.append(&label);
        }
        if how.first_child().is_some() {
            words.append(&how);
        }
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        line.set_margin_top(10);
        line.set_margin_bottom(10);
        line.set_margin_start(14);
        line.set_margin_end(12);
        line.append(&icon);
        line.append(&words);
        if feature.open {
            let open = gtk::Button::builder().label(tr("Open")).valign(gtk::Align::Center).build();
            open.add_css_class("pill");
            // Weak: the button is inside the dialog, and a dialog its own button keeps alive
            // is never freed.
            let (weak, dialog, action) = (Rc::downgrade(window), dialog.downgrade(), feature.action);
            open.connect_clicked(move |_| {
                if let Some(dialog) = dialog.upgrade() {
                    dialog.close();
                }
                let Some(w) = weak.upgrade() else { return };
                // After the dialog is gone, so the action finds the window in its normal state.
                glib::idle_add_local_once(move || {
                    gio::prelude::ActionGroupExt::activate_action(&w.window, action, None);
                });
            });
            line.append(&open);
        }
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.set_child(Some(&line));
        unsafe {
            row.set_data("group", Box::leak(area.clone().into_boxed_str()) as &'static str);
            row.set_data("haystack", format!("{area} {title} {text} {place} {}", keys.join(" ")).to_lowercase());
        }
        list.append(&row);
    }
    list.set_header_func(|row, before| {
        let group: &str = unsafe { row.data::<&'static str>("group").map(|p| *p.as_ref()).unwrap_or("") };
        let prev: Option<&str> = before.map(|b| unsafe { b.data::<&'static str>("group").map(|p| *p.as_ref()).unwrap_or("") });
        if prev == Some(group) {
            row.set_header(None::<&gtk::Widget>);
        } else {
            let label = gtk::Label::builder().label(group).xalign(0.0).margin_start(12).margin_top(16).margin_bottom(4).build();
            label.add_css_class("caption-heading");
            label.add_css_class("omafile-dim");
            row.set_header(Some(&label));
        }
    });
    list
}

/// What the About page says to whoever clicks its icon five times in a row.
const EGG: &str = "For my Sisko Aylin ❤️";

/// The fifth click of a quick series (and every fifth after it).
fn egg_due(clicks_in_a_row: i32) -> bool {
    clicks_in_a_row > 0 && clicks_in_a_row % 5 == 0
}

fn about_page(window: &Rc<Window>) -> gtk::Widget {
    let icon = gtk::Image::from_icon_name("io.github.whogotpwned.Omafile");
    icon.set_pixel_size(96);
    // Five quick clicks on the icon: it turns a somersault and shows the dedication.
    let egg = gtk::Label::builder().label(EGG).css_classes(["omafile-egg"]).build();
    let egg_reveal = gtk::Revealer::builder().transition_type(gtk::RevealerTransitionType::SlideDown).transition_duration(crate::motion::PANEL).child(&egg).build();
    let clicks = gtk::GestureClick::new();
    let (reveal, spin) = (egg_reveal.clone(), icon.clone());
    clicks.connect_pressed(move |_, n, _, _| {
        if egg_due(n) {
            spin.remove_css_class("omafile-egg-spin");
            spin.add_css_class("omafile-egg-spin");
            reveal.set_reveal_child(true);
        }
    });
    icon.add_controller(clicks);
    let name = gtk::Label::builder().label("omafile").css_classes(["title-1"]).build();
    let version = gtk::Label::builder().label(format!("{} {VERSION}", tr("Version"))).selectable(true).css_classes(["omafile-help-version"]).build();
    let tagline = gtk::Label::builder().label(tr("The file manager that feels like part of Omarchy.")).css_classes(["omafile-dim"]).wrap(true).justify(gtk::Justification::Center).build();
    let counts = gtk::Label::builder().label(format!("{} {}  ·  {} {}  ·  7 {}", FEATURES.len(), tr("features"), bindings::all().len(), tr("shortcuts and commands"), tr("languages"))).css_classes(["omafile-dim", "caption"]).build();
    let links = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    links.set_halign(gtk::Align::Center);
    links.set_margin_top(6);
    let tour = gtk::Button::builder().label(tr("Take the Tour")).build();
    tour.add_css_class("pill");
    tour.add_css_class("suggested-action");
    let weak = Rc::downgrade(window);
    tour.connect_clicked(move |_| {
        if let Some(w) = weak.upgrade() {
            w.show_tour();
        }
    });
    links.append(&tour);
    for (label, uri) in [(tr("Website"), WEBSITE.to_string()), (tr("Changelog"), format!("{WEBSITE}/blob/main/CHANGELOG.md")), (tr("Report a Problem"), format!("{WEBSITE}/issues"))] {
        let button = gtk::Button::builder().label(label).build();
        button.add_css_class("pill");
        let weak = Rc::downgrade(window);
        button.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                gtk::UriLauncher::new(&uri).launch(Some(&w.window), gio::Cancellable::NONE, |_| {});
            }
        });
        links.append(&button);
    }
    let head = gtk::Box::new(gtk::Orientation::Vertical, 6);
    head.set_margin_top(18);
    for w in [icon.upcast_ref::<gtk::Widget>(), egg_reveal.upcast_ref(), name.upcast_ref(), version.upcast_ref(), tagline.upcast_ref(), counts.upcast_ref(), links.upcast_ref()] {
        head.append(w);
    }

    // What is new in this version, straight from the changelog that was built in.
    let news = adw::PreferencesGroup::builder().title(format!("{} {VERSION}", tr("New in"))).build();
    let sections = whats_new(CHANGELOG, VERSION);
    if sections.is_empty() {
        news.add(&adw::ActionRow::builder().title(tr("See the changelog on the website")).build());
    }
    for (section, items) in sections {
        for item in items {
            let label = gtk::Label::builder().use_markup(true).label(inline_markup(&item)).wrap(true).xalign(0.0).selectable(true).margin_top(10).margin_bottom(10).margin_start(12).margin_end(12).build();
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            if !section.is_empty() {
                let badge = gtk::Label::builder().label(&section).valign(gtk::Align::Start).margin_top(10).margin_start(12).css_classes(["omafile-keycap"]).build();
                line.append(&badge);
            }
            line.append(&label);
            let row = gtk::ListBoxRow::builder().activatable(false).child(&line).build();
            news.add(&row);
        }
    }

    // What was found on this computer.
    let tools = adw::PreferencesGroup::builder().title(tr("On this computer")).description(tr("omafile uses these programs when they are installed; without one, only its feature is missing.")).build();
    let mut found_all = Vec::new();
    for (commands, what) in TOOLS {
        let found = commands.iter().find(|c| glib::find_program_in_path(c).is_some());
        found_all.push((commands.join(" / "), tr(what), found.is_some()));
    }
    found_all.push((tr("System keyring"), tr("Remembering server passwords"), crate::connections::keyring_available()));
    for (command, what, found) in found_all {
        let row = adw::ActionRow::builder().title(glib::markup_escape_text(&what)).subtitle(glib::markup_escape_text(&command)).build();
        let mark = gtk::Image::from_icon_name(if found { "emblem-ok-symbolic" } else { "window-close-symbolic" });
        mark.add_css_class(if found { "omafile-help-found" } else { "omafile-dim" });
        mark.set_tooltip_text(Some(&if found { tr("Installed") } else { tr("Not installed") }));
        row.add_suffix(&mark);
        tools.add(&row);
    }

    // Where things are kept.
    let places = adw::PreferencesGroup::builder().title(tr("Where things are kept")).build();
    for (what, path) in [(tr("Settings, tags, rules, vaults, feeds, connections"), glib::user_config_dir().join("omafile")), (tr("Session, read marks, import log"), glib::user_state_dir().join("omafile")), (tr("Thumbnails and fetched feeds"), glib::user_cache_dir())] {
        let row = adw::ActionRow::builder().title(glib::markup_escape_text(&what)).subtitle(glib::markup_escape_text(&crate::window::abbreviate_home(&path))).subtitle_selectable(true).build();
        let open = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Open")).build();
        open.add_css_class("flat");
        let weak = Rc::downgrade(window);
        open.connect_clicked(move |b| {
            if let Some(w) = weak.upgrade() {
                if let Some(dialog) = b.ancestor(adw::Dialog::static_type()).and_downcast::<adw::Dialog>() {
                    dialog.close();
                }
                w.navigate(gio::File::for_path(&path));
            }
        });
        row.add_suffix(&open);
        places.add(&row);
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 22);
    column.set_margin_bottom(24);
    column.set_margin_start(12);
    column.set_margin_end(12);
    column.append(&head);
    column.append(&news);
    column.append(&tools);
    column.append(&places);
    let clamp = adw::Clamp::builder().maximum_size(680).child(&column).build();
    gtk::ScrolledWindow::builder().child(&clamp).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build().upcast()
}

impl Window {
    /// Help, opened on one of its pages: "features", "keys" or "about".
    pub fn show_help(self: &Rc<Self>, page: &str) {
        let dialog = adw::Dialog::builder().title(format!("omafile {VERSION}")).content_width(760).content_height(780).build();
        dialog.add_css_class("omafile-bindings-dialog");
        let features = features_list(self, &dialog);
        let keys = bindings::bindings_list();
        let scrolled = |list: &gtk::ListBox| {
            let clamp = adw::Clamp::builder().maximum_size(720).child(list).margin_start(12).margin_end(12).margin_bottom(16).build();
            gtk::ScrolledWindow::builder().child(&clamp).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build()
        };
        let stack = adw::ViewStack::new();
        stack.add_titled_with_icon(&scrolled(&features), Some("features"), &tr("Features"), "starred-symbolic");
        stack.add_titled_with_icon(&scrolled(&keys), Some("keys"), &tr("Shortcuts"), "input-keyboard-symbolic");
        stack.add_titled_with_icon(&about_page(self), Some("about"), &tr("About"), "help-about-symbolic");
        stack.set_visible_child_name(page);
        let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&switcher));
        let version = gtk::Label::builder().label(format!("v{VERSION}")).css_classes(["omafile-dim", "caption"]).margin_start(8).build();
        header.pack_start(&version);

        let search = gtk::SearchEntry::builder().placeholder_text(tr("Search features and shortcuts…")).hexpand(true).build();
        let bar = adw::Clamp::builder().maximum_size(720).child(&search).margin_start(12).margin_end(12).margin_top(6).margin_bottom(8).build();
        let (f2, k2, s2) = (features.clone(), keys.clone(), stack.clone());
        search.connect_search_changed(move |e| {
            let needle = e.text().to_string();
            bindings::filter_rows(&f2, &needle);
            bindings::filter_rows(&k2, &needle);
            // Searching from the About page means looking for something: show the features.
            if !needle.is_empty() && s2.visible_child_name().as_deref() == Some("about") {
                s2.set_visible_child_name("features");
            }
        });
        let hint = gtk::Label::new(Some(tr("Type to search  ·  Esc closes  ·  omafile --print-bindings prints the shortcuts").as_str()));
        hint.add_css_class("omafile-dim");
        hint.add_css_class("caption");
        hint.set_margin_bottom(8);
        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.add_top_bar(&bar);
        content.set_content(Some(&stack));
        content.add_bottom_bar(&hint);
        dialog.set_child(Some(&content));
        search.set_key_capture_widget(Some(&dialog));
        // Esc in the search field clears it; with an empty field it closes the dialog.
        let d2 = dialog.downgrade();
        search.connect_stop_search(move |e| {
            if e.text().is_empty() {
                if let Some(dialog) = d2.upgrade() {
                    dialog.close();
                }
            } else {
                e.set_text("");
            }
        });
        self.present_dialog(&dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_egg_waits_for_the_fifth_click_and_is_a_dedication() {
        assert!(!egg_due(0) && !egg_due(1) && !egg_due(4) && egg_due(5) && !egg_due(6) && egg_due(10));
        assert!(EGG.contains("Aylin") && EGG.ends_with("❤️"));
    }

    #[test]
    fn features_point_at_real_actions() {
        let known: Vec<&str> = crate::actions::ACCELS.iter().map(|(a, _)| a.trim_start_matches("win.")).collect();
        let listed: Vec<&str> = bindings::TABLE.iter().map(|(_, action, _, _)| *action).chain(crate::actions::PALETTE_ACTIONS.iter().map(|(action, _, _)| *action)).collect();
        for feature in FEATURES {
            assert!(!feature.title.is_empty() && !feature.text.is_empty() && !feature.area.is_empty());
            if !feature.action.is_empty() {
                assert!(known.contains(&feature.action) || listed.contains(&feature.action), "{}: no action called {}", feature.title, feature.action);
            }
            let has_key = !feature.action.is_empty() && crate::actions::ACCELS.iter().any(|(a, _)| a.trim_start_matches("win.") == feature.action);
            assert!(has_key || !feature.place.is_empty(), "{}: say how to reach it", feature.title);
            assert!(!feature.open || !feature.action.is_empty(), "{}: an Open button needs an action", feature.title);
        }
        // Titles are what people search for: no two alike.
        let mut titles: Vec<&str> = FEATURES.iter().map(|f| f.title).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), FEATURES.len());
    }

    #[test]
    fn every_help_text_is_in_the_catalogs() {
        // The catalogs all hold the same keys (scripts/check-i18n.py), so English stands for all.
        let catalog = crate::json::parse(include_str!("../data/i18n/en.json")).unwrap();
        let has = |key: &str| catalog.get(key).is_some();
        for feature in FEATURES {
            for text in [feature.area, feature.title, feature.text] {
                assert!(has(text), "not in the catalogs: {text}");
            }
            let literal = feature.place.starts_with("Ctrl") || feature.place.starts_with("omarchy ");
            assert!(feature.place.is_empty() || literal || has(feature.place), "not in the catalogs: {}", feature.place);
        }
        for (_, what) in TOOLS {
            assert!(has(what), "not in the catalogs: {what}");
        }
    }

    #[test]
    fn the_changelog_has_this_version() {
        let news = whats_new(CHANGELOG, VERSION);
        assert!(!news.is_empty() && news.iter().any(|(_, items)| !items.is_empty()), "CHANGELOG.md has no entry for {VERSION}");
        let sample = "# Changelog\n\n## 1.2.0 — 2026-01-02\n\n### Added\n- one\n- two\n\n### Fixed\n- three\n\n## 1.1.0 — 2026-01-01\n\n### Added\n- old\n";
        assert_eq!(whats_new(sample, "1.2.0"), vec![("Added".to_string(), vec!["one".to_string(), "two".to_string()]), ("Fixed".to_string(), vec!["three".to_string()])]);
        assert_eq!(whats_new(sample, "1.1.0"), vec![("Added".to_string(), vec!["old".to_string()])]);
        assert!(whats_new(sample, "9.9.9").is_empty());
    }

    #[test]
    fn changelog_markdown_becomes_markup() {
        assert_eq!(inline_markup("**Vaults** travel as *one* file `a<b>.tar` & more"), "<b>Vaults</b> travel as <i>one</i> file <tt>a&lt;b&gt;.tar</tt> &amp; more");
        assert_eq!(inline_markup("`*.gocryptfs.tar` stays"), "<tt>*.gocryptfs.tar</tt> stays");
        assert_eq!(inline_markup("left **open"), "left <b>open</b>");
        for (_, items) in whats_new(CHANGELOG, VERSION) {
            for item in items {
                assert!(gtk::pango::parse_markup(&inline_markup(&item), '\0').is_ok(), "does not parse: {item}");
            }
        }
    }
}
