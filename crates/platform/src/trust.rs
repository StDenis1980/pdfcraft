//! Tinta Office fork: the operating system's trusted root certificates, used (besides the user's
//! own trusted certificates) to validate signatures, so a signature from a Spanish qualified
//! certificate (FNMT, ACCV…) shows as valid the way it does in Acrobat and VALIDe.

use pdfcraft_sign::Certificate;

/// The system's root certificates: Windows' Trusted Root store; none elsewhere yet. A store that
/// can't be read gives none (validation then shows such signers as unknown, never fails).
pub fn system_roots() -> Vec<Certificate> {
    #[cfg(target_os = "windows")]
    {
        pdfcraft_sign::windows::root_certificates().unwrap_or_default()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}
