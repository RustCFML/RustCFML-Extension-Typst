//! The `typst::World` a CFML caller implies.
//!
//! Typst compiles against a `World`: a standard library, a font book, a main
//! source file, and lookups for further sources and binary files. Ours is
//! deliberately the smallest honest one — a single in-memory main source and
//! the operating system's fonts. There is no project root and no package
//! resolution, so a document cannot reach the filesystem or the network by
//! `#import`-ing its way there. That is a security property, not an omission:
//! the markup being compiled is frequently assembled from application data.

use std::sync::{Arc, OnceLock};

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

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
        Fonts { book: LazyHash::new(book), faces }
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
}

impl CfmlWorld {
    pub fn new(markup: &str) -> Self {
        let id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("main.typ").expect("a literal filename is always valid"),
        ));
        Self {
            library: LazyHash::new(Library::default()),
            main: Source::new(id, markup.to_string()),
        }
    }
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
            Ok(self.main.clone())
        } else {
            // No project root: `#import "other.typ"` cannot escape into the
            // filesystem. See the module note.
            Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
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

/// Compile Typst markup to PDF bytes.
pub fn compile_pdf(markup: &str) -> Result<Vec<u8>, String> {
    let world = CfmlWorld::new(markup);
    let compiled = typst::compile::<typst_layout::PagedDocument>(&world);
    let doc = compiled.output.map_err(|diags| {
        let mut out = String::from("Typst could not compile this document:");
        for d in diags.iter().take(5) {
            out.push_str("\n  - ");
            out.push_str(&d.message);
        }
        if font_count() == 0 {
            out.push_str(
                "\n  (no system fonts were found on this host — Typst cannot lay out text \
                 without at least one font family)",
            );
        }
        out
    })?;
    typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|e| format!("Typst compiled the document but could not write the PDF: {:?}", e))
}

/// Page count without exporting, for callers that only want to know.
pub fn page_count(markup: &str) -> Result<usize, String> {
    let world = CfmlWorld::new(markup);
    typst::compile::<typst_layout::PagedDocument>(&world)
        .output
        .map(|d| d.pages().len())
        .map_err(|d| {
            d.iter().next().map(|x| x.message.to_string()).unwrap_or_else(|| "compile failed".into())
        })
}

/// Keep the unused-import lint honest about `Arc` while the module is a stub.
#[allow(dead_code)]
fn _arc_is_used(_: Arc<()>) {}
