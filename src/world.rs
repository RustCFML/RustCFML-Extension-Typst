//! The `typst::World` a CFML caller implies.
//!
//! Typst compiles against a `World`: a standard library, a font book, a main
//! source file, and lookups for further sources and binary files. There are two
//! shapes here, and the difference between them is the whole security story:
//!
//! * **generated** — the builder emitted the markup. There is no project root at
//!   all: `source()` serves one in-memory file and returns `NotFound` for
//!   everything else, so `#import` has nowhere to go, and `file()` serves only
//!   the images the builder registered by generated name. A document assembled
//!   out of application data cannot reach the filesystem.
//!
//! * **templated** — the caller named a `.typ` file. Now a root *must* exist,
//!   because a template's whole value is that it can `#import` a sibling and
//!   `json()` its data. The root is one canonical directory, and every lookup
//!   goes through `VirtualPath::resolve`, which is Typst's own root-relative
//!   resolver and cannot escape upward. Packages stay unresolvable in both
//!   shapes: `@preview/…` reaches the network, and nothing here should.
//!
//! Fonts come from the operating system in both shapes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::model::{PdfSettings, Template};

/// Everything a terminal needs in order to compile, gathered under the
/// document's lock in one go so nothing is read twice or half-read.
pub struct Job {
    /// Generated markup — `None` when a template supplies the main source.
    pub markup: Option<String>,
    pub template: Option<Template>,
    /// Virtual name → bytes: embedded images, and `data.json` when a caller
    /// passed `.data()`.
    pub files: Vec<(String, Vec<u8>)>,
    pub pdf: PdfSettings,
    /// Whether the document has any text to lay out. See `check_fonts`.
    pub has_text: bool,
}

/// Refuse to produce a document whose text cannot be drawn.
///
/// On a host with **no fonts installed**, Typst does not fail — it compiles
/// happily and writes a PDF containing zero text-showing operators and zero
/// embedded fonts. A blank page, successfully. Measured in a `rust:1-slim`
/// container: a one-paragraph document came out at 1,966 bytes with nothing in
/// it, and every one of this extension's 106 end-to-end checks passed.
///
/// That is a silent-empty failure, so it is an error here instead. A document
/// with no text at all — an image or a shape — is a legitimate thing to want
/// without fonts, so the check is conditional rather than absolute.
fn check_fonts(job: &Job) -> Result<(), String> {
    if job.has_text && font_count() == 0 {
        return Err(
            "no fonts are installed on this host, so the text in this document cannot be drawn — \
             Typst would produce a blank PDF rather than fail. Install a font package (on Debian \
             or Ubuntu, `fonts-dejavu-core` is enough) and check with typstFontCount()."
                .to_string(),
        );
    }
    Ok(())
}

/// The system font set, loaded exactly once per process.
///
/// Enumerating and parsing the host's fonts took ~2.1 s for 904 faces on the
/// development machine. Per call that would dominate everything else the module
/// does, so it is built on first use and shared thereafter.
struct Fonts {
    book: LazyHash<FontBook>,
    faces: Vec<Font>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut book = FontBook::new();
        let mut faces = Vec::new();
        for face in db.faces() {
            if let Some((data, index)) = db.with_face_data(face.id, |d, idx| (d.to_vec(), idx)) {
                if let Some(f) = Font::new(Bytes::new(data), index) {
                    book.push(f.info().clone());
                    faces.push(f);
                }
            }
        }
        Fonts {
            book: LazyHash::new(book),
            faces,
        }
    })
}

/// How many font faces the host offers. Exposed so a caller can tell "no fonts
/// installed" apart from "my markup was wrong".
pub fn font_count() -> usize {
    fonts().faces.len()
}

pub struct CfmlWorld {
    library: LazyHash<Library>,
    main: Source,
    /// Virtual name -> bytes, for images the builder embedded and the
    /// `data.json` a template reads. In the generated shape this is the ONLY way
    /// a document reaches binary data.
    files: Vec<(String, Bytes)>,
    /// The sandbox for a templated document. `None` in the generated shape,
    /// which is what makes `#import` impossible there.
    root: Option<PathBuf>,
    /// Sources read from the root. `World::source` takes `&self` and Typst may
    /// ask for the same file repeatedly, so the parse is memoised.
    sources: Mutex<HashMap<FileId, Source>>,
}

fn virtual_main() -> FileId {
    FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new("main.typ").expect("a literal filename is always valid"),
    ))
}

impl CfmlWorld {
    /// The generated shape: markup we produced, no root.
    pub fn generated(markup: &str, files: &[(String, Vec<u8>)]) -> Self {
        CfmlWorld {
            library: LazyHash::new(Library::default()),
            main: Source::new(virtual_main(), markup.to_string()),
            files: to_bytes(files),
            root: None,
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// The templated shape: a real file under a real root.
    pub fn templated(t: &Template, files: &[(String, Vec<u8>)]) -> Result<Self, String> {
        let vpath = VirtualPath::virtualize(&t.root, &t.main).map_err(|e| {
            format!(
                "the template [{}] cannot be expressed relative to the root [{}]: {:?}",
                t.main.display(),
                t.root.display(),
                e
            )
        })?;
        let text = std::fs::read_to_string(&t.main)
            .map_err(|e| format!("unable to read the template [{}]: {}", t.main.display(), e))?;
        Ok(CfmlWorld {
            library: LazyHash::new(Library::default()),
            main: Source::new(
                FileId::new(RootedPath::new(VirtualRoot::Project, vpath)),
                text,
            ),
            files: to_bytes(files),
            root: Some(t.root.clone()),
            sources: Mutex::new(HashMap::new()),
        })
    }

    /// A file id as a real path under the root, or an error explaining why not.
    ///
    /// Every rejection here is deliberate and every one of them has to say why:
    /// "file not found" for a document that reached outside its root would send
    /// the author looking in the wrong place.
    fn realize(&self, id: FileId) -> FileResult<PathBuf> {
        if matches!(id.get().root(), VirtualRoot::Package(_)) {
            return Err(FileError::Other(Some(
                "Typst packages (@preview/…) are not resolved by this extension — it never \
                 reaches the network. Vendor what you need under the template root and `#import` \
                 it by path."
                    .into(),
            )));
        }
        let Some(root) = &self.root else {
            return Err(FileError::Other(Some(
                "this document has no template root, so it cannot read other files. Use \
                 Document().template( path ) if the document should be able to #import."
                    .into(),
            )));
        };
        // Typst's own resolver. It normalises `..` and refuses to leave the
        // root, which is exactly the check that must not be hand-rolled.
        id.vpath()
            .realize(root)
            .map_err(|_| FileError::Other(Some("that path is outside the template root".into())))
    }
}

fn to_bytes(files: &[(String, Vec<u8>)]) -> Vec<(String, Bytes)> {
    files
        .iter()
        .map(|(name, bytes)| (name.clone(), Bytes::new(bytes.clone())))
        .collect()
}

impl World for CfmlWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().book
    }
    fn main(&self) -> FileId {
        self.main.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        if let Some(hit) = self.sources.lock().ok().and_then(|c| c.get(&id).cloned()) {
            return Ok(hit);
        }
        let path = self.realize(id)?;
        let text = std::fs::read_to_string(&path).map_err(|e| FileError::from_io(e, &path))?;
        let source = Source::new(id, text);
        if let Ok(mut cache) = self.sources.lock() {
            cache.insert(id, source.clone());
        }
        Ok(source)
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // Registered virtual files first: `data.json` and the embedded images
        // shadow anything on disk of the same name, so a template's data cannot
        // be swapped for a file that happens to sit beside it.
        let wanted = id.vpath().get_without_slash();
        if let Some((_, bytes)) = self.files.iter().find(|(name, _)| name == wanted) {
            return Ok(bytes.clone());
        }
        let path = self.realize(id)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|e| FileError::from_io(e, &path))
    }
    fn font(&self, index: usize) -> Option<Font> {
        fonts().faces.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // Left to the caller: a document that stamps "today" should be given the
        // date explicitly, so the same inputs always produce the same PDF.
        None
    }
}

fn build(job: &Job) -> Result<CfmlWorld, String> {
    match (&job.template, &job.markup) {
        (Some(t), _) => CfmlWorld::templated(t, &job.files),
        (None, Some(markup)) => Ok(CfmlWorld::generated(markup, &job.files)),
        (None, None) => Ok(CfmlWorld::generated("", &job.files)),
    }
}

/// Human-readable diagnostics from a failed compile.
fn diagnose(diags: &[typst::diag::SourceDiagnostic]) -> String {
    let mut out = String::from("Typst could not compile this document:");
    for d in diags.iter().take(5) {
        out.push_str("\n  - ");
        out.push_str(&d.message);
        for hint in d.hints.iter().take(2) {
            out.push_str("\n      hint: ");
            out.push_str(&hint.v);
        }
    }
    if font_count() == 0 {
        out.push_str(
            "\n  (no system fonts were found on this host — Typst cannot lay out text without at \
             least one font family)",
        );
    }
    out
}

/// Compile a job to PDF bytes.
pub fn compile_pdf(job: &Job) -> Result<Vec<u8>, String> {
    check_fonts(job)?;
    let world = build(job)?;
    let compiled = typst::compile::<typst_layout::PagedDocument>(&world);
    let doc = compiled.output.map_err(|diags| diagnose(&diags))?;
    let options = pdf_options(&job.pdf)?;
    typst_pdf::pdf(&doc, &options).map_err(|e| {
        format!(
            "Typst compiled the document but could not write the PDF: {:?}",
            e
        )
    })
}

/// Page count without exporting, for callers that only want to know.
pub fn page_count(job: &Job) -> Result<usize, String> {
    // A page count from a fontless layout is not merely blank, it is WRONG:
    // with no glyphs there is no line wrapping, so a long document reports one
    // page.
    check_fonts(job)?;
    let world = build(job)?;
    typst::compile::<typst_layout::PagedDocument>(&world)
        .output
        .map(|d| d.pages().len())
        .map_err(|diags| diagnose(&diags))
}

/// Compile raw markup — the `typstCompile()` surface, with no builder involved.
pub fn compile_markup(markup: &str) -> Result<Vec<u8>, String> {
    compile_pdf(&Job {
        markup: Some(markup.to_string()),
        template: None,
        files: Vec::new(),
        pdf: PdfSettings::default(),
        // Hand-written markup is assumed to have text; a caller writing raw
        // Typst to draw one rectangle can still see the real error.
        has_text: true,
    })
}

/// Page count for raw markup.
pub fn page_count_markup(markup: &str) -> Result<usize, String> {
    page_count(&Job {
        markup: Some(markup.to_string()),
        template: None,
        files: Vec::new(),
        pdf: PdfSettings::default(),
        has_text: true,
    })
}

/// The Typst version this extension is built against.
pub fn version() -> String {
    let v = typst::utils::version();
    format!("{}.{}.{}", v.major(), v.minor(), v.patch())
}

// ---------------------------------------------------------------------------
// PDF options
// ---------------------------------------------------------------------------

/// Every PDF standard name `.pdf( standard = … )` accepts.
///
/// Spelled out rather than deserialised so the error message can list them, and
/// so a Typst version bump that renames one fails to compile here instead of
/// silently accepting a name that no longer means anything.
const STANDARDS: &[(&str, typst_pdf::PdfStandard)] = {
    use typst_pdf::PdfStandard as S;
    &[
        ("1.4", S::V_1_4),
        ("1.5", S::V_1_5),
        ("1.6", S::V_1_6),
        ("1.7", S::V_1_7),
        ("2.0", S::V_2_0),
        ("a-1b", S::A_1b),
        ("a-1a", S::A_1a),
        ("a-2b", S::A_2b),
        ("a-2u", S::A_2u),
        ("a-2a", S::A_2a),
        ("a-3b", S::A_3b),
        ("a-3u", S::A_3u),
        ("a-3a", S::A_3a),
        ("a-4", S::A_4),
        ("a-4f", S::A_4f),
        ("a-4e", S::A_4e),
        ("ua-1", S::Ua_1),
    ]
};

/// Validate a standard name at the API boundary, so `.pdf()` fails where the
/// caller can see it rather than at the terminal three lines later.
pub fn check_pdf_standard(name: &str) -> Result<(), String> {
    if STANDARDS.iter().any(|(n, _)| *n == name) {
        return Ok(());
    }
    Err(format!(
        "unknown PDF standard [{}] — one of: {}",
        name,
        STANDARDS
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn pdf_options(settings: &PdfSettings) -> Result<typst_pdf::PdfOptions, String> {
    let mut options = typst_pdf::PdfOptions::default();

    if !settings.standards.is_empty() {
        let mut list = Vec::new();
        for name in &settings.standards {
            let (_, s) = STANDARDS
                .iter()
                .find(|(n, _)| n == name)
                .ok_or_else(|| check_pdf_standard(name).unwrap_err())?;
            list.push(*s);
        }
        // Typst refuses incompatible combinations (PDF/A-2b with PDF 2.0, say)
        // and says which; pass its own message through rather than paraphrase.
        options.standards = typst_pdf::PdfStandards::new(&list)
            .map_err(|e| format!("these PDF standards cannot be combined: {}", e.message()))?;
    }
    if let Some(tagged) = settings.tagged {
        options.tagged = tagged;
    }
    if let Some(spec) = &settings.page_ranges {
        options.page_ranges = Some(parse_page_ranges(spec)?);
    }
    if let Some(c) = &settings.creator {
        options.creator = typst::foundations::Smart::Custom(Some(c.clone()));
    }
    if let Some(epoch) = settings.timestamp {
        options.timestamp = Some(typst_pdf::Timestamp::new_utc(datetime_from_epoch(epoch)?));
    }
    Ok(options)
}

/// `"1-3,7,10-"` → Typst page ranges. One-indexed and inclusive, as the Typst
/// CLI's `--pages` is.
fn parse_page_ranges(spec: &str) -> Result<typst::layout::PageRanges, String> {
    use std::num::NonZeroUsize;
    let bound = |s: &str| -> Result<Option<NonZeroUsize>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        s.parse::<usize>()
            .ok()
            .and_then(NonZeroUsize::new)
            .map(Some)
            .ok_or_else(|| format!("[{}] is not a page number — pages start at 1", s))
    };
    let mut ranges = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((from, to)) => ranges.push(bound(from)?..=bound(to)?),
            None => {
                let one = bound(part)?;
                ranges.push(one..=one);
            }
        }
    }
    if ranges.is_empty() {
        return Err(format!("[{}] names no pages", spec));
    }
    Ok(typst::layout::PageRanges::new(ranges))
}

/// Seconds since the Unix epoch as a civil date.
///
/// Hinnant's `civil_from_days`, which is exact and needs no dependency — the
/// alternative is pulling `chrono` into an extension for one conversion.
fn datetime_from_epoch(epoch: i64) -> Result<Datetime, String> {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    Datetime::from_ymd_hms(
        year as i32,
        m as u8,
        d as u8,
        (secs / 3_600) as u8,
        ((secs % 3_600) / 60) as u8,
        (secs % 60) as u8,
    )
    .ok_or_else(|| format!("[{}] is not a representable date", epoch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_the_unix_epoch() {
        // A wrong civil-from-days is the kind of thing that produces a plausible
        // date, so this checks a known boundary and a known leap year.
        let d = datetime_from_epoch(0).unwrap();
        assert_eq!(
            d.display(typst::foundations::Smart::Auto)
                .unwrap()
                .to_string(),
            "1970-01-01 00:00:00"
        );
        let d = datetime_from_epoch(951_782_400).unwrap(); // 2000-02-29
        assert_eq!(
            d.display(typst::foundations::Smart::Auto)
                .unwrap()
                .to_string(),
            "2000-02-29 00:00:00"
        );
        // Cross-checked against Python's `datetime.utcfromtimestamp`, not
        // against this function's own output.
        let d = datetime_from_epoch(1_774_000_000).unwrap();
        assert_eq!(
            d.display(typst::foundations::Smart::Auto)
                .unwrap()
                .to_string(),
            "2026-03-20 09:46:40"
        );
    }

    #[test]
    fn page_ranges_are_one_indexed_and_open_ended() {
        assert!(parse_page_ranges("1-3,7,10-").is_ok());
        assert!(parse_page_ranges("0").is_err(), "pages start at 1");
        assert!(parse_page_ranges("x").is_err());
        assert!(parse_page_ranges("").is_err());
    }

    #[test]
    fn standard_names_are_checked_against_the_list_we_can_actually_emit() {
        assert!(check_pdf_standard("a-2b").is_ok());
        assert!(check_pdf_standard("ua-1").is_ok());
        assert!(
            check_pdf_standard("A-2B").is_err(),
            "lower-cased by the caller"
        );
        assert!(check_pdf_standard("pdf/a").is_err());
    }

    #[test]
    fn a_generated_document_cannot_read_anything() {
        // The property the generated shape exists for. `realize` is the only
        // route to the filesystem and it refuses outright with no root.
        let w = CfmlWorld::generated("hello", &[]);
        let id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("secrets.typ").unwrap(),
        ));
        assert!(w.source(id).is_err());
        assert!(w.file(id).is_err());
    }

    #[test]
    fn a_registered_virtual_file_shadows_the_disk() {
        let w = CfmlWorld::generated("x", &[("data.json".into(), b"{\"a\":1}".to_vec())]);
        let id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("data.json").unwrap(),
        ));
        assert_eq!(&*w.file(id).unwrap(), b"{\"a\":1}");
    }
}
