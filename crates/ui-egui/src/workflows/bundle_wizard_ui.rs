//! The folder wizard. Create a PDF ▸ Assemble a brief bundle… (and the Home card) opens it on its
//! options; dropping a folder on the window with nothing else to do opens it on a first page that
//! offers everything PdfCraft does with a folder: assemble a brief bundle, export each email to
//! PDF, or combine the folder's PDFs. A bundle is assembled on its own thread with its own
//! session ([`pdfcraft_engine::BundleOptions`]); the last page says what it did, duplicates and
//! everything kept aside included. Nothing in a folder is ever removed: everything is evidence.

// The web build has no folders: the wizard shows there but assembles nothing.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_variables))]

use crate::PdfCraftApp;

/// Which page of the wizard shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// A folder was dropped: choose what to do with it.
    Choose,
    Options,
    Running,
    Done,
}

/// (messages done, messages in all, the result once finished).
#[cfg(not(target_arch = "wasm32"))]
type Status = std::sync::Arc<std::sync::Mutex<(usize, usize, Option<Result<pdfcraft_engine::BundleReport, String>>)>>;

#[derive(Debug)]
pub(crate) struct BundleWizard {
    pub step: Step,
    /// The page the window last measured itself for: it re-measures only when the page changes,
    /// so it stays centred (re-measuring every frame centres it on no height at all).
    measured: Option<Step>,
    pub source: Option<std::path::PathBuf>,
    /// Emails and PDFs found in `source` (for the first page).
    pub found: (usize, usize),
    pub title: String,
    pub use_dates: bool,
    pub from: String,
    pub to: String,
    pub index: bool,
    /// 0 none, 1 page numbers, 2 Bates.
    pub numbering: u8,
    pub prefix: String,
    pub start: String,
    pub out: Option<std::path::PathBuf>,
    pub error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    status: Option<Status>,
    #[cfg(not(target_arch = "wasm32"))]
    pub report: Option<pdfcraft_engine::BundleReport>,
}

impl BundleWizard {
    fn new(source: Option<std::path::PathBuf>, step: Step) -> Self {
        let mut w = Self {
            step,
            measured: None,
            source: None,
            found: (0, 0),
            title: String::new(),
            use_dates: false,
            from: String::new(),
            to: String::new(),
            index: true,
            numbering: 1,
            prefix: String::new(),
            start: "1".into(),
            out: None,
            error: None,
            #[cfg(not(target_arch = "wasm32"))]
            status: None,
            #[cfg(not(target_arch = "wasm32"))]
            report: None,
        };
        if let Some(dir) = source {
            w.set_source(dir);
        }
        w
    }

    /// Use `dir`: title it after the folder, and put the bundle beside the folder.
    fn set_source(&mut self, dir: std::path::PathBuf) {
        self.title = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.out = dir.parent().map(std::path::Path::to_path_buf).or_else(|| Some(dir.clone()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let emails = pdfcraft_engine::email_files(&dir).len();
            let pdfs = std::fs::read_dir(&dir)
                .map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf"))).count())
                .unwrap_or(0);
            self.found = (emails, pdfs);
        }
        self.source = Some(dir);
    }

    /// The options as the engine takes them, or what is wrong with them.
    #[cfg(not(target_arch = "wasm32"))]
    fn options(&self) -> Result<pdfcraft_engine::BundleOptions, String> {
        let date = |s: &str, what: &str| -> Result<Option<chrono::NaiveDate>, String> {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(Some).map_err(|_| format!("{what} must be a date like 2026-11-03"))
        };
        let (from, to) = if self.use_dates { (date(&self.from, "From")?, date(&self.to, "To")?) } else { (None, None) };
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            return Err("From must be on or before To".into());
        }
        let numbering = match self.numbering {
            0 => pdfcraft_engine::Numbering::None,
            1 => pdfcraft_engine::Numbering::Pages,
            _ => {
                if self.prefix.chars().count() > 128 || self.prefix.contains('#') || self.prefix.contains(">>") {
                    return Err("The Bates prefix can't contain # or >>".into());
                }
                let start =
                    self.start.trim().parse::<u64>().ok().filter(|n| *n >= 1).ok_or("The first Bates number must be a whole number from 1")?;
                pdfcraft_engine::Numbering::Bates { prefix: self.prefix.clone(), start, digits: 6 }
            }
        };
        Ok(pdfcraft_engine::BundleOptions { title: self.title.trim().to_string(), from, to, index: self.index, numbering })
    }
}

impl PdfCraftApp {
    /// Create a PDF ▸ Assemble a brief bundle…: the wizard on its options, asking for the
    /// folder there.
    pub fn open_bundle_wizard(&mut self) {
        self.bundle_wizard = Some(BundleWizard::new(None, Step::Options));
    }

    /// A folder dropped on the window: the wizard's first page, offering what can be done with it.
    pub fn open_folder_wizard(&mut self, dir: std::path::PathBuf) {
        self.bundle_wizard = Some(BundleWizard::new(Some(dir), Step::Choose));
    }

    /// Start assembling with the wizard's options, on a thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_bundle(&mut self) {
        let Some(w) = self.bundle_wizard.as_mut() else { return };
        let (Some(source), Some(out)) = (w.source.clone(), w.out.clone()) else {
            w.error = Some("Choose the folder of emails and where the bundle goes".into());
            return;
        };
        let options = match w.options() {
            Ok(o) => o,
            Err(e) => {
                w.error = Some(e);
                return;
            }
        };
        let paths = pdfcraft_engine::email_files(&source);
        if paths.is_empty() {
            w.error = Some(format!("No .eml or .msg files in {}", source.display()));
            return;
        }
        let status: Status = std::sync::Arc::new(std::sync::Mutex::new((0, paths.len(), None)));
        let shared = std::sync::Arc::clone(&status);
        let paper = self.session.paper();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new().name("brief-bundle".into()).spawn(move || {
            let set = |done: usize, total: usize, result: Option<Result<pdfcraft_engine::BundleReport, String>>| {
                if let Ok(mut s) = shared.lock() {
                    *s = (done, total, result);
                }
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut session = pdfcraft_engine::Session::new();
                session.set_paper(paper);
                let mut write = |path: &std::path::Path, bytes: &[u8]| crate::editing::write_atomically(&path.to_string_lossy(), bytes);
                session.assemble_bundle(&paths, &options, &out, &mut write, &mut |d, t| set(d, t, None)).map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("the bundle stopped unexpectedly; nothing was removed from the folder".into()));
            set(0, 0, Some(result));
        });
        match spawned {
            Ok(_) => {
                w.status = Some(status);
                w.error = None;
                w.step = Step::Running;
            }
            Err(e) => w.error = Some(format!("Couldn't start: {e}")),
        }
    }
}

/// Draw the wizard, if it is open (each frame).
pub(crate) fn show(app: &mut PdfCraftApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    poll(app);
    let Some(w) = app.bundle_wizard.as_mut() else { return };
    let step = w.step.clone();
    let remeasure = w.measured.as_ref() != Some(&step);
    w.measured = Some(step.clone());
    let mut close = false;
    let response = crate::widgets::show_modal_sized(ctx, "brief_bundle_wizard", remeasure, |ui| {
        ui.set_width(520.0);
        match step {
            Step::Choose => choose_page(app, ui, &mut close),
            Step::Options => options_page(app, ui, &mut close),
            Step::Running => running_page(app, ui),
            Step::Done => done_page(app, ui, &mut close),
        }
    });
    let running = app.bundle_wizard.as_ref().is_some_and(|w| w.step == Step::Running);
    if close || (response.should_close() && !running) {
        app.bundle_wizard = None;
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(crate::theme::semibold(17.0)));
    ui.add_space(6.0);
}

fn muted(ui: &mut egui::Ui, text: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text).color(t.text_muted));
}

/// The first page after a folder drop: every folder feature, one click each.
fn choose_page(app: &mut PdfCraftApp, ui: &mut egui::Ui, close: &mut bool) {
    let Some(w) = app.bundle_wizard.as_ref() else { return };
    let (emails, pdfs) = w.found;
    let folder = w.source.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
    heading(ui, tl!("What would you like to do with this folder?"));
    muted(ui, &folder);
    muted(ui, &crate::i18n::fmt(tl!("{e} emails and {p} PDFs found"), &[("e", &emails.to_string()), ("p", &pdfs.to_string())]));
    ui.add_space(10.0);
    let choice = |ui: &mut egui::Ui, title: &str, detail: &str, enabled: bool| -> bool {
        let mut clicked = false;
        ui.add_enabled_ui(enabled, |ui| {
            let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
            let card =
                egui::Frame::NONE.inner_margin(egui::Margin::same(10)).corner_radius(egui::CornerRadius::same(8)).stroke(stroke).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    clicked = ui.add(egui::Button::new(egui::RichText::new(title).strong()).frame(false)).clicked();
                    ui.label(detail);
                });
            // The whole card is the choice, not just its title.
            let area =
                ui.interact(card.response.rect, ui.scope_id().with(title), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if enabled && area.hovered() {
                ui.painter().rect_stroke(
                    card.response.rect,
                    8.0,
                    egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
            clicked |= area.clicked();
        });
        ui.add_space(6.0);
        clicked
    };
    let bundle = choice(
        ui,
        tl!("Assemble a brief bundle"),
        tl!(
            "One PDF: a correspondence index, then every email in date order with its attachments, bookmarked and numbered. Duplicates are flagged, nothing is removed."
        ),
        emails > 0,
    );
    let export = choice(
        ui,
        tl!("Export each email to PDF"),
        tl!("A PDF per email and a file per attachment, named by date and time, in a new folder beside this one."),
        emails > 0,
    );
    let combine =
        choice(ui, tl!("Combine the PDFs in this folder"), tl!("Open Combine files with every PDF here, sorted by name, ready to join."), pdfs > 0);
    ui.add_space(4.0);
    if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
        *close = true;
    }
    let Some(source) = app.bundle_wizard.as_ref().and_then(|w| w.source.clone()) else { return };
    if bundle {
        if let Some(w) = app.bundle_wizard.as_mut() {
            w.step = Step::Options;
        }
    } else if export {
        *close = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Emails".into());
            let out = source.parent().unwrap_or(&source).join(format!("{name} - PDFs"));
            match std::fs::create_dir_all(&out) {
                Ok(()) => app.start_email_export(&source, out),
                Err(e) => app.notify_fmt("Couldn't make {dir}: {e}", &[("dir", &out.display().to_string()), ("e", &e.to_string())]),
            }
        }
    } else if combine {
        *close = true;
        #[cfg(not(target_arch = "wasm32"))]
        app.add_folder_to_combine(&source, false);
    }
}

fn options_page(app: &mut PdfCraftApp, ui: &mut egui::Ui, close: &mut bool) {
    heading(ui, tl!("Assemble a brief bundle"));
    muted(
        ui,
        tl!(
            "Every email in the folder, in the order sent, each followed by its attachments. Nothing is removed: duplicates are flagged, attachments that can't be pages are saved beside the bundle."
        ),
    );
    ui.add_space(10.0);
    let mut pick_source = false;
    let mut pick_out = false;
    let mut assemble = false;
    {
        let Some(w) = app.bundle_wizard.as_mut() else { return };
        egui::Grid::new("bundle_options").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(tl!("Emails"));
            ui.horizontal(|ui| {
                let shown = w.source.as_ref().map_or_else(|| tl!("No folder chosen").to_string(), |d| d.display().to_string());
                ui.label(egui::RichText::new(shown).monospace());
                pick_source = ui.button(tl!("Choose…")).clicked();
            });
            ui.end_row();
            ui.label(tl!("Title"));
            ui.add(egui::TextEdit::singleline(&mut w.title).desired_width(300.0));
            ui.end_row();
            ui.label(tl!("Dates"));
            ui.vertical(|ui| {
                ui.add(crate::widgets::checkbox(&mut w.use_dates, tl!("Only emails sent between (optional)")));
                if w.use_dates {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut w.from).hint_text("2026-01-01").desired_width(110.0));
                        ui.label(tl!("and"));
                        ui.add(egui::TextEdit::singleline(&mut w.to).hint_text("2026-12-31").desired_width(110.0));
                    });
                    ui.label(egui::RichText::new(tl!("Either may be left empty. Undated emails are always included, flagged.")).small());
                }
            });
            ui.end_row();
            ui.label(tl!("Index"));
            ui.add(crate::widgets::checkbox(&mut w.index, tl!("Correspondence index at the front")));
            ui.end_row();
            ui.label(tl!("Numbering"));
            ui.vertical(|ui| {
                crate::widgets::radio_value(ui, &mut w.numbering, 1, tl!("Page numbers"));
                crate::widgets::radio_value(ui, &mut w.numbering, 2, tl!("Bates numbers"));
                if w.numbering == 2 {
                    ui.horizontal(|ui| {
                        ui.label(tl!("Prefix"));
                        ui.add(egui::TextEdit::singleline(&mut w.prefix).hint_text("RCH-").desired_width(90.0));
                        ui.label(tl!("first number"));
                        ui.add(egui::TextEdit::singleline(&mut w.start).desired_width(70.0));
                    });
                }
                crate::widgets::radio_value(ui, &mut w.numbering, 0, tl!("None"));
            });
            ui.end_row();
            ui.label(tl!("Save to"));
            ui.horizontal(|ui| {
                let shown = w.out.as_ref().map_or_else(|| tl!("No folder chosen").to_string(), |d| d.display().to_string());
                ui.label(egui::RichText::new(shown).monospace());
                pick_out = ui.button(tl!("Choose…")).clicked();
            });
            ui.end_row();
        });
        if let Some(e) = &w.error {
            ui.add_space(6.0);
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                *close = true;
            }
            assemble = crate::widgets::pill_button(ui, tl!("Assemble bundle"), true).clicked();
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if pick_source {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose the folder of emails (.eml, .msg)"));
            app.ask_one(crate::pickers::Ask::Folder(dialog), None, |app: &mut PdfCraftApp, dir: std::path::PathBuf| {
                if let Some(w) = app.bundle_wizard.as_mut() {
                    w.set_source(dir);
                }
            });
        }
        if pick_out {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose where the bundle goes"));
            app.ask_one(crate::pickers::Ask::Folder(dialog), None, |app: &mut PdfCraftApp, dir: std::path::PathBuf| {
                if let Some(w) = app.bundle_wizard.as_mut() {
                    w.out = Some(dir);
                }
            });
        }
        if assemble {
            app.start_bundle();
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (pick_source, pick_out);
        if assemble && let Some(w) = app.bundle_wizard.as_mut() {
            w.error = Some(tl!("Brief bundles work in the desktop app").to_string());
        }
    }
}

fn running_page(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    heading(ui, tl!("Assembling the bundle…"));
    #[cfg(not(target_arch = "wasm32"))]
    let (done, total) = app.bundle_wizard.as_ref().and_then(|w| w.status.as_ref()).and_then(|s| s.lock().ok().map(|s| (s.0, s.1))).unwrap_or((0, 0));
    #[cfg(target_arch = "wasm32")]
    let (done, total) = {
        let _ = app;
        (0usize, 0usize)
    };
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(crate::i18n::fmt(tl!("{d} of {t} emails"), &[("d", &done.to_string()), ("t", &total.to_string())]));
    });
    ui.add(egui::ProgressBar::new(if total > 0 { done as f32 / total as f32 } else { 0.0 }));
}

fn done_page(app: &mut PdfCraftApp, ui: &mut egui::Ui, close: &mut bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(w) = app.bundle_wizard.as_ref() else { return };
        let mut open: Option<std::path::PathBuf> = None;
        let mut reveal: Option<std::path::PathBuf> = None;
        match (&w.report, &w.error) {
            (Some(r), _) => {
                heading(ui, tl!("Bundle assembled"));
                let bundle = r.bundle.clone();
                if let Some(b) = &bundle {
                    ui.label(egui::RichText::new(b.display().to_string()).monospace());
                }
                ui.label(crate::i18n::fmt(tl!("{e} emails, {p} pages."), &[("e", &r.emails.to_string()), ("p", &r.pages.to_string())]));
                let warn = ui.visuals().warn_fg_color;
                if !r.duplicates.is_empty() {
                    ui.colored_label(
                        warn,
                        crate::i18n::fmt(
                            tl!("{n} duplicate(s) detected, kept and flagged in the index and bookmarks:"),
                            &[("n", &r.duplicates.len().to_string())],
                        ),
                    );
                    for (file, n, of) in r.duplicates.iter().take(6) {
                        ui.label(format!("    {n}. {file}: {} {of}", tl!("duplicate of")));
                    }
                    if r.duplicates.len() > 6 {
                        ui.label(format!("    … {}", r.duplicates.len() - 6));
                    }
                }
                if r.undated > 0 {
                    ui.colored_label(
                        warn,
                        crate::i18n::fmt(tl!("{n} undated email(s), placed after the dated ones and flagged."), &[("n", &r.undated.to_string())]),
                    );
                }
                if !r.not_printable.is_empty() {
                    ui.label(crate::i18n::fmt(
                        tl!(
                            "{n} attachment(s) can't be pages (video, archives…): each has a page saying so, and the file is saved beside the bundle."
                        ),
                        &[("n", &r.not_printable.len().to_string())],
                    ));
                }
                if !r.unreadable.is_empty() {
                    ui.colored_label(
                        warn,
                        crate::i18n::fmt(
                            tl!("{n} file(s) couldn't be read: each has a page saying so, and the original is saved beside the bundle."),
                            &[("n", &r.unreadable.len().to_string())],
                        ),
                    );
                }
                if r.outside_range > 0 {
                    ui.label(crate::i18n::fmt(
                        tl!("{n} email(s) were outside the dates you chose and are not in this bundle."),
                        &[("n", &r.outside_range.to_string())],
                    ));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if crate::widgets::pill_button(ui, tl!("Close"), false).clicked() {
                        *close = true;
                    }
                    if crate::widgets::pill_button(ui, tl!("Show in Finder"), false).clicked() {
                        reveal = bundle.clone();
                    }
                    if crate::widgets::pill_button(ui, tl!("Open bundle"), true).clicked() {
                        open = bundle.clone();
                    }
                });
            }
            (None, error) => {
                heading(ui, tl!("The bundle wasn't made"));
                ui.colored_label(ui.visuals().error_fg_color, error.clone().unwrap_or_default());
                muted(ui, tl!("Nothing in the folder was changed."));
                if crate::widgets::pill_button(ui, tl!("Close"), false).clicked() {
                    *close = true;
                }
            }
        }
        if let Some(path) = reveal
            && let Err(e) = crate::menus::reveal(&path)
        {
            app.notify(e);
        }
        if let Some(path) = open {
            *close = true;
            app.open_path(&path.to_string_lossy());
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        if crate::widgets::pill_button(ui, tl!("Close"), false).clicked() {
            *close = true;
        }
    }
}

/// Pick up a finished bundle (each frame while one runs).
#[cfg(not(target_arch = "wasm32"))]
fn poll(app: &mut PdfCraftApp) {
    let Some(w) = app.bundle_wizard.as_mut() else { return };
    let Some(status) = w.status.clone() else { return };
    let finished = status.lock().ok().and_then(|mut s| s.2.take());
    match finished {
        Some(Ok(report)) => {
            w.report = Some(report);
            w.status = None;
            w.step = Step::Done;
        }
        Some(Err(e)) => {
            w.error = Some(e);
            w.status = None;
            w.step = Step::Done;
        }
        None => {
            if let Some(ctx) = &app.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }
}
