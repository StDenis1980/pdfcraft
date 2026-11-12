//! What each tool may do beyond the open documents, for agents' permission layers
//! (tintaoffice-web#26, Vilna OS's Herkos). `tools/list` carries it as `_meta.risk` on every
//! tool, and `command_list`, `tool_search` and `pdfcraft-cli tools` as `risk`.
//!
//! - `reads_document` / `changes_document`: the open PDFs (a tool that takes a `doc`, or lists
//!   them; `changes_document` is a tool that isn't read-only and isn't one that only writes a
//!   file from the document).
//! - `reads_disk` / `writes_disk`: files (open, insert, import, attach, certificates, fonts,
//!   OCR dictionaries; save, export, extract, split, reports). The automation roots and `--root`
//!   still hold every such path.
//! - `network`: anything that leaves the device (a timestamp server).
//! - `print`: the printer. `sign`: a certificate signature or the trust list.
//! - `other_program`: hands something to another program (Office files go to the suite's app
//!   that reads them, a link to the system).
//! - `runs_commands`: runs other tools, steps or scripts (each step through its own tool).
//! - `ui_only`: only how a window would show the document.
//!
//! The tables are reviewed by hand: a new tool changes `docs/tinta-mcp-tools.tsv`, whose test
//! fails until the snapshot is updated on purpose, which is the moment to add it here. An entry
//! that names no tool fails the test too.

use serde_json::{Value, json};

use crate::ToolDef;

/// Tools that read files from disk.
const READS_DISK: &[&str] = &[
    "doc_open",
    "doc_create",
    "doc_combine",
    "doc_create_multiple",
    "doc_batch",
    "doc_bates_multi",
    "attachment_add",
    "page_insert_file",
    "page_replace",
    "page_add_image",
    "bookmark_import",
    "stamp_custom",
    "model_add",
    "form_set_image",
    "form_merge_data",
    "doc_import_data",
    "content_update",
    "comment_add",
    "doc_watermark",
    "doc_background",
    "fill_sign_add",
    "sign_document",
    "sign_trust",
    "accessibility_review_load",
    "action_run",
    "ocr_recognize_files",
    "ocr_dictionary",
    "link_open",
    "text_font_files",
    "text_font_import",
];

/// Tools that write files.
const WRITES_DISK: &[&str] = &[
    "doc_save",
    "doc_combine",
    "doc_create_multiple",
    "doc_split",
    "doc_batch",
    "doc_bates_multi",
    "doc_reduce",
    "doc_optimize",
    "doc_print",
    "page_extract",
    "bookmark_export",
    "measure_export",
    "comments_summarize",
    "doc_export_data",
    "doc_export_images",
    "doc_export_all_images",
    "doc_export_office",
    "doc_export_text",
    "image_save",
    "form_merge_data",
    "accessibility_report",
    "accessibility_review_record",
    "accessibility_review_save",
    "accessibility_review_html",
    "doc_compare_report",
    "sign_id_create",
    "sign_document",
    "sign_timestamp",
    "sign_trust",
    "ocr_recognize",
    "ocr_review_apply",
    "ocr_recognize_files",
    "ocr_dictionary",
    "action_run",
];

/// Tools that may reach the network.
const NETWORK: &[&str] = &["sign_timestamp"];

/// Tools that print, or ask the system's print spooler.
const PRINT: &[&str] = &["doc_print", "printers", "printer_options"];

/// Tools that sign, or change which certificates are trusted.
const SIGN: &[&str] = &["sign_document", "sign_timestamp", "sign_id_create", "sign_trust"];

/// Tools that hand something to another program: Office files to the suite's app that converts
/// them, and the keychain or certificate store a digital ID can live in.
const OTHER_PROGRAM: &[&str] =
    &["doc_open", "doc_create", "doc_combine", "doc_create_multiple", "doc_batch", "page_insert_file", "sign_windows_ids", "sign_keychain_ids"];

/// Tools that run other tools, steps or scripts.
const RUNS_COMMANDS: &[&str] = &["command_run", "command_batch", "doc_batch", "action_run", "js_run", "form_set_script", "js_set_document_script"];

/// Tools that write a file from a document but leave the document as it was.
const KEEPS_DOCUMENT: &[&str] = &[
    "doc_save",
    "doc_optimize",
    "doc_split",
    "doc_reduce",
    "doc_print",
    "page_extract",
    "bookmark_export",
    "measure_export",
    "comments_summarize",
    "doc_export_data",
    "doc_export_images",
    "doc_export_all_images",
    "doc_export_office",
    "doc_export_text",
    "image_save",
    "accessibility_report",
    "accessibility_review_record",
    "accessibility_review_save",
    "accessibility_review_html",
    "doc_compare_report",
    "sign_document",
    "sign_timestamp",
    "form_merge_data",
];

/// Tools that only change how a window would show the document.
const UI_ONLY: &[&str] = &["view_reading_colors"];

fn named(table: &[&str], name: &str) -> bool {
    table.contains(&name)
}

/// The `risk` object of the tool `def`.
pub fn risk(def: &ToolDef) -> Value {
    let name = def.name;
    let ui_only = named(UI_ONLY, name);
    let props = def.input_schema.get("properties").and_then(Value::as_object);
    let takes_doc = props.is_some_and(|p| p.contains_key("doc") || p.contains_key("other"));
    let reads_document = !ui_only && (takes_doc || matches!(name, "doc_list" | "doc_inspect" | "render_preview"));
    json!({
        "reads_document": reads_document,
        "changes_document": reads_document && !def.read_only && !named(KEEPS_DOCUMENT, name),
        "reads_disk": named(READS_DISK, name),
        "writes_disk": named(WRITES_DISK, name),
        "network": named(NETWORK, name),
        "print": named(PRINT, name),
        "sign": named(SIGN, name),
        "other_program": named(OTHER_PROGRAM, name),
        "runs_commands": named(RUNS_COMMANDS, name),
        "ui_only": ui_only,
    })
}

/// One line per tool, sorted by name: the name, then the risk flags that are set
/// (`docs/tinta-mcp-tools.tsv`).
pub fn snapshot(tools: &[ToolDef]) -> String {
    let mut lines: Vec<String> = tools
        .iter()
        .map(|t| {
            let r = risk(t);
            let set: Vec<&str> =
                r.as_object().map(|o| o.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.as_str()).collect()).unwrap_or_default();
            format!("{}\t{}", t.name, set.join(","))
        })
        .collect();
    lines.sort();
    let mut out = String::from(
        "# Tinta PDF tools (pdfcraft-cli mcp) and what they may do (crates/automation/src/tool_risk.rs). Generated: TINTA_UPDATE_SNAPSHOT=1 cargo test -p pdfcraft-automation --lib tool_risk\n",
    );
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/tinta-mcp-tools.tsv");

    /// Tool names are what agents' permission manifests name (tintaoffice-web#33): a tool added,
    /// renamed or removed, or a risk flag changed, fails here until the snapshot is regenerated
    /// on purpose (and the change noted in the release notes).
    #[test]
    fn tool_names_and_risks_match_the_snapshot() {
        let now = snapshot(&crate::tools());
        if std::env::var_os("TINTA_UPDATE_SNAPSHOT").is_some() {
            std::fs::write(SNAPSHOT, &now).unwrap();
            return;
        }
        let saved = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
        let old: std::collections::BTreeSet<&str> = saved.lines().collect();
        let new: std::collections::BTreeSet<&str> = now.lines().collect();
        let gone: Vec<&&str> = old.difference(&new).collect();
        let added: Vec<&&str> = new.difference(&old).collect();
        assert!(
            gone.is_empty() && added.is_empty(),
            "the tools changed: gone {gone:?}, new {added:?}. Review tool_risk.rs for the new ones, then run \
             TINTA_UPDATE_SNAPSHOT=1 cargo test -p pdfcraft-automation --lib tool_risk"
        );
    }

    #[test]
    fn every_table_entry_names_a_tool() {
        let all = crate::tools();
        for table in [READS_DISK, WRITES_DISK, NETWORK, PRINT, SIGN, OTHER_PROGRAM, RUNS_COMMANDS, KEEPS_DOCUMENT, UI_ONLY] {
            for entry in table {
                assert!(all.iter().any(|t| t.name == *entry), "{entry} names no tool");
            }
        }
    }

    #[test]
    fn saving_printing_signing_and_editing_are_flagged() {
        let all = crate::tools();
        let flags = |name: &str| risk(all.iter().find(|t| t.name == name).unwrap());
        assert_eq!(flags("doc_save")["writes_disk"], true);
        assert_eq!(flags("doc_save")["changes_document"], false);
        assert_eq!(flags("doc_print")["print"], true);
        assert_eq!(flags("sign_document")["sign"], true);
        assert_eq!(flags("sign_timestamp")["network"], true);
        assert_eq!(flags("page_rotate")["changes_document"], true);
        assert_eq!(flags("page_rotate")["writes_disk"], false);
        assert_eq!(flags("text_extract")["changes_document"], false);
        assert_eq!(flags("text_extract")["reads_document"], true);
        assert_eq!(flags("js_run")["runs_commands"], true);
    }
}
