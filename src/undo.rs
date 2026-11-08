//! Undo / redo journal for file operations, like Nautilus and Finder.
//!
//! Every recorded operation knows its inverse. Undo applies the inverse and
//! pushes the original onto the redo stack; redo does the opposite.

use crate::ops;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;

const LIMIT: usize = 50;

#[derive(Clone, Debug)]
pub enum Op {
    /// Files moved or renamed: (from, to).
    Move(Vec<(gio::File, gio::File)>),
    /// Copies made: (source, copy).
    Copy(Vec<(gio::File, gio::File)>),
    /// Copies removed again (inverse of Copy).
    RemoveCopies(Vec<(gio::File, gio::File)>),
    /// Files moved to the trash.
    Trash(Vec<gio::File>),
    /// Files restored from the trash (inverse of Trash).
    Restore(Vec<gio::File>),
    /// Empty files/folders created.
    Create(Vec<(gio::File, bool)>),
    /// Created empties removed (inverse of Create).
    RemoveCreated(Vec<(gio::File, bool)>),
    /// Files produced from others (converted or resized images). Undo trashes them.
    Derive(Vec<gio::File>),
    /// Permission changes: (file, old mode, new mode).
    Mode(Vec<(gio::File, u32, u32)>),
    /// Ownership changes: (file, (old uid, old gid), (new uid, new gid)).
    Owner(Vec<(gio::File, (u32, u32), (u32, u32))>),
    /// Tag changes: (file, old, new).
    Tag(Vec<(gio::File, Option<String>, Option<String>)>),
}

impl Op {
    pub fn inverse(&self) -> Op {
        match self {
            Op::Move(pairs) => Op::Move(pairs.iter().map(|(a, b)| (b.clone(), a.clone())).collect()),
            Op::Copy(pairs) => Op::RemoveCopies(pairs.clone()),
            Op::RemoveCopies(pairs) => Op::Copy(pairs.clone()),
            Op::Trash(files) => Op::Restore(files.clone()),
            Op::Restore(files) => Op::Trash(files.clone()),
            Op::Create(files) => Op::RemoveCreated(files.clone()),
            Op::RemoveCreated(files) => Op::Create(files.clone()),
            Op::Derive(files) => Op::Trash(files.clone()),
            Op::Mode(changes) => Op::Mode(changes.iter().map(|(f, old, new)| (f.clone(), *new, *old)).collect()),
            Op::Owner(changes) => Op::Owner(changes.iter().map(|(f, old, new)| (f.clone(), *new, *old)).collect()),
            Op::Tag(changes) => Op::Tag(changes.iter().map(|(f, old, new)| (f.clone(), new.clone(), old.clone())).collect()),
        }
    }

    /// Short description for toasts ("Undo: moved 3 items").
    pub fn describe(&self) -> String {
        let n = |len: usize, what: &str| format!("{len} {}", if len == 1 { what.to_string() } else { format!("{what}s") });
        match self {
            Op::Move(p) if p.len() == 1 && same_parent(&p[0].0, &p[0].1) => "rename".into(),
            Op::Move(p) => format!("move of {}", n(p.len(), "item")),
            Op::Copy(p) => format!("copy of {}", n(p.len(), "item")),
            Op::RemoveCopies(p) => format!("removal of {}", n(p.len(), "copy")),
            Op::Trash(f) => format!("trashing {}", n(f.len(), "item")),
            Op::Restore(f) => format!("restore of {}", n(f.len(), "item")),
            Op::Create(f) | Op::RemoveCreated(f) => format!("creation of {}", n(f.len(), "item")),
            Op::Derive(f) => format!("creation of {}", n(f.len(), "file")),
            Op::Mode(c) => format!("mode change on {}", n(c.len(), "item")),
            Op::Owner(c) => format!("ownership change on {}", n(c.len(), "item")),
            Op::Tag(c) => format!("tag change on {}", n(c.len(), "item")),
        }
    }
}

fn same_parent(a: &gio::File, b: &gio::File) -> bool {
    match (a.parent(), b.parent()) {
        (Some(x), Some(y)) => x.equal(&y),
        (None, None) => true,
        _ => false,
    }
}

#[derive(Default)]
pub struct Journal {
    undo: RefCell<Vec<Op>>,
    redo: RefCell<Vec<Op>>,
}

impl Journal {
    pub fn record(&self, op: Op) {
        let mut undo = self.undo.borrow_mut();
        undo.push(op);
        if undo.len() > LIMIT {
            undo.remove(0);
        }
        self.redo.borrow_mut().clear();
    }

    pub fn pop_undo(&self) -> Option<Op> {
        self.undo.borrow_mut().pop()
    }

    pub fn pop_redo(&self) -> Option<Op> {
        self.redo.borrow_mut().pop()
    }

    pub fn push_redo(&self, op: Op) {
        self.redo.borrow_mut().push(op);
    }

    pub fn push_undo_only(&self, op: Op) {
        self.undo.borrow_mut().push(op);
    }
}

/// Find the trash entry for a file that lived at `orig`.
pub async fn find_in_trash(orig: &gio::File) -> Option<gio::File> {
    let orig_path = orig.path()?;
    let trash = gio::File::for_uri("trash:///");
    let enumerator = trash
        .enumerate_children_future("standard::name,trash::orig-path,trash::deletion-date", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT)
        .await
        .ok()?;
    let mut best: Option<(gio::File, String)> = None;
    loop {
        let batch = enumerator.next_files_future(200, glib::Priority::DEFAULT).await.ok()?;
        if batch.is_empty() {
            break;
        }
        for info in batch {
            let Some(path) = info.attribute_byte_string("trash::orig-path") else { continue };
            if std::path::Path::new(path.as_str()) == orig_path {
                let date = info.attribute_string("trash::deletion-date").map(|s| s.to_string()).unwrap_or_default();
                // Newest deletion wins when the same path was trashed several times.
                if best.as_ref().map(|(_, d)| date > *d).unwrap_or(true) {
                    best = Some((enumerator.child(&info), date));
                }
            }
        }
    }
    best.map(|(f, _)| f)
}

/// Apply `op` and return how many items it touched; errors stop at the first failure.
pub async fn apply(op: &Op, tags: &crate::tags::TagIndex) -> Result<usize, glib::Error> {
    let mut done = 0;
    match op {
        Op::Move(pairs) => {
            for (from, to) in pairs {
                if let Some(parent) = to.parent() {
                    if !parent.query_exists(gio::Cancellable::NONE) {
                        parent.make_directory_with_parents(gio::Cancellable::NONE)?;
                    }
                }
                ops::move_file(from.clone(), to.clone()).await?;
                tags.moved(from, to);
                done += 1;
            }
        }
        Op::Copy(pairs) => {
            for (src, dst) in pairs {
                ops::copy_recursive(src.clone(), dst.clone()).await?;
                done += 1;
            }
        }
        Op::RemoveCopies(pairs) => {
            for (_, dst) in pairs {
                ops::delete_recursive(dst.clone()).await?;
                tags.removed(dst);
                done += 1;
            }
        }
        Op::Trash(files) => {
            for f in files {
                ops::trash(f.clone()).await?;
                done += 1;
            }
        }
        Op::Restore(files) => {
            for f in files {
                let Some(trashed) = find_in_trash(f).await else {
                    return Err(glib::Error::new(gio::IOErrorEnum::NotFound, "The item is no longer in the trash"));
                };
                if let Some(parent) = f.parent() {
                    if !parent.query_exists(gio::Cancellable::NONE) {
                        parent.make_directory_with_parents(gio::Cancellable::NONE)?;
                    }
                }
                ops::move_file(trashed, f.clone()).await?;
                done += 1;
            }
        }
        Op::Create(files) => {
            for (f, dir) in files {
                if *dir { ops::make_dir(f.clone()).await? } else { ops::create_file(f.clone()).await? }
                done += 1;
            }
        }
        Op::RemoveCreated(files) => {
            for (f, dir) in files {
                // Only remove what is still empty, never work that happened since.
                if *dir {
                    f.delete_future(glib::Priority::DEFAULT).await.map_err(|_| glib::Error::new(gio::IOErrorEnum::NotEmpty, "The folder is not empty any more"))?;
                } else {
                    let info = f.query_info_future("standard::size", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT).await?;
                    if info.size() > 0 {
                        return Err(glib::Error::new(gio::IOErrorEnum::NotEmpty, "The file has content now"));
                    }
                    f.delete_future(glib::Priority::DEFAULT).await?;
                }
                done += 1;
            }
        }
        Op::Derive(files) => {
            // Redo after an undo: the outputs went to the trash, bring them back.
            for f in files {
                if f.query_exists(gio::Cancellable::NONE) {
                    done += 1;
                    continue;
                }
                let Some(trashed) = find_in_trash(f).await else {
                    return Err(glib::Error::new(gio::IOErrorEnum::NotFound, "The item is no longer in the trash"));
                };
                ops::move_file(trashed, f.clone()).await?;
                done += 1;
            }
        }
        Op::Mode(changes) => {
            for (f, _, new) in changes {
                f.set_attribute_uint32("unix::mode", *new, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
                done += 1;
            }
        }
        Op::Owner(changes) => {
            for (f, _, (uid, gid)) in changes {
                f.set_attribute_uint32("unix::uid", *uid, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
                f.set_attribute_uint32("unix::gid", *gid, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
                done += 1;
            }
        }
        Op::Tag(changes) => {
            for (f, _, new) in changes {
                tags.set(f, new.as_deref())?;
                done += 1;
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_round_trips() {
        let a = gio::File::for_path("/a");
        let b = gio::File::for_path("/b");
        let op = Op::Move(vec![(a.clone(), b.clone())]);
        match op.inverse() {
            Op::Move(p) => assert!(p[0].0.equal(&b) && p[0].1.equal(&a)),
            _ => panic!(),
        }
        assert!(matches!(Op::Trash(vec![a.clone()]).inverse(), Op::Restore(_)));
        assert!(matches!(Op::Copy(vec![(a.clone(), b.clone())]).inverse().inverse(), Op::Copy(_)));
        assert_eq!(Op::Move(vec![(a.clone(), gio::File::for_path("/c"))]).describe(), "rename");
    }

    #[test]
    fn journal_limits_and_clears_redo() {
        let j = Journal::default();
        for i in 0..60 {
            j.record(Op::Trash(vec![gio::File::for_path(format!("/{i}"))]));
        }
        assert_eq!(j.undo.borrow().len(), LIMIT);
        j.push_redo(Op::Trash(vec![]));
        j.record(Op::Trash(vec![]));
        assert!(j.pop_redo().is_none());
    }
}
