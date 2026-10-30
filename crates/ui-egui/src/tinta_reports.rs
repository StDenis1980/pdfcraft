//! Tinta Office: automatic error reports, sent only with the person's consent.
//!
//! The first start of a build that can send them asks once ([`first_start`]); Preferences changes
//! the answer, shows the installation id and asks the server to delete this installation's
//! reports ([`preferences_section`]). The choice is saved with the app's settings
//! ([`ReportsPrefs`]). The desktop app does the rest through [`Hooks`]
//! (`apps/pdfcraft/src/tinta_report.rs`: writing, scrubbing and sending to
//! report.tintaoffice.com); without hooks (upstream builds, tests, the web) nothing asks or sends.

use egui::{RichText, Ui};

use crate::PdfCraftApp;

/// The saved choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportsPrefs {
    /// `None` until the person answered the first-start question.
    pub consent: Option<bool>,
    /// The random id reports are filed under (made by the desktop app when first allowed).
    pub install_id: String,
    /// An id whose reports the server should delete; cleared once the server confirmed.
    pub forget_pending: Option<String>,
}

/// A valid installation id: the lowercase 8-4-4-4-12 hex form.
pub fn id_ok(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

impl ReportsPrefs {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"consent": self.consent, "install_id": self.install_id, "forget_pending": self.forget_pending})
    }

    /// Lenient: anything malformed reads as "not asked yet" or no id.
    pub fn from_json(v: &serde_json::Value) -> Self {
        let id = |k: &str| v[k].as_str().filter(|s| id_ok(s)).map(str::to_owned);
        ReportsPrefs { consent: v["consent"].as_bool(), install_id: id("install_id").unwrap_or_default(), forget_pending: id("forget_pending") }
    }

    /// Delete this installation's reports from the server; later reports go under a new id.
    pub fn forget(&mut self) {
        if !self.install_id.is_empty() {
            self.forget_pending = Some(std::mem::take(&mut self.install_id));
        }
    }
}

/// Each frame after `logic`: follow the choice (the interface language is the second argument).
pub type Tick = Box<dyn FnMut(&mut ReportsPrefs, &str)>;

/// What the desktop app plugs in.
pub struct Hooks {
    /// Where reports are kept, for "Open the reports folder".
    pub dir: std::path::PathBuf,
    /// The UI thread starts and ends a frame phase (the not-responding watchdog).
    pub busy: fn(),
    pub idle: fn(),
    /// Each frame after `logic`: follow the choice, make the id, start sending or deleting.
    pub tick: Tick,
}

/// Calls `idle` when a frame phase ends, whichever way it returns.
pub(crate) struct Busy(Option<fn()>);

impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(idle) = self.0 {
            idle();
        }
    }
}

pub(crate) fn busy(app: &PdfCraftApp) -> Busy {
    let hooks = app.tinta_hooks.as_ref();
    if let Some(h) = hooks {
        (h.busy)();
    }
    Busy(hooks.map(|h| h.idle))
}

pub(crate) fn tick(app: &mut PdfCraftApp) {
    let lang = crate::i18n::current().code().to_owned();
    if let Some(h) = app.tinta_hooks.as_mut() {
        (h.tick)(&mut app.tinta_reports, &lang);
    }
}

pub(crate) const TITLE: &str = "Send error reports automatically?";
pub(crate) const WHAT: &str = "When the app crashes or stops responding, it can send a report so the problem gets fixed without you writing to us. A report holds the app version, the system, the language, where in the code the error happened and the last lines of the technical log. Never the text of your documents, file names, folders, e-mail addresses or passwords.";
pub(crate) const RIGHTS: &str = "Reports go to Tinta Office (report.tintaoffice.com, stored in the EU) and are deleted after 90 days. You can change this at any time in Preferences and delete your reports from the server there. Details: tintaoffice.com/aviso-legal";
pub(crate) const YES: &str = "Yes, send reports";
pub(crate) const NO: &str = "No";
pub(crate) const SECTION: &str = "Error reports";
pub(crate) const CHECKBOX: &str = "Send error reports automatically";
pub(crate) const ID: &str = "Installation id: {id}";
pub(crate) const FORGET: &str = "Delete my reports from the server";
pub(crate) const PENDING: &str = "Deletion requested: it completes the next time the app is online.";
pub(crate) const FOLDER: &str = "Open the reports folder";

/// The question, over everything, until it is answered once.
pub(crate) fn first_start(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if app.tinta_hooks.is_none() || app.tinta_reports.consent.is_some() {
        return;
    }
    egui::Modal::new(egui::Id::unique("tinta-error-reports")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.label(RichText::new(tl!(TITLE)).strong().size(17.0));
        ui.add_space(8.0);
        ui.label(tl!(WHAT));
        ui.add_space(6.0);
        ui.label(RichText::new(tl!(RIGHTS)).small().weak());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(tl!(YES)).clicked() {
                app.tinta_reports.consent = Some(true);
            }
            if ui.button(tl!(NO)).clicked() {
                app.tinta_reports.consent = Some(false);
            }
        });
    });
}

/// The section in Preferences.
pub(crate) fn preferences_section(ui: &mut Ui, app: &mut PdfCraftApp) {
    let Some(dir) = app.tinta_hooks.as_ref().map(|h| h.dir.clone()) else { return };
    ui.add_space(8.0);
    ui.label(RichText::new(tl!(SECTION)).strong());
    let mut on = app.tinta_reports.consent == Some(true);
    if ui.add(crate::widgets::checkbox(&mut on, tl!(CHECKBOX))).changed() {
        app.tinta_reports.consent = Some(on);
    }
    ui.label(RichText::new(tl!(WHAT)).small().weak());
    ui.label(RichText::new(tl!(RIGHTS)).small().weak());
    let p = app.tinta_reports.clone();
    if !p.install_id.is_empty() {
        ui.label(RichText::new(tl!(ID).replace("{id}", &p.install_id)).small().monospace());
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!p.install_id.is_empty(), egui::Button::new(tl!(FORGET))).clicked() {
            app.tinta_reports.forget();
        }
        if ui.button(tl!(FOLDER)).clicked() {
            let _ = std::fs::create_dir_all(&dir);
            app.open_url(&format!("file://{}", dir.display()));
        }
    });
    if p.forget_pending.is_some() {
        ui.label(RichText::new(tl!(PENDING)).small().weak());
    }
}

#[cfg(test)]
pub(crate) const STRINGS: [&str; 11] = [TITLE, WHAT, RIGHTS, YES, NO, SECTION, CHECKBOX, ID, FORGET, PENDING, FOLDER];

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn hooks() -> Hooks {
        Hooks { dir: std::env::temp_dir().join("tinta-reports-test"), busy: || {}, idle: || {}, tick: Box::new(|_, _| {}) }
    }

    fn harness(app: PdfCraftApp, preferences: bool) -> egui_kittest::Harness<'static, PdfCraftApp> {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
            move |ui, app: &mut PdfCraftApp| {
                if preferences {
                    preferences_section(ui, app);
                } else {
                    first_start(app, &ui.ctx().clone());
                }
            },
            app,
        );
        h.run_steps(2);
        h
    }

    #[test]
    fn the_first_start_asks_once_and_never_without_hooks() {
        let mut app = PdfCraftApp::new();
        app.tinta_hooks = Some(hooks());
        let mut h = harness(app, false);
        let _ = h.get_by_label(TITLE);
        h.get_by_label(YES).click();
        h.run_steps(2);
        assert_eq!(h.state().tinta_reports.consent, Some(true));
        assert!(h.query_by_label(TITLE).is_none());
        let h = harness(PdfCraftApp::new(), false);
        assert!(h.query_by_label(TITLE).is_none());
    }

    #[test]
    fn preferences_turn_reports_off_and_ask_for_deletion() {
        let mut app = PdfCraftApp::new();
        app.tinta_hooks = Some(hooks());
        app.tinta_reports = ReportsPrefs { consent: Some(true), install_id: "0f8fad5b-d9cb-469f-a165-70867728950e".into(), forget_pending: None };
        let mut h = harness(app, true);
        h.get_by_label(CHECKBOX).click();
        h.run_steps(2);
        assert_eq!(h.state().tinta_reports.consent, Some(false));
        h.get_by_label(FORGET).click();
        h.run_steps(2);
        assert_eq!(h.state().tinta_reports.forget_pending.as_deref(), Some("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(h.state().tinta_reports.install_id.is_empty());
        let _ = h.get_by_label(PENDING);
    }

    #[test]
    fn the_choice_survives_a_restart_and_bad_values_read_as_not_asked() {
        let mut app = PdfCraftApp::new();
        app.tinta_reports = ReportsPrefs { consent: Some(false), install_id: "0f8fad5b-d9cb-469f-a165-70867728950e".into(), forget_pending: None };
        let mut b = PdfCraftApp::new();
        b.restore(&app.persist());
        assert_eq!(b.tinta_reports, app.tinta_reports);
        let bad = ReportsPrefs::from_json(&serde_json::json!({"consent": "yes", "install_id": "../x", "forget_pending": 3}));
        assert_eq!(bad, ReportsPrefs::default());
    }

    /// Every string is translated in the Tinta Office languages.
    #[test]
    fn every_string_is_translated() {
        for (lang, rows) in [
            ("es", include_str!("i18n/es.tsv")),
            ("ca", include_str!("i18n/ca.tsv")),
            ("gl", include_str!("i18n/gl.tsv")),
            ("eu", include_str!("i18n/eu.tsv")),
            ("uk", include_str!("i18n/uk.tsv")),
            ("fr", include_str!("i18n/fr.tsv")),
            ("de", include_str!("i18n/de.tsv")),
            ("it", include_str!("i18n/it.tsv")),
            ("pl", include_str!("i18n/pl.tsv")),
            ("pt-br", include_str!("i18n/pt-br.tsv")),
            ("ru", include_str!("i18n/ru.tsv")),
        ] {
            for s in STRINGS {
                let found = rows.lines().any(|l| {
                    let mut c = l.split('\t');
                    c.next() == Some("") && c.next() == Some(s) && c.next().is_some_and(|t| !t.trim().is_empty())
                });
                assert!(found, "{lang}: {s:?}");
            }
        }
    }
}
