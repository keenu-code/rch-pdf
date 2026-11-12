//! A brief bundle: a folder of email messages as one PDF for a brief. A correspondence index
//! comes first, then every message in the order it was sent, each followed by its attachments;
//! a bookmark per message with its attachments under it; numbers on every page (page numbers or
//! Bates numbers). Nothing is left out: a duplicate is flagged (in the index, its bookmark and
//! the report), an undated message goes at the end flagged, an attachment that can't be a page
//! (a video, an archive) gets a page saying so and is saved beside the bundle, and a message that
//! can't be read gets a page saying so with its file saved beside the bundle too. Only a date
//! range the user sets keeps messages out, and the report counts them.

// Assembling reads and writes folders, which the web build has none of: only the types are used there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_create::{EmailFacts, IndexEntry};
use pdfcraft_organize::OutlineNode;

use crate::email_export::{ExportKind, MAX_EMAIL_BYTES, clean_name, local_offset_at};
use crate::{Edit, EditError, HeaderFooter, Session};

/// How the bundle's pages are numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Numbering {
    None,
    /// `1`, `2`… at the bottom right.
    Pages,
    /// A prefix and a zero-padded number at the bottom right (`RCH-000001`).
    Bates {
        prefix: String,
        start: u64,
        digits: u8,
    },
}

#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// The bundle's title: the index heading and the file name.
    pub title: String,
    /// Only messages sent on or after / on or before these local dates (either may be absent).
    pub from: Option<chrono::NaiveDate>,
    pub to: Option<chrono::NaiveDate>,
    pub index: bool,
    pub numbering: Numbering,
}

/// What assembling a bundle did.
#[derive(Debug, Clone, Default)]
pub struct BundleReport {
    /// The bundle file, once written.
    pub bundle: Option<PathBuf>,
    pub pages: usize,
    /// Messages in the bundle (duplicates and undated included).
    pub emails: usize,
    /// (file, its number, the number it duplicates).
    pub duplicates: Vec<(String, usize, usize)>,
    pub undated: usize,
    /// Messages outside the chosen date range, so not in the bundle.
    pub outside_range: usize,
    /// Attachments that can't be pages: (attachment, the file it was saved as).
    pub not_printable: Vec<(String, String)>,
    /// Messages that couldn't be read: (file, why); each has a page in the bundle saying so.
    pub unreadable: Vec<(String, String)>,
    /// Every file written (the bundle and anything saved beside it).
    pub written: Vec<PathBuf>,
}

/// One message on its way into the bundle.
struct Item {
    file: String,
    bytes: Vec<u8>,
    facts: Option<EmailFacts>,
    why: Option<String>,
}

/// One PDF in the bundle, with the bookmark it gets.
struct Part {
    title: String,
    pdf: Arc<Vec<u8>>,
    pages: usize,
}

impl Session {
    /// Assemble the brief bundle for the email messages `paths` into `out_dir` (see the module
    /// documentation), writing every file with `write` under a name no file there has yet, and
    /// calling `progress(done, total)` after each message.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn assemble_bundle(
        &self,
        paths: &[PathBuf],
        options: &BundleOptions,
        out_dir: &Path,
        write: &mut dyn FnMut(&Path, &[u8]) -> std::io::Result<()>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<BundleReport, EditError> {
        let mut report = BundleReport::default();
        let title = {
            let t = clean_name(&options.title, 100);
            if t.is_empty() { "Brief".to_string() } else { t }
        };
        let side_dir_name = format!("{title} - not printable");
        let side_dir = out_dir.join(&side_dir_name);

        // Read every message; keep the ones in range.
        let mut items = Vec::new();
        for path in paths {
            let file = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let bytes = match std::fs::metadata(path) {
                Ok(m) if m.len() > MAX_EMAIL_BYTES => Err(format!("larger than {} MB", MAX_EMAIL_BYTES >> 20)),
                Ok(_) => std::fs::read(path).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let item = match bytes {
                Err(why) => Item { file, bytes: Vec::new(), facts: None, why: Some(why) },
                Ok(bytes) => match pdfcraft_create::email_with_facts(&file, &bytes, self.paper(), 10.0) {
                    Ok((_, facts)) => Item { file, bytes, facts: Some(facts), why: None },
                    Err(e) => Item { file, bytes, facts: None, why: Some(e.to_string()) },
                },
            };
            if let Some(sent) = item.facts.as_ref().and_then(|f| f.sent) {
                let date = local_date(sent);
                if options.from.is_some_and(|d| date < Some(d)) || options.to.is_some_and(|d| date > Some(d)) {
                    report.outside_range += 1;
                    continue;
                }
            }
            items.push(item);
        }
        // Sent order; undated after the dated; unreadable last. Ties by file name.
        items.sort_by(|a, b| {
            let rank = |i: &Item| match (&i.facts, i.facts.as_ref().and_then(|f| f.sent)) {
                (Some(_), Some(t)) => (0, t),
                (Some(_), None) => (1, 0),
                (None, _) => (2, 0),
            };
            rank(a).cmp(&rank(b)).then_with(|| a.file.cmp(&b.file))
        });

        // Number them and find duplicates (flagged, never removed).
        let mut first_of: HashMap<String, usize> = HashMap::new();
        let mut entries: Vec<IndexEntry> = Vec::new();
        let mut parts_of: Vec<Vec<Part>> = Vec::new();
        let total = items.len();
        for (i, item) in items.into_iter().enumerate() {
            let number = i + 1;
            let mut entry = IndexEntry { number, ..Default::default() };
            let mut parts = Vec::new();
            match &item.facts {
                None => {
                    let why = item.why.clone().unwrap_or_default();
                    let saved = self.save_beside(&side_dir, &format!("{number:04} {}", item.file), &item.bytes, write, &mut report);
                    entry.subject = item.file.clone();
                    entry.flags.push(format!("COULD NOT BE READ ({why}). The original file is saved as \"{side_dir_name}/{saved}\"."));
                    parts.push(self.placeholder(
                        &format!("{number}. Could not be read: {}", item.file),
                        &format!("EMAIL COULD NOT BE READ\n\nFile: {}\nWhy: {why}\n\nThe original file is saved beside this bundle as:\n{side_dir_name}/{saved}", item.file),
                    )?);
                    report.unreadable.push((item.file.clone(), why));
                }
                Some(facts) => {
                    entry.when = facts.sent.map(when_text);
                    entry.from = facts.from.clone();
                    entry.to = facts.to.clone();
                    entry.subject = facts.subject.clone().unwrap_or_else(|| "(no subject)".into());
                    if facts.sent.is_none() {
                        report.undated += 1;
                        entry.flags.push("UNDATED: placed after the dated emails".into());
                    }
                    let key = duplicate_key(facts);
                    match first_of.get(&key) {
                        Some(&first) => {
                            entry.flags.push(format!("DUPLICATE of {first}"));
                            report.duplicates.push((item.file.clone(), number, first));
                            if let Some(original) = entries.get_mut(first - 1) {
                                let note = format!("ALSO APPEARS as {number}");
                                if !original.flags.contains(&note) {
                                    original.flags.push(note);
                                }
                            }
                        }
                        None => {
                            first_of.insert(key, number);
                        }
                    }
                    // Attachments first, so the message's own list can name what was saved where.
                    let mut saved_as = Vec::new();
                    let mut attachment_parts = Vec::new();
                    for (k, (name, data)) in facts.attachments.iter().enumerate() {
                        let size = data.len();
                        let (bytes, kind, _) = self.attachment_output(name, data.clone());
                        let printable = kind == ExportKind::Converted;
                        if printable {
                            saved_as.push(String::new());
                            entry.attachments.push(name.clone());
                            attachment_parts.push(Part { title: format!("Attachment: {name}"), pages: page_count(&bytes), pdf: bytes });
                        } else {
                            let saved = self.save_beside(&side_dir, &format!("{number:04}-{:02} {name}", k + 1), &bytes, write, &mut report);
                            let place = format!("{side_dir_name}/{saved}");
                            saved_as.push(place.clone());
                            entry.attachments.push(format!("{name} (not printable, provided separately)"));
                            report.not_printable.push((name.clone(), place.clone()));
                            attachment_parts.push(self.placeholder(
                                &format!("Attachment (not printable): {name}"),
                                &format!("ATTACHMENT NOT PRINTABLE\n\nAttachment: {name} ({size} bytes)\nTo email {number}: {}\n\nProvided separately as:\n{place}", entry.subject),
                            )?);
                        }
                    }
                    let (doc, _) = pdfcraft_create::email_with_saved_names(&item.file, &item.bytes, self.paper(), 10.0, &saved_as)?;
                    let pdf = self.write_new(&doc)?;
                    let flag = if entry.flags.iter().any(|f| f.starts_with("DUPLICATE")) { " (DUPLICATE)" } else { "" };
                    let label = format!("{number}. {} - {}{flag}", entry.when.as_deref().unwrap_or("Undated"), entry.subject);
                    parts.push(Part { title: label, pages: page_count(&pdf), pdf });
                    parts.extend(attachment_parts);
                }
            }
            entries.push(entry);
            parts_of.push(parts);
            progress(number, total);
        }
        report.emails = entries.len();

        // The index, laid out until its own length stops changing the page numbers it states.
        let summary = summary_line(&report, options);
        let mut index_pdf: Option<(Arc<Vec<u8>>, usize)> = None;
        if options.index {
            let mut index_pages = 1usize;
            for _ in 0..4 {
                set_starts(&mut entries, &parts_of, index_pages, &options.numbering);
                let doc = pdfcraft_create::bundle_index(&format!("{title}: correspondence index"), &summary, &entries, self.paper(), 10.0)?;
                let pdf = self.write_new(&doc)?;
                let n = page_count(&pdf);
                let settled = n == index_pages;
                index_pages = n;
                index_pdf = Some((pdf, n));
                if settled {
                    break;
                }
            }
        }

        // One PDF, then the outline and the numbers.
        let mut sources: Vec<Result<crate::CombineSource, EditError>> = Vec::new();
        let mut outline = Vec::new();
        let mut page = 0usize;
        if let Some((pdf, n)) = &index_pdf {
            sources.push(Ok(("Correspondence index".into(), Arc::clone(pdf), None)));
            outline.push(node("Correspondence index", page, Vec::new()));
            page += n;
        }
        for parts in &parts_of {
            let mut children = Vec::new();
            let mut head: Option<(String, usize)> = None;
            for (k, part) in parts.iter().enumerate() {
                if k == 0 {
                    head = Some((part.title.clone(), page));
                } else {
                    children.push(node(&part.title, page, Vec::new()));
                }
                sources.push(Ok((part.title.clone(), Arc::clone(&part.pdf), None)));
                page += part.pages;
            }
            if let Some((title, at)) = head {
                outline.push(node(&title, at, children));
            }
        }
        let combined = self.combine_each(sources)?.ok_or_else(|| EditError::Source("there are no emails to put in a bundle".into()))?;
        let mut scratch = Session::new();
        scratch.set_paper(self.paper());
        let id = scratch.open(format!("{title} - Brief bundle.pdf"), None, combined, None).map_err(|e| EditError::Source(e.to_string()))?;
        scratch.apply(id, Edit::ImportBookmarks { parent: Vec::new(), index: 0, bookmarks: outline, replace: true })?;
        if let Some(text) = footer_text(&options.numbering) {
            let pages: Vec<usize> = (0..page).collect();
            let mut settings = HeaderFooter { font_size: 9.0, ..HeaderFooter::default() };
            settings.text[5] = text;
            scratch.apply(id, Edit::AddHeaderFooter { pages, settings, replace: false })?;
        }
        let bytes = scratch.save_bytes(id)?;
        report.pages = page;
        let target = crate::email_export::unused_path(out_dir, &format!("{title} - Brief bundle.pdf"))
            .ok_or_else(|| EditError::Source("no unused file name for the bundle".into()))?;
        write(&target, &bytes).map_err(|e| EditError::Source(format!("{}: {e}", target.display())))?;
        report.written.push(target.clone());
        report.bundle = Some(target);
        Ok(report)
    }

    /// Save `bytes` beside the bundle (in `dir`, made if missing) as `name` or a numbered
    /// variant; the name it got.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_beside(
        &self,
        dir: &Path,
        name: &str,
        bytes: &[u8],
        write: &mut dyn FnMut(&Path, &[u8]) -> std::io::Result<()>,
        report: &mut BundleReport,
    ) -> String {
        let name = {
            let n = clean_name(name, 180);
            if n.is_empty() { "file".to_string() } else { n }
        };
        let saved = std::fs::create_dir_all(dir).ok().and_then(|()| crate::email_export::unused_path(dir, &name)).and_then(|target| {
            write(&target, bytes).ok()?;
            let shown = target.file_name().map(|n| n.to_string_lossy().into_owned());
            report.written.push(target);
            shown
        });
        saved.unwrap_or_else(|| format!("{name} (could not be saved)"))
    }

    /// A one-page PDF saying `text`, bookmarked as `title`.
    fn placeholder(&self, title: &str, text: &str) -> Result<Part, EditError> {
        let (doc, _) = pdfcraft_create::from_text_escaped(title, text, self.paper(), 12.0)?;
        let pdf = self.write_new(&doc)?;
        Ok(Part { title: title.to_string(), pages: page_count(&pdf), pdf })
    }
}

/// The local date a message was sent.
fn local_date(sent: i64) -> Option<chrono::NaiveDate> {
    chrono::DateTime::from_timestamp(sent.checked_add(local_offset_at(sent))?, 0).map(|t| t.date_naive())
}

/// `Tue 3 Nov 2026 09:14` in local time.
fn when_text(sent: i64) -> String {
    sent.checked_add(local_offset_at(sent))
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map_or_else(|| "Undated".to_string(), |t| t.format("%a %-d %b %Y %H:%M").to_string())
}

/// What makes two files the same message: its Message-ID, else when, who and what with which
/// attachments.
fn duplicate_key(f: &EmailFacts) -> String {
    match f.message_id.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(id) => format!("id:{}", id.trim_matches(['<', '>']).to_lowercase()),
        None => {
            let attachments: Vec<String> = f.attachments.iter().map(|(n, b)| format!("{n}:{}", b.len())).collect();
            format!("facts:{:?}|{}|{}|{}|{}", f.sent, f.from, f.to, f.subject.as_deref().unwrap_or(""), attachments.join(","))
        }
    }
}

/// Where each entry starts, as the bundle numbers it: the index takes the first `index_pages`.
fn set_starts(entries: &mut [IndexEntry], parts_of: &[Vec<Part>], index_pages: usize, numbering: &Numbering) {
    let mut page = index_pages;
    for (entry, parts) in entries.iter_mut().zip(parts_of) {
        entry.at = page_label(page, numbering);
        page += parts.iter().map(|p| p.pages).sum::<usize>();
    }
}

/// How the bundle names its 0-based page `page`.
fn page_label(page: usize, numbering: &Numbering) -> String {
    match numbering {
        Numbering::Bates { prefix, start, digits } => {
            let n = start.saturating_add(page as u64);
            format!("{prefix}{n:0width$}", width = usize::from(*digits))
        }
        _ => format!("page {}", page + 1),
    }
}

/// The footer template for `numbering` (bottom right), if any.
fn footer_text(numbering: &Numbering) -> Option<String> {
    match numbering {
        Numbering::None => None,
        Numbering::Pages => Some("<<1>>".into()),
        Numbering::Bates { prefix, start, digits } => Some(format!("<<Bates Number#{digits}#{start}#{prefix}#>>")),
    }
}

/// The line under the index heading.
fn summary_line(r: &BundleReport, o: &BundleOptions) -> String {
    let mut parts = vec![format!("{} emails in the order they were sent, each followed by its attachments.", r.emails)];
    match (o.from, o.to) {
        (Some(f), Some(t)) => parts.push(format!("Sent {} to {}.", f.format("%-d %b %Y"), t.format("%-d %b %Y"))),
        (Some(f), None) => parts.push(format!("Sent from {}.", f.format("%-d %b %Y"))),
        (None, Some(t)) => parts.push(format!("Sent up to {}.", t.format("%-d %b %Y"))),
        (None, None) => {}
    }
    if !r.duplicates.is_empty() {
        parts.push(format!("{} duplicate(s) detected and flagged (kept, not removed).", r.duplicates.len()));
    }
    parts.push("Times are local.".into());
    parts.join(" ")
}

fn node(title: &str, page: usize, children: Vec<OutlineNode>) -> OutlineNode {
    OutlineNode { title: title.to_string(), page: Some(page), view: None, color: None, italic: false, bold: false, open: false, children }
}

/// Pages in a PDF this session wrote (0 if it can't be read, which a written PDF always can).
fn page_count(pdf: &Arc<Vec<u8>>) -> usize {
    pdfcraft_cos::Document::open(Arc::clone(pdf)).ok().and_then(|d| pdfcraft_organize::page_count(&d).ok()).unwrap_or(0)
}
