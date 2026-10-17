use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_zsuite_assistant::Host;
use zsuite_agent::Problem;
use zsuite_sealed::Knowledge;

use crate::workspace::Workspace;
use crate::{compile, knowledge, project};

pub const KEYS: &str = "znote assistant";

pub struct Assistant;

impl Host<Wry> for Assistant {
    fn root(&self, app: &AppHandle) -> Result<PathBuf, String> {
        app.state::<Workspace>().root()
    }

    fn chats(&self, app: &AppHandle) -> Result<PathBuf, String> {
        project::chats(app)
    }

    fn problems(&self, app: &AppHandle, page: &str, text: String) -> Vec<Problem> {
        compile::problems(app, page, text)
            .into_iter()
            .map(|problem| Problem {
                line: problem.line,
                message: problem.message,
            })
            .collect()
    }

    fn preview(
        &self,
        app: &AppHandle,
        page: &str,
        text: String,
        paths: HashMap<String, String>,
    ) -> Result<Vec<Vec<u8>>, String> {
        compile::preview(app, page, text, paths)
    }

    fn changed(&self, app: &AppHandle) {
        project::changed(app);
    }

    fn options_changed(&self, app: &AppHandle) {
        let options = app.state::<Knowledge>().options();
        if options.names {
            knowledge::models_ready(app, "names");
        }
        if options.knowledge {
            knowledge::models_ready(app, "search");
        }
    }
}

pub fn chat_path(app: &AppHandle, chat: &str) -> Result<PathBuf, String> {
    zsuite_agent::chat_folder(&project::chats(app)?, chat)
}
