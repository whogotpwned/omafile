//! Reading news feeds inside omafile: a Feeds section in the sidebar and a page with the
//! list of articles on the left and the article itself on the right. Feeds are fetched
//! with curl (http and https only), kept on disk for offline reading, and what has been
//! read is remembered. Parsing and HTML handling live in `feeds`.

use crate::feeds::{self, Article, Feed};
use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

/// The pseudo address of "all feeds together".
pub const ALL: &str = "";

/// Download a feed or a web page. Only http(s): a hostile page must not be able to point
/// the reader at local files.
pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(tr("Only http and https addresses can be read"));
    }
    let args = ["curl", "-sSL", "--max-time", "25", "--max-filesize", "12000000", "--proto", "=http,https", "-A", "omafile feed reader", "-H", "Accept: application/rss+xml, application/atom+xml, application/xml;q=0.9, text/html;q=0.8, */*;q=0.5", "--", url];
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let process = gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE).map_err(|e| e.message().to_string())?;
    let (stdout, stderr) = process.communicate_future(None).await.map_err(|e| e.message().to_string())?;
    if process.is_successful() {
        Ok(stdout.map(|b| b.to_vec()).unwrap_or_default())
    } else {
        let message = stderr.map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default();
        Err(message.strip_prefix("curl: ").unwrap_or(&message).to_string())
    }
}

struct Entry {
    feed_url: String,
    feed_title: String,
    article: Article,
}

pub struct FeedPage {
    pub root: gtk::Box,
    title: gtk::Label,
    spinner: gtk::Spinner,
    list: gtk::ListBox,
    list_stack: gtk::Stack,
    reader: gtk::Stack,
    reader_scroll: gtk::ScrolledWindow,
    image: gtk::Picture,
    article_title: gtk::Label,
    meta: gtk::Label,
    body: gtk::Label,
    /// Which feed is on show: an address, or ALL.
    feed: RefCell<String>,
    entries: RefCell<Vec<Entry>>,
    marks: RefCell<HashSet<String>>,
    shown: Cell<Option<usize>>,
    filling: Cell<bool>,
    sidebar_pending: Cell<bool>,
}

impl FeedPage {
    fn new(window: &Rc<Window>) -> Rc<Self> {
        // Top row: back, which feed, and what can be done with it.
        let back = gtk::Button::builder()
            .child(&{
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
                b.append(&gtk::Label::new(Some(&tr("Files"))));
                b
            })
            .tooltip_text(tr("Back to the files (Esc)"))
            .build();
        back.add_css_class("flat");
        let title = gtk::Label::builder().css_classes(["title-4"]).ellipsize(gtk::pango::EllipsizeMode::End).hexpand(true).xalign(0.0).margin_start(6).build();
        let spinner = gtk::Spinner::new();
        let mark_all = gtk::Button::builder().icon_name("object-select-symbolic").tooltip_text(tr("Mark all as read")).build();
        let refresh = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text(tr("Fetch new articles (R)")).build();
        for b in [&mark_all, &refresh] {
            b.add_css_class("flat");
        }
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.set_margin_top(8);
        top.set_margin_bottom(6);
        top.set_margin_start(10);
        top.set_margin_end(10);
        top.append(&back);
        top.append(&title);
        top.append(&spinner);
        top.append(&mark_all);
        top.append(&refresh);

        // Left: the articles.
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("omafile-feed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let list_scroll = gtk::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let nothing = adw::StatusPage::builder().icon_name("application-rss+xml-symbolic").title(tr("No articles yet")).description(tr("Fetching the feed…")).build();
        nothing.add_css_class("compact");
        let list_stack = gtk::Stack::new();
        list_stack.add_named(&nothing, Some("empty"));
        list_stack.add_named(&list_scroll, Some("list"));
        list_stack.set_size_request(300, -1);

        // Right: the article.
        let image = gtk::Picture::new();
        image.set_content_fit(gtk::ContentFit::Cover);
        image.set_can_shrink(true);
        image.set_size_request(-1, 220);
        image.set_overflow(gtk::Overflow::Hidden);
        image.add_css_class("omafile-feed-image");
        image.set_visible(false);
        let article_title = gtk::Label::builder().wrap(true).xalign(0.0).selectable(true).css_classes(["omafile-feed-title"]).build();
        let meta = gtk::Label::builder().wrap(true).xalign(0.0).css_classes(["omafile-dim", "caption"]).build();
        let body = gtk::Label::builder().wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).xalign(0.0).yalign(0.0).selectable(true).css_classes(["omafile-feed-body"]).build();
        let open = gtk::Button::builder().label(tr("Open in Browser")).halign(gtk::Align::Start).tooltip_text(tr("Open the article's page (O)")).build();
        open.add_css_class("pill");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
        column.set_margin_top(18);
        column.set_margin_bottom(28);
        column.set_margin_start(22);
        column.set_margin_end(22);
        column.append(&image);
        column.append(&article_title);
        column.append(&meta);
        column.append(&body);
        column.append(&open);
        let clamp = adw::Clamp::builder().maximum_size(760).child(&column).build();
        let reader_scroll = gtk::ScrolledWindow::builder().child(&clamp).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).hexpand(true).build();
        let pick = adw::StatusPage::builder().icon_name("application-rss+xml-symbolic").title(tr("Pick an article")).description(tr("↑ ↓ or J K move through the list, O opens the page in the browser, M marks as unread, R fetches new articles.")).build();
        let reader = gtk::Stack::new();
        reader.set_transition_type(gtk::StackTransitionType::Crossfade);
        reader.set_transition_duration(crate::motion::FAST);
        reader.add_named(&pick, Some("empty"));
        reader.add_named(&reader_scroll, Some("article"));

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&list_stack));
        paned.set_end_child(Some(&reader));
        paned.set_position(360);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.set_vexpand(true);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-feed-page");
        root.append(&top);
        root.append(&paned);

        let page = Rc::new(FeedPage {
            root,
            title,
            spinner,
            list,
            list_stack,
            reader,
            reader_scroll,
            image,
            article_title,
            meta,
            body,
            feed: RefCell::new(String::new()),
            entries: RefCell::new(Vec::new()),
            marks: RefCell::new(feeds::read_marks()),
            shown: Cell::new(None),
            filling: Cell::new(false),
            sidebar_pending: Cell::new(false),
        });

        let wweak = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = wweak.upgrade() {
                w.show_files();
                w.refocus();
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        page.list.connect_row_selected(move |_, row| {
            let (Some(p), Some(w), Some(row)) = (weak.upgrade(), wweak.upgrade(), row) else { return };
            if !p.filling.get() {
                p.show_article(&w, row.index() as usize);
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        refresh.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                let feed = p.feed.borrow().clone();
                w.feed_refresh(if feed.is_empty() { None } else { Some(feed) }, true);
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        mark_all.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                p.mark_all(&w);
            }
        });
        let weak = Rc::downgrade(&page);
        let wweak = Rc::downgrade(window);
        open.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                p.open_in_browser(&w);
            }
        });
        // Keys for reading without the mouse.
        let keys = gtk::EventControllerKey::new();
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        keys.connect_key_pressed(move |_, key, _, state| {
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return glib::Propagation::Proceed };
            if state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Escape => {
                    w.show_files();
                    w.refocus();
                }
                gdk::Key::j => p.step(1),
                gdk::Key::k => p.step(-1),
                gdk::Key::o | gdk::Key::Return | gdk::Key::KP_Enter => p.open_in_browser(&w),
                gdk::Key::r => {
                    let feed = p.feed.borrow().clone();
                    w.feed_refresh(if feed.is_empty() { None } else { Some(feed) }, true);
                }
                gdk::Key::m => p.toggle_read(&w),
                gdk::Key::space => {
                    let adj = p.reader_scroll.vadjustment();
                    adj.set_value((adj.value() + adj.page_size() * 0.85).min(adj.upper() - adj.page_size()));
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        page.root.add_controller(keys);
        page
    }

    fn mark(&self, entry: &Entry) -> String {
        feeds::mark_of(&entry.feed_url, &entry.article.id)
    }

    /// Put the articles of `feed` (or of every feed) into the list, from what is on disk.
    fn fill(self: &Rc<Self>, feed: &str) {
        let all = feeds::list();
        let wanted: Vec<Feed> = if feed.is_empty() { all } else { all.into_iter().filter(|f| f.url == feed).collect() };
        self.title.set_text(&if feed.is_empty() { tr("All Articles") } else { wanted.first().map(|f| f.title.clone()).unwrap_or_default() });
        let mut entries: Vec<Entry> = Vec::new();
        for f in &wanted {
            if let Some((_, articles)) = feeds::cached(&f.url) {
                entries.extend(articles.into_iter().map(|article| Entry { feed_url: f.url.clone(), feed_title: f.title.clone(), article }));
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.article.date));
        // Keep the place: the article on show stays selected when the list is rebuilt.
        let keep = self.shown.get().and_then(|i| self.entries.borrow().get(i).map(|e| self.mark(e)));
        self.filling.set(true);
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let now = glib::DateTime::now_local().ok();
        let (now_unix, offset) = (now.as_ref().map(|n| n.to_unix()).unwrap_or(0), now.as_ref().map(|n| n.utc_offset().as_seconds()).unwrap_or(0));
        let marks = self.marks.borrow();
        for entry in &entries {
            let unread = !marks.contains(&self.mark(entry));
            let title = gtk::Label::builder().label(&entry.article.title).xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).lines(2).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-feed-row-title"]).build();
            let mut facts: Vec<String> = Vec::new();
            let date = feeds::friendly_date(entry.article.date, now_unix, offset);
            if !date.is_empty() {
                facts.push(date);
            }
            if feed.is_empty() {
                facts.push(entry.feed_title.clone());
            }
            let meta = gtk::Label::builder().label(facts.join("  ·  ")).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-dim", "caption"]).build();
            let teaser = gtk::Label::builder().label(feeds::teaser(&entry.article.html, 110)).xalign(0.0).wrap(true).lines(2).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-dim", "caption"]).build();
            let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
            text.set_hexpand(true);
            text.append(&title);
            text.append(&meta);
            if !teaser.text().is_empty() {
                text.append(&teaser);
            }
            let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            dot.add_css_class("omafile-feed-dot");
            dot.set_valign(gtk::Align::Start);
            dot.set_margin_top(7);
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            line.append(&dot);
            line.append(&text);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&line));
            if unread {
                row.add_css_class("omafile-feed-unread");
            }
            self.list.append(&row);
        }
        drop(marks);
        self.list_stack.set_visible_child_name(if entries.is_empty() { "empty" } else { "list" });
        let again = keep.and_then(|k| entries.iter().position(|e| self.mark(e) == k));
        self.entries.replace(entries);
        self.feed.replace(feed.to_string());
        self.shown.set(again);
        match again.and_then(|i| self.list.row_at_index(i as i32)) {
            Some(row) => self.list.select_row(Some(&row)),
            None => {
                self.list.unselect_all();
                self.reader.set_visible_child_name("empty");
            }
        }
        self.filling.set(false);
    }

    fn step(&self, delta: i32) {
        let count = self.entries.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let next = match self.list.selected_row() {
            Some(row) => (row.index() + delta).clamp(0, count - 1),
            None => 0,
        };
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            row.grab_focus();
        }
    }

    fn show_article(self: &Rc<Self>, window: &Rc<Window>, index: usize) {
        let entries = self.entries.borrow();
        let Some(entry) = entries.get(index) else { return };
        self.shown.set(Some(index));
        let a = &entry.article;
        self.article_title.set_text(&a.title);
        let now = glib::DateTime::now_local().ok();
        let date = feeds::friendly_date(a.date, now.as_ref().map(|n| n.to_unix()).unwrap_or(0), now.as_ref().map(|n| n.utc_offset().as_seconds()).unwrap_or(0));
        let facts: Vec<&str> = [entry.feed_title.as_str(), a.author.as_str(), date.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        self.meta.set_text(&facts.join("  ·  "));
        let markup = feeds::to_markup(&a.html, if a.link.is_empty() { &entry.feed_url } else { &a.link });
        // Markup built from somebody else's HTML: if Pango will not have it, show plain text.
        // (Checked without the links: those are GTK's addition, Pango alone does not know <a>.)
        if gtk::pango::parse_markup(&feeds::without_links(&markup), '\0').is_ok() {
            self.body.set_markup(&markup);
        } else {
            self.body.set_text(&feeds::strip_tags(&a.html));
        }
        self.reader.set_visible_child_name("article");
        self.reader_scroll.vadjustment().set_value(0.0);
        // The lead picture arrives when it arrives; an older request must not overwrite a newer article.
        self.image.set_visible(false);
        self.image.set_paintable(None::<&gdk::Paintable>);
        if let Some(url) = a.image.clone() {
            let weak = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let Ok(bytes) = fetch(&url).await else { return };
                let Some(p) = weak.upgrade() else { return };
                if p.shown.get() != Some(index) {
                    return;
                }
                let loader = gtk::gdk_pixbuf::PixbufLoader::new();
                if loader.write(&bytes).is_ok() && loader.close().is_ok() {
                    if let Some(pixbuf) = loader.pixbuf().filter(|pb| pb.width() >= 200) {
                        p.image.set_paintable(Some(&gdk::Texture::for_pixbuf(&pixbuf)));
                        p.image.set_visible(true);
                    }
                }
            });
        }
        // Reading it marks it read.
        let mark = self.mark(entry);
        drop(entries);
        if self.marks.borrow_mut().insert(mark) {
            feeds::save_read_marks(&self.marks.borrow());
            if let Some(row) = self.list.row_at_index(index as i32) {
                row.remove_css_class("omafile-feed-unread");
            }
            self.counts_changed(window);
        }
    }

    fn toggle_read(self: &Rc<Self>, window: &Rc<Window>) {
        let Some(index) = self.shown.get() else { return };
        let Some(mark) = self.entries.borrow().get(index).map(|e| self.mark(e)) else { return };
        let now_unread = {
            let mut marks = self.marks.borrow_mut();
            if marks.remove(&mark) {
                true
            } else {
                marks.insert(mark);
                false
            }
        };
        feeds::save_read_marks(&self.marks.borrow());
        if let Some(row) = self.list.row_at_index(index as i32) {
            if now_unread {
                row.add_css_class("omafile-feed-unread");
            } else {
                row.remove_css_class("omafile-feed-unread");
            }
        }
        self.counts_changed(window);
    }

    fn mark_all(self: &Rc<Self>, window: &Rc<Window>) {
        {
            let mut marks = self.marks.borrow_mut();
            for entry in self.entries.borrow().iter() {
                marks.insert(self.mark(entry));
            }
        }
        feeds::save_read_marks(&self.marks.borrow());
        let mut i = 0;
        while let Some(row) = self.list.row_at_index(i) {
            row.remove_css_class("omafile-feed-unread");
            i += 1;
        }
        self.counts_changed(window);
    }

    fn open_in_browser(&self, window: &Rc<Window>) {
        let link = self.shown.get().and_then(|i| self.entries.borrow().get(i).map(|e| e.article.link.clone())).filter(|l| l.starts_with("http"));
        if let Some(link) = link {
            gtk::UriLauncher::new(&link).launch(Some(&window.window), gio::Cancellable::NONE, |_| {});
        }
    }

    /// The unread numbers in the sidebar follow, a moment later: stepping through a list
    /// must not rebuild the sidebar on every key press.
    fn counts_changed(self: &Rc<Self>, window: &Rc<Window>) {
        window.feed_counts.replace(None);
        if self.sidebar_pending.replace(true) {
            return;
        }
        let (weak, wweak) = (Rc::downgrade(self), Rc::downgrade(window));
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            if let Some(p) = weak.upgrade() {
                p.sidebar_pending.set(false);
            }
            if let Some(w) = wweak.upgrade() {
                w.refresh_sidebar();
            }
        });
    }
}

impl Window {
    /// How many unread articles each feed has (from what is on disk), worked out once and
    /// kept until something changes.
    pub fn feed_unread(&self) -> HashMap<String, usize> {
        if let Some(counts) = self.feed_counts.borrow().as_ref() {
            return counts.clone();
        }
        let marks = feeds::read_marks();
        let mut counts = HashMap::new();
        for feed in feeds::list() {
            let unread = feeds::cached(&feed.url).map(|(_, articles)| articles.iter().filter(|a| !marks.contains(&feeds::mark_of(&feed.url, &a.id))).count()).unwrap_or(0);
            counts.insert(feed.url, unread);
        }
        self.feed_counts.replace(Some(counts.clone()));
        counts
    }

    /// Show a feed (or all of them with `feeds::ALL`) where the files are.
    pub fn open_feed(self: &Rc<Self>, feed: &str) {
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.feed_page.borrow().clone();
        let page = match existing {
            Some(p) => p,
            None => {
                let p = FeedPage::new(self);
                self.content_stack.add_named(&p.root, Some("feeds"));
                self.feed_page.replace(Some(p.clone()));
                p
            }
        };
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.marks.replace(feeds::read_marks());
        page.shown.set(None);
        page.fill(feed);
        self.content_stack.set_visible_child_name("feeds");
        page.list.grab_focus();
        // Fetch when there is nothing yet, or what is there is older than a quarter of an hour.
        let stale = |url: &str| std::fs::metadata(feeds::cache_path(url)).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|age| age > Duration::from_secs(900)).unwrap_or(true);
        let wanted: Vec<String> = if feed.is_empty() { feeds::list().into_iter().map(|f| f.url).collect() } else { vec![feed.to_string()] };
        if wanted.iter().any(|u| stale(u)) {
            self.feed_refresh(if feed.is_empty() { None } else { Some(feed.to_string()) }, false);
        }
    }

    /// Fetch one feed, or all of them. `loud`: say so when a feed cannot be reached.
    pub fn feed_refresh(self: &Rc<Self>, only: Option<String>, loud: bool) {
        let wanted: Vec<Feed> = feeds::list().into_iter().filter(|f| only.as_ref().map(|u| *u == f.url).unwrap_or(true)).collect();
        if wanted.is_empty() {
            return;
        }
        if let Some(page) = self.feed_page.borrow().as_ref() {
            page.spinner.start();
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut failed: Vec<String> = Vec::new();
            for feed in wanted {
                match fetch(&feed.url).await {
                    Ok(bytes) if feeds::parse_feed(&String::from_utf8_lossy(&bytes)).is_some() => {
                        let path = feeds::cache_path(&feed.url);
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let _ = std::fs::write(path, bytes);
                    }
                    _ => failed.push(feed.title),
                }
            }
            let Some(w) = this.upgrade() else { return };
            w.feed_counts.replace(None);
            w.refresh_sidebar();
            let page = w.feed_page.borrow().clone();
            if let Some(page) = page {
                page.spinner.stop();
                if w.content_stack.visible_child_name().as_deref() == Some("feeds") {
                    let current = page.feed.borrow().clone();
                    page.fill(&current);
                }
            }
            if loud && !failed.is_empty() {
                w.toast_error(&format!("{}: {}", tr("Could not fetch"), failed.join(", ")));
            }
        });
    }

    /// Sidebar › Add Feed…
    pub fn feed_add_dialog(self: &Rc<Self>) {
        let entry = gtk::Entry::builder().placeholder_text(tr("Address of a feed or of a site, e.g. example.org")).activates_default(true).width_chars(42).build();
        let dialog = adw::AlertDialog::new(Some(tr("Add Feed").as_str()), Some(tr("RSS and Atom feeds are read right here. Give the address of the feed, or of the site: omafile looks for its feed.").as_str()));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("add", tr("Add").as_str())]);
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            let text = entry2.text().trim().to_string();
            if response != "add" || text.is_empty() {
                w.refocus();
                return;
            }
            let url = if text.contains("://") { text } else { format!("https://{text}") };
            w.feed_add(url);
        });
        entry.grab_focus();
    }

    /// Fetch `url`; if it is a web page, follow its feed link. Then list the feed and show it.
    pub fn feed_add(self: &Rc<Self>, url: String) {
        self.toast(&tr("Looking for the feed…"));
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut address = url.clone();
            let mut found: Option<(String, Vec<u8>)> = None;
            let mut problem = String::new();
            // At most one hop: from a page to the feed it announces.
            for _ in 0..2 {
                match fetch(&address).await {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        if let Some((title, _)) = feeds::parse_feed(&text) {
                            found = Some((if title.is_empty() { feeds::host_of(&address) } else { title }, bytes));
                            break;
                        }
                        match feeds::discover(&text, &address) {
                            Some(next) if next != address => address = next,
                            _ => {
                                problem = tr("No feed found at that address");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        problem = e;
                        break;
                    }
                }
            }
            let Some(w) = this.upgrade() else { return };
            match found {
                Some((title, bytes)) => {
                    let path = feeds::cache_path(&address);
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(path, bytes);
                    feeds::add(&title, &address);
                    w.feed_counts.replace(None);
                    w.refresh_sidebar();
                    w.open_feed(&address);
                }
                None => w.toast_error(&format!("{}: {}", tr("Cannot add the feed"), if problem.is_empty() { tr("No feed found at that address") } else { problem })),
            }
        });
    }

    pub fn feed_remove(self: &Rc<Self>, url: &str) {
        feeds::remove(url);
        self.feed_counts.replace(None);
        self.refresh_sidebar();
        let showing = self.feed_page.borrow().as_ref().map(|p| *p.feed.borrow() == url).unwrap_or(false);
        if showing && self.content_stack.visible_child_name().as_deref() == Some("feeds") {
            self.show_files();
            self.refocus();
        }
        self.toast(&tr("Feed removed"));
    }

    /// Mark everything in a feed as read, from the sidebar's menu.
    pub fn feed_mark_read(self: &Rc<Self>, url: &str) {
        let mut marks = feeds::read_marks();
        if let Some((_, articles)) = feeds::cached(url) {
            for a in articles {
                marks.insert(feeds::mark_of(url, &a.id));
            }
        }
        feeds::save_read_marks(&marks);
        self.feed_counts.replace(None);
        self.refresh_sidebar();
        let page = self.feed_page.borrow().clone();
        if let Some(page) = page {
            page.marks.replace(marks);
            if self.content_stack.visible_child_name().as_deref() == Some("feeds") {
                let current = page.feed.borrow().clone();
                page.fill(&current);
            }
        }
    }

    /// Look for new articles shortly after start and then twice an hour.
    pub fn start_feed_updates(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if let Some(w) = this.upgrade() {
                w.feed_refresh(None, false);
            }
        });
        let this = Rc::downgrade(self);
        glib::timeout_add_seconds_local(1800, move || match this.upgrade() {
            Some(w) => {
                w.feed_refresh(None, false);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }
}
