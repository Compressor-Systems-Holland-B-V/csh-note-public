use std::any::Any;
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, hash128};
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::datetime::Time;
use typst_kit::downloader::{Downloader, SystemDownloader};
use typst_kit::files::{FileLoader, FileStore};
use typst_kit::fonts::{self, FontStore};
use typst_kit::packages::SystemPackages;
use typst_layout::PagedDocument;
use typst_svg::SvgOptions;

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

static FONTS: LazyLock<FontStore> = LazyLock::new(|| {
    let mut store = FontStore::new();
    store.extend(fonts::system());
    store.extend(fonts::embedded());
    store
});

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    severity: &'static str,
    message: String,
    from: usize,
    to: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedPage {
    index: usize,
    svg: String,
    width: f64,
    height: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compiled {
    page_count: Option<usize>,
    pages: Vec<RenderedPage>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompiledEvent<'a> {
    page: &'a str,
    seq: u64,
    #[serde(flatten)]
    result: &'a Compiled,
}

struct Announcing<F> {
    inner: SystemDownloader,
    announce: F,
}

impl<F: Fn(&str) + Send + Sync + 'static> Downloader for Announcing<F> {
    fn stream(&self, key: &dyn Any, url: &str) -> io::Result<(Option<usize>, Box<dyn Read>)> {
        if let Some(spec) = key.downcast_ref::<PackageSpec>() {
            (self.announce)(&spec.to_string());
        }
        self.inner.stream(key, url)
    }
}

struct Loader {
    main: FileId,
    text: String,
    packages: SystemPackages,
}

impl FileLoader for Loader {
    fn load(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            return Ok(Bytes::from_string(self.text.clone()));
        }
        match id.root() {
            VirtualRoot::Package(spec) => self.packages.obtain(spec)?.load(id.vpath()),
            VirtualRoot::Project => Err(FileError::NotFound(PathBuf::from(id.vpath().get_without_slash()))),
        }
    }
}

pub struct PageWorld {
    main: FileId,
    files: FileStore<Loader>,
    time: Time,
    hashes: Vec<u128>,
}

impl World for PageWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        FONTS.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONTS.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        self.time.today(offset)
    }
}

impl PageWorld {
    pub fn new(announce_download: impl Fn(&str) + Send + Sync + 'static) -> Result<Self, String> {
        let path = VirtualPath::new("main.typ").map_err(|error| error.to_string())?;
        let main = FileId::unique(RootedPath::new(VirtualRoot::Project, path));
        let downloader = Announcing {
            inner: SystemDownloader::new(concat!("znote/", env!("CARGO_PKG_VERSION"))),
            announce: announce_download,
        };
        let loader = Loader { main, text: String::new(), packages: SystemPackages::new(downloader) };
        Ok(Self { main, files: FileStore::new(loader), time: Time::system(), hashes: Vec::new() })
    }

    pub fn forget_sent_pages(&mut self) {
        self.hashes.clear();
    }

    pub fn compile(&mut self, text: String) -> Compiled {
        self.files.loader_mut().text = text;
        self.files.reset();
        self.time.reset();

        let warned = typst::compile::<PagedDocument>(self);
        let mut diagnostics: Vec<Diagnostic> = warned.warnings.iter().map(|d| self.diagnostic(d)).collect();

        let document = match warned.output {
            Ok(document) => document,
            Err(errors) => {
                diagnostics.extend(errors.iter().map(|d| self.diagnostic(d)));
                return Compiled { page_count: None, pages: Vec::new(), diagnostics };
            }
        };

        let hashes: Vec<u128> = document.pages().iter().map(hash128).collect();
        let pages = document
            .pages()
            .iter()
            .enumerate()
            .filter(|(index, _)| self.hashes.get(*index) != Some(&hashes[*index]))
            .map(|(index, page)| RenderedPage {
                index,
                svg: typst_svg::svg(page, &SvgOptions::default()),
                width: page.frame.width().to_pt(),
                height: page.frame.height().to_pt(),
            })
            .collect();
        self.hashes = hashes;
        Compiled { page_count: Some(self.hashes.len()), pages, diagnostics }
    }

    fn diagnostic(&self, diagnostic: &SourceDiagnostic) -> Diagnostic {
        let mut message = diagnostic.message.to_string();
        for hint in &diagnostic.hints {
            message.push_str("\nHint: ");
            message.push_str(&hint.v);
        }
        let (from, to) = self.utf16_range(diagnostic).unwrap_or((0, 0));
        Diagnostic {
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            message,
            from,
            to,
        }
    }

    fn utf16_range(&self, diagnostic: &SourceDiagnostic) -> Option<(usize, usize)> {
        if diagnostic.span.id()? != self.main {
            return None;
        }
        let range = self.range(diagnostic.span)?;
        let source = self.source(self.main).ok()?;
        let lines = source.lines();
        Some((lines.byte_to_utf16(range.start)?, lines.byte_to_utf16(range.end)?))
    }
}

#[derive(Default)]
pub struct Compiler {
    latest: Mutex<HashMap<String, u64>>,
    worlds: Mutex<HashMap<String, PageWorld>>,
}

fn compile_page(app: &AppHandle, page: &str, seq: u64, text: String, fresh: bool) -> Result<(), String> {
    let compiler = app.state::<Compiler>();
    let mut worlds = compiler.worlds.lock().map_err(|error| error.to_string())?;
    let superseded = compiler.latest.lock().map_err(|error| error.to_string())?.get(page) != Some(&seq);
    if superseded {
        return Ok(());
    }
    if !worlds.contains_key(page) {
        let announcer = app.clone();
        let world = PageWorld::new(move |package| {
            let _ = announcer.emit("downloading", package);
        })?;
        worlds.insert(page.to_string(), world);
    }
    let world = worlds.get_mut(page).ok_or("The page could not be opened.")?;
    if fresh {
        world.forget_sent_pages();
    }
    let result = world.compile(text);
    app.emit("compiled", CompiledEvent { page, seq, result: &result }).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn compile(
    app: AppHandle,
    compiler: State<Compiler>,
    page: String,
    seq: u64,
    text: String,
    fresh: bool,
) -> Result<(), String> {
    compiler.latest.lock().map_err(|error| error.to_string())?.insert(page.clone(), seq);
    std::thread::spawn(move || {
        if let Err(message) = compile_page(&app, &page, seq, text, fresh) {
            let _ = app.emit("compile-failed", message);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> PageWorld {
        PageWorld::new(|_| {}).unwrap()
    }

    #[test]
    fn valid_source_gives_its_pages() {
        let result = world().compile("One #pagebreak() Two #pagebreak() Three".into());
        assert_eq!(result.page_count, Some(3));
        assert_eq!(result.pages.len(), 3);
        assert!(result.pages[0].svg.starts_with("<svg"));
        assert!(result.pages[0].width > 0.0);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn invalid_source_gives_a_diagnostic_with_a_range() {
        let result = world().compile("é #nope".into());
        assert_eq!(result.page_count, None);
        assert_eq!(result.diagnostics.len(), 1);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.severity, "error");
        assert_eq!((diagnostic.from, diagnostic.to), (3, 7));
    }

    #[test]
    fn only_changed_pages_are_sent_again() {
        let mut world = world();
        world.compile("One #pagebreak() Two".into());
        let result = world.compile("One #pagebreak() Changed".into());
        assert_eq!(result.page_count, Some(2));
        assert_eq!(result.pages.iter().map(|page| page.index).collect::<Vec<_>>(), [1]);

        let unchanged = world.compile("One #pagebreak() Changed".into());
        assert_eq!(unchanged.page_count, Some(2));
        assert!(unchanged.pages.is_empty());

        world.forget_sent_pages();
        assert_eq!(world.compile("One #pagebreak() Changed".into()).pages.len(), 2);
    }
}
