//! Tinta Office fork: Create PDF from Word, Excel and PowerPoint files (and their OpenDocument
//! siblings). PdfCraft does not read Office formats itself; the suite's other apps do, and each
//! ships a command-line tool that converts to PDF with its own layout engine: Tinta Texto
//! (`wordcraft-cli`), Tinta Calculo (`gridcraft-cli`) and Tinta Presenta (`deckcraft-cli`).
//! This finds the one a file needs and runs `<tool> convert <in> <out.pdf>` in a folder of its
//! own, with a time limit, and hands back the PDF.
//!
//! Nothing leaves the machine: the tools are local programs, given a copy of the file.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The longest a conversion may take before it is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(120);
/// The largest PDF read back.
const MAX_PDF: u64 = 1 << 30;
/// The most of a tool's error output kept for the message.
const MAX_ERROR: u64 = 4096;

/// Which app of the suite converts a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeApp {
    /// Tinta Texto: Word and OpenDocument text.
    Texto,
    /// Tinta Calculo: Excel and OpenDocument spreadsheets.
    Calculo,
    /// Tinta Presenta: PowerPoint.
    Presenta,
}

/// The file extensions each app converts (lower case).
pub const EXTENSIONS: &[(OfficeApp, &[&str])] = &[
    (OfficeApp::Texto, &["docx", "docm", "dotx", "dotm", "doc", "dot", "odt", "rtf"]),
    (OfficeApp::Calculo, &["xlsx", "xlsm", "xltx", "xltm", "xls", "xlt", "xlsb", "ods"]),
    (OfficeApp::Presenta, &["pptx", "potx", "ppsx"]),
];

impl OfficeApp {
    /// The app that converts a file named `name`, by its extension.
    pub fn for_name(name: &str) -> Option<OfficeApp> {
        let (_, ext) = name.rsplit_once('.')?;
        let ext = ext.to_ascii_lowercase();
        EXTENSIONS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(app, _)| *app)
    }

    /// The app's name, as the user knows it.
    pub fn name(self) -> &'static str {
        match self {
            OfficeApp::Texto => "Tinta Texto",
            OfficeApp::Calculo => "Tinta Calculo",
            OfficeApp::Presenta => "Tinta Presenta",
        }
    }

    /// The app's command-line tool, without the `.exe`.
    fn tool(self) -> &'static str {
        match self {
            OfficeApp::Texto => "wordcraft-cli",
            OfficeApp::Calculo => "gridcraft-cli",
            OfficeApp::Presenta => "deckcraft-cli",
        }
    }

    /// The environment variable that names the tool's path, ahead of every other place.
    pub fn env_var(self) -> &'static str {
        match self {
            OfficeApp::Texto => "TINTA_TEXTO_CLI",
            OfficeApp::Calculo => "TINTA_CALCULO_CLI",
            OfficeApp::Presenta => "TINTA_PRESENTA_CLI",
        }
    }
}

/// Every Office extension converted, for file pickers.
pub fn all_extensions() -> Vec<&'static str> {
    EXTENSIONS.iter().flat_map(|(_, exts)| exts.iter().copied()).collect()
}

/// Why a conversion failed.
#[derive(Debug, PartialEq, Eq)]
pub enum OfficeError {
    /// The app that converts this kind of file is not installed.
    NotInstalled(OfficeApp),
    /// The tool ran and failed; its own message (possibly empty).
    Failed(OfficeApp, String),
    /// The tool took longer than [`TIMEOUT`] and was stopped.
    TimedOut(OfficeApp),
    /// The file is not one of [`EXTENSIONS`].
    Unsupported(String),
    /// A local file problem (the temporary folder, reading the result).
    Io(String),
}

impl std::fmt::Display for OfficeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OfficeError::NotInstalled(app) => write!(f, "this needs {}, which is not installed; install it from tintaoffice.com", app.name()),
            OfficeError::Failed(app, why) if why.is_empty() => write!(f, "{} could not convert it", app.name()),
            OfficeError::Failed(app, why) => write!(f, "{} could not convert it: {why}", app.name()),
            OfficeError::TimedOut(app) => write!(f, "{} took longer than {} seconds and was stopped", app.name(), TIMEOUT.as_secs()),
            OfficeError::Unsupported(name) => write!(f, "{name}: not a Word, Excel or PowerPoint file"),
            OfficeError::Io(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for OfficeError {}

/// The tool's file name on this system.
fn tool_file(app: OfficeApp) -> String {
    format!("{}{}", app.tool(), std::env::consts::EXE_SUFFIX)
}

/// Where the tool of `app` may be, most specific first: the environment variable, beside this
/// program (one folder for the Linux packages and the portable zips unpacked together), the
/// app's own folder next to this one's (`…\Tinta Office\Tinta Texto\` on Windows, for machine and
/// per-user installs), and every folder on `PATH`.
fn candidates(app: OfficeApp, exe: Option<&Path>, path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let file = tool_file(app);
    let mut out = Vec::new();
    if let Some(dir) = exe.and_then(Path::parent) {
        out.push(dir.join(&file));
        if let Some(suite) = dir.parent() {
            out.push(suite.join(app.name()).join(&file));
        }
    }
    if let Some(path) = path {
        out.extend(std::env::split_paths(path).map(|dir| dir.join(&file)));
    }
    out
}

/// The tool that converts for `app`, if it is installed.
pub fn find(app: OfficeApp) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(app.env_var()).map(PathBuf::from) {
        // An explicit choice is kept even when it is wrong, so the error names it.
        return Some(p);
    }
    let exe = std::env::current_exe().ok();
    candidates(app, exe.as_deref(), std::env::var_os("PATH").as_deref()).into_iter().find(|p| p.is_file())
}

/// A folder of our own under the system's temporary folder, removed when dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<WorkDir, OfficeError> {
        let base = std::env::temp_dir();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        for i in 0..100u32 {
            let dir = base.join(format!("tinta-office-{}-{stamp}-{i}", std::process::id()));
            // `create_dir` fails when the name exists, so the folder is always a fresh one.
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(WorkDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(OfficeError::Io(format!("could not make a temporary folder: {e}"))),
            }
        }
        Err(OfficeError::Io("could not make a temporary folder".into()))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Convert the Office file `bytes`, named `name`, to PDF with the suite app that reads it.
pub fn convert(name: &str, bytes: &[u8]) -> Result<Vec<u8>, OfficeError> {
    let app = OfficeApp::for_name(name).ok_or_else(|| OfficeError::Unsupported(name.to_string()))?;
    let tool = find(app).ok_or(OfficeError::NotInstalled(app))?;
    convert_with(&tool, app, name, bytes, TIMEOUT)
}

/// [`convert`] with a given tool and time limit.
pub fn convert_with(tool: &Path, app: OfficeApp, name: &str, bytes: &[u8], timeout: Duration) -> Result<Vec<u8>, OfficeError> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).ok_or_else(|| OfficeError::Unsupported(name.to_string()))?;
    let work = WorkDir::new()?;
    // Plain names: the user's file name never reaches the tool's command line.
    let input = work.0.join(format!("document.{ext}"));
    let output = work.0.join("document.pdf");
    std::fs::write(&input, bytes).map_err(|e| OfficeError::Io(format!("could not copy the file for {}: {e}", app.name())))?;
    let mut command = Command::new(tool);
    command.arg("convert").arg(&input).arg(&output).current_dir(&work.0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // No console window flashes up over the app (CREATE_NO_WINDOW).
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => OfficeError::NotInstalled(app),
        _ => OfficeError::Failed(app, e.to_string()),
    })?;
    // The error output is read as it comes, so a chatty tool never blocks on a full pipe.
    let errors = child.stderr.take().map(|pipe| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let mut pipe = pipe;
            let _ = (&mut pipe).take(MAX_ERROR).read_to_string(&mut text);
            // Drain the rest unread.
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            text
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OfficeError::TimedOut(app));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(OfficeError::Failed(app, e.to_string())),
        }
    };
    let message = errors.and_then(|t| t.join().ok()).unwrap_or_default();
    // The last line says why (earlier ones are progress).
    let message = message.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
    if !status.success() {
        return Err(OfficeError::Failed(app, message));
    }
    let file = std::fs::File::open(&output).map_err(|_| OfficeError::Failed(app, message.clone()))?;
    let mut pdf = Vec::new();
    file.take(MAX_PDF.saturating_add(1)).read_to_end(&mut pdf).map_err(|e| OfficeError::Io(format!("could not read the converted PDF: {e}")))?;
    if pdf.len() as u64 > MAX_PDF {
        return Err(OfficeError::Failed(app, "the PDF is larger than 1 GiB".into()));
    }
    if !pdf.starts_with(b"%PDF-") {
        return Err(OfficeError::Failed(app, "it did not write a PDF".into()));
    }
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_office_file_goes_to_the_app_that_reads_it() {
        assert_eq!(OfficeApp::for_name("Factura.DOCX"), Some(OfficeApp::Texto));
        assert_eq!(OfficeApp::for_name("carta.odt"), Some(OfficeApp::Texto));
        assert_eq!(OfficeApp::for_name("a.b.xlsx"), Some(OfficeApp::Calculo));
        assert_eq!(OfficeApp::for_name("hoja.ods"), Some(OfficeApp::Calculo));
        assert_eq!(OfficeApp::for_name("deck.pptx"), Some(OfficeApp::Presenta));
        for other in ["scan.pdf", "photo.jpg", "notes.txt", "docx", "", "a.", "a.zip"] {
            assert_eq!(OfficeApp::for_name(other), None, "{other:?}");
        }
        assert!(all_extensions().contains(&"ppsx"));
    }

    #[test]
    fn the_tool_is_looked_for_beside_this_program_in_its_own_app_folder_and_on_the_path() {
        let exe = Path::new("/suite/Tinta PDF/pdfcraft");
        let path = std::env::join_paths([Path::new("/bin"), Path::new("/usr/local/bin")]).unwrap();
        let file = tool_file(OfficeApp::Calculo);
        assert_eq!(
            candidates(OfficeApp::Calculo, Some(exe), Some(&path)),
            vec![
                PathBuf::from("/suite/Tinta PDF").join(&file),
                PathBuf::from("/suite/Tinta Calculo").join(&file),
                PathBuf::from("/bin").join(&file),
                PathBuf::from("/usr/local/bin").join(&file),
            ]
        );
        assert!(candidates(OfficeApp::Texto, None, None).is_empty());
    }

    #[test]
    fn a_missing_tool_says_which_app_to_install() {
        let missing = std::env::temp_dir().join("tinta-office-no-such-tool");
        let e = convert_with(&missing, OfficeApp::Presenta, "deck.pptx", b"PK", TIMEOUT).unwrap_err();
        assert_eq!(e, OfficeError::NotInstalled(OfficeApp::Presenta));
        assert!(e.to_string().contains("Tinta Presenta"), "{e}");
        assert!(matches!(convert("photo.png", b""), Err(OfficeError::Unsupported(_))));
    }

    /// A stand-in tool: a shell script with `body` (Unix only; the suite's tools are tested in
    /// their own repositories).
    #[cfg(unix)]
    fn script(body: &str) -> (WorkDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = WorkDir::new().unwrap();
        let tool = dir.0.join("fake-cli");
        std::fs::write(&tool, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, tool)
    }

    #[cfg(unix)]
    #[test]
    fn the_tool_converts_a_copy_and_its_pdf_comes_back() {
        // `$1` is "convert"; the copy keeps the extension and the tool's PDF is read back.
        let (_dir, tool) = script(r#"[ "$1" = convert ] && case "$2" in *document.docx) cat "$2" > "$3";; esac"#);
        let pdf = convert_with(&tool, OfficeApp::Texto, "Mi carta.DOCX", b"%PDF-1.7 converted", TIMEOUT).unwrap();
        assert_eq!(pdf, b"%PDF-1.7 converted");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_tool_gives_its_own_last_message() {
        let (_dir, tool) = script("echo 'reading…' >&2; echo 'the file is damaged' >&2; exit 3");
        let e = convert_with(&tool, OfficeApp::Calculo, "a.xlsx", b"PK", TIMEOUT).unwrap_err();
        assert_eq!(e, OfficeError::Failed(OfficeApp::Calculo, "the file is damaged".into()));
        // A tool that succeeds without writing a PDF fails too.
        let (_dir, tool) = script(r#"echo hello > "$3""#);
        let e = convert_with(&tool, OfficeApp::Calculo, "a.xlsx", b"PK", TIMEOUT).unwrap_err();
        assert_eq!(e, OfficeError::Failed(OfficeApp::Calculo, "it did not write a PDF".into()));
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_hangs_is_stopped() {
        let (_dir, tool) = script("sleep 30");
        let started = Instant::now();
        let e = convert_with(&tool, OfficeApp::Presenta, "a.pptx", b"PK", Duration::from_millis(300)).unwrap_err();
        assert_eq!(e, OfficeError::TimedOut(OfficeApp::Presenta));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
