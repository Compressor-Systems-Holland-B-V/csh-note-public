use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use typst_kit::fonts::{self, FontStore};
use zsuite_typst::{Compiled, PageWorld, Problem, font_store};

use crate::theme;
use crate::workspace::Workspace;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompiledEvent<'a> {
    page: &'a str,
    seq: u64,
    #[serde(flatten)]
    result: &'a Compiled,
}

#[derive(Default)]
pub struct Compiler {
    latest: Mutex<HashMap<String, u64>>,
    world: Mutex<Option<(PathBuf, PageWorld)>>,
    fonts: Mutex<Option<Arc<FontStore>>>,
}

impl Compiler {
    pub fn forget_themes(&self) -> Result<(), String> {
        *self.world.lock().map_err(|error| error.to_string())? = None;
        *self.fonts.lock().map_err(|error| error.to_string())? = None;
        Ok(())
    }
}

fn new_world(app: &AppHandle, project: &Path, main: &str) -> Result<PageWorld, String> {
    let announcer = app.clone();
    let themes = theme::folder(app)?;
    let fonts = app
        .state::<Compiler>()
        .fonts
        .lock()
        .map_err(|error| error.to_string())?
        .get_or_insert_with(|| {
            let mut store = font_store(&themes);
            if let Some(carried) = crate::project::fonts(app) {
                store.extend(fonts::scan(&carried));
            }
            Arc::new(store)
        })
        .clone();
    PageWorld::new(project, main, &themes, fonts, move |package| {
        let _ = announcer.emit("downloading", package);
    })
}

/// The errors on a page the assistant changed, from a world of its own so the preview's is left
/// alone. A page that cannot be checked counts as one without errors.
pub fn problems(app: &AppHandle, page: &str, text: String) -> Vec<Problem> {
    let check = || {
        let (project, main) = app.state::<Workspace>().typst_source(page)?;
        let mut world = new_world(app, &project, &main)?;
        crate::guarded("", || Ok(world.problems(text)))
    };
    check().unwrap_or_default()
}

/// Pictures of a page drawn from its hidden text, for the assistant's design mode, from a world of
/// its own like `problems`.
pub fn preview(
    app: &AppHandle,
    page: &str,
    text: String,
    paths: HashMap<String, String>,
) -> Result<Vec<Vec<u8>>, String> {
    let (project, main) = app.state::<Workspace>().typst_source(page)?;
    let mut world = new_world(app, &project, &main)?;
    crate::guarded("This page could not be drawn.", || {
        world.preview(text, paths)
    })
}

type OpenWorld = Option<(PathBuf, PageWorld)>;

fn open_world<'a>(
    app: &AppHandle,
    open: &'a mut OpenWorld,
    page: &str,
) -> Result<&'a mut PageWorld, String> {
    let (project, main) = app.state::<Workspace>().typst_source(page)?;
    let file = project.join(&main);
    if open.as_ref().is_none_or(|(current, _)| *current != file) {
        *open = Some((file, new_world(app, &project, &main)?));
    }
    open.as_mut()
        .map(|(_, world)| world)
        .ok_or_else(|| "The page could not be opened.".to_string())
}

/// Runs `work` on the page's world, which stays locked for the whole call.
pub fn with_page<T>(
    app: &AppHandle,
    page: &str,
    work: impl FnOnce(&mut PageWorld) -> Result<T, String>,
) -> Result<T, String> {
    let compiler = app.state::<Compiler>();
    let mut open = compiler.world.lock().map_err(|error| error.to_string())?;
    let world = open_world(app, &mut open, page)?;
    crate::guarded(
        "This page could not be read. Undo your last change and try again.",
        || work(world),
    )
}

fn compile_page(
    app: &AppHandle,
    page: &str,
    seq: u64,
    text: String,
    fresh: bool,
) -> Result<(), String> {
    let compiler = app.state::<Compiler>();
    let mut open = compiler.world.lock().map_err(|error| error.to_string())?;
    let superseded = compiler
        .latest
        .lock()
        .map_err(|error| error.to_string())?
        .get(page)
        != Some(&seq);
    if superseded {
        return Ok(());
    }
    let world = open_world(app, &mut open, page)?;
    if fresh {
        world.forget_sent_pages();
    }
    let result = crate::guarded(
        "This page could not be read. Undo your last change and try again.",
        || Ok(world.compile(text)),
    )?;
    if result.page_count.is_some() {
        crate::project::fonts_used(app, page, world.used_fonts());
    }
    app.emit(
        "compiled",
        CompiledEvent {
            page,
            seq,
            result: &result,
        },
    )
    .map_err(|error| error.to_string())
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
    compiler
        .latest
        .lock()
        .map_err(|error| error.to_string())?
        .insert(page.clone(), seq);
    std::thread::spawn(move || {
        if let Err(message) = compile_page(&app, &page, seq, text, fresh) {
            let _ = app.emit("compile-failed", message);
        }
    });
    Ok(())
}
