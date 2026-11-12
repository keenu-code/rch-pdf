//! The folder wizard: Assemble a brief bundle from the menu, and a dropped folder's chooser,
//! through to a bundle on disk, in the real shell.

use egui_kittest::{Harness, kittest::Queryable};
use pdfcraft_ui_egui::PdfCraftApp;

fn eml(subject: &str, date: &str) -> String {
    format!("From: Ada <ada@example.com>\r\nTo: b@example.com\r\nSubject: {subject}\r\nDate: {date}\r\n\r\nHello.\r\n")
}

/// With PDFCRAFT_BUNDLE_SHOT=<dir>, save what the window shows as `<dir>/<name>.png`.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_BUNDLE_SHOT") {
        let _gpu = crate::gpu();
        h.render().unwrap().save(format!("{dir}/{name}.png")).unwrap();
    }
}

fn harness(setup: impl FnOnce(&mut PdfCraftApp) + 'static) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1300.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        setup(&mut app);
        app
    });
    h.run_steps(3);
    h
}

#[test]
fn the_menu_opens_the_bundle_options() {
    let mut h = harness(|_| {});
    assert!(h.state_mut().execute("create.bundle"));
    h.run_steps(2);
    h.get_by_label("Assemble bundle");
    h.get_by_label("Correspondence index at the front");
    assert!(h.query_all_by_label("No folder chosen").count() >= 1);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.query_by_label("Assemble bundle").is_none());
}

/// A dropped folder offers every folder feature; choosing the bundle and assembling it writes
/// one bundle beside the folder and says what it flagged.
#[test]
fn a_dropped_folder_becomes_a_bundle_through_the_wizard() {
    let root = std::env::temp_dir().join(format!("pdfcraft-bundle-wizard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mail = root.join("Smith v Jones");
    std::fs::create_dir_all(&mail).unwrap();
    std::fs::write(mail.join("a.eml"), eml("First", "Tue, 3 Nov 2026 10:00:00 +0000")).unwrap();
    std::fs::write(mail.join("b.eml"), eml("Second", "Wed, 4 Nov 2026 10:00:00 +0000")).unwrap();
    std::fs::write(mail.join("b copy.eml"), eml("Second", "Wed, 4 Nov 2026 10:00:00 +0000")).unwrap();
    let folder = mail.clone();
    let mut h = harness(move |app| app.open_folder_wizard(folder));
    h.get_by_label("What would you like to do with this folder?");
    h.get_by_label("3 emails and 0 PDFs found");
    h.get_by_label("Export each email to PDF");
    h.get_by_label("Combine the PDFs in this folder");
    shot(&mut h, "1-choose");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Assemble a brief bundle").click();
    h.run_steps(2);
    shot(&mut h, "2-options");
    h.get_by_label("Assemble bundle").click();
    let mut done = false;
    for _ in 0..400 {
        h.run_steps(1);
        if h.query_by_label("Bundle assembled").is_some() {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(done, "the bundle finished");
    h.get_by_label("1 duplicate(s) detected, kept and flagged in the index and bookmarks:");
    shot(&mut h, "3-done");
    let bundle = root.join("Smith v Jones - Brief bundle.pdf");
    assert!(bundle.is_file(), "beside the folder");
    h.get_by_label("Open bundle").click();
    h.run_steps(6);
    shot(&mut h, "4-bundle");
    assert!(h.query_by_label("Bundle assembled").is_none());
    let _ = std::fs::remove_dir_all(&root);
}
