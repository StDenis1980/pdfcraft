//! Tinta Office fork: file types another program converts to PDF.
//!
//! The engine reads PDFs, images and plain text itself. Word, Excel and PowerPoint files are
//! converted by the suite's own apps (Tinta Texto, Calculo and Presenta), which the engine can't
//! reach: they are found and run by `pdfcraft-platform`, a layer above. The front end (the app,
//! the command line) registers that converter once at start-up, and from then on every path that
//! turns a file into a PDF ([`crate::Session::convert_to_pdf`]: Create, Combine, Insert pages,
//! Create from multiple files, batch processing) accepts those files too.

use std::sync::RwLock;

/// A converter for files the engine can't read itself.
#[derive(Clone, Copy, Debug)]
pub struct DocumentConverter {
    /// Lower-case file extensions it converts (without the dot).
    pub extensions: &'static [&'static str],
    /// Convert a file (its name and bytes) to PDF bytes, or say why not, in words for the user.
    pub convert: fn(&str, &[u8]) -> Result<Vec<u8>, String>,
}

static CONVERTER: RwLock<Option<DocumentConverter>> = RwLock::new(None);

fn current() -> Option<DocumentConverter> {
    // The value is a plain copy, so a poisoned lock still holds a whole one.
    *CONVERTER.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Register the converter. One process has one set of sibling apps, but the app and its
/// automation tools both register one (the app's speaks the user's language): with `replace`
/// this one takes over, without it it is used only when none is registered yet. Returns whether
/// it was taken.
pub fn set_document_converter(converter: DocumentConverter, replace: bool) -> bool {
    let mut slot = CONVERTER.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_some() && !replace {
        return false;
    }
    *slot = Some(converter);
    true
}

/// The converter for a file with this name, if one is registered and takes its extension.
pub(crate) fn for_name(name: &str) -> Option<DocumentConverter> {
    let converter = current()?;
    let (_, ext) = name.rsplit_once('.')?;
    converter.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)).then_some(converter)
}

/// Every extension a file can have to be made into a PDF: images and text
/// ([`pdfcraft_create::CONVERTIBLE`]) and those of the registered converter.
pub fn convertible_extensions() -> Vec<&'static str> {
    let external = current().map_or(&[][..], |c| c.extensions);
    pdfcraft_create::CONVERTIBLE.iter().copied().chain(external.iter().copied()).collect()
}
