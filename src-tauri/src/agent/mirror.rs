use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Event, keep, level_of};
use crate::extract;
use crate::knowledge::{Gate, LIGHT, hash};
use crate::workspace;

const STATE: &str = "state.json";
const ATTACHMENTS: &str = ".attachments";
const CHAT_FILES: [&str; 4] = ["chat.json", "history.json", "system.txt", "veil.txt"];

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
struct Copy {
    real: String,
    veiled: String,
    #[serde(default)]
    level: u8,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    knowledge: Option<String>,
    #[serde(default)]
    level: u8,
    pages: BTreeMap<String, Copy>,
}

pub struct Mirror {
    pub root: PathBuf,
    pub attachments: PathBuf,
    state: State,
    renamed: Vec<(String, String)>,
}

fn failed(error: impl std::fmt::Display) -> String {
    format!("The hidden copy for the assistant could not be made: {error}")
}

fn page_path(root: &Path, page: &str) -> Option<PathBuf> {
    let names: Vec<&str> = page.split('/').collect();
    if names.len() != 3
        || names
            .iter()
            .any(|name| workspace::check_name(name).is_err())
    {
        return None;
    }
    let mut file = root.to_path_buf();
    names.iter().for_each(|name| file.push(name));
    file.as_mut_os_string().push(".typ");
    Some(file)
}

fn mirror_pages(root: &Path) -> Vec<String> {
    workspace::pages(root)
}

fn write(file: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    keep(file, text.as_bytes()).map_err(failed)
}

impl Mirror {
    pub fn build(
        root: PathBuf,
        workspace: &Path,
        chat_folder: &Path,
        chat: &str,
        gate: &Gate,
    ) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(failed)?;
        let mut state: State = fs::read(root.join(STATE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let knowledge = gate.id();
        if state.knowledge != knowledge || state.level != LIGHT {
            state = State {
                knowledge,
                level: LIGHT,
                pages: BTreeMap::new(),
            };
        }
        let pages: BTreeSet<String> = workspace::pages(workspace).into_iter().collect();
        for page in &pages {
            let real = workspace::read_text(workspace, page)?;
            let Some(file) = page_path(&root, page) else {
                continue;
            };
            let level = gate.page_level(page);
            let known = state.pages.get(page);
            let unchanged = known.is_some_and(|copy| copy.real == hash(&real) && copy.level == level)
                && fs::read_to_string(&file)
                    .is_ok_and(|text| known.is_some_and(|copy| copy.veiled == hash(&text)));
            if unchanged {
                continue;
            }
            let veiled = gate.veil_at(&real, level)?.text;
            write(&file, &veiled)?;
            state.pages.insert(
                page.clone(),
                Copy {
                    real: hash(&real),
                    veiled: hash(&veiled),
                    level,
                },
            );
        }
        for gone in mirror_pages(&root)
            .into_iter()
            .filter(|page| !pages.contains(page))
        {
            if let Some(file) = page_path(&root, &gone) {
                let _ = fs::remove_file(file);
            }
            state.pages.remove(&gone);
        }
        state.pages.retain(|page, _| pages.contains(page));
        let attachments = root.join(ATTACHMENTS).join(chat);
        let _ = fs::remove_dir_all(&attachments);
        fs::create_dir_all(&attachments).map_err(failed)?;
        let mut renamed = Vec::new();
        for entry in fs::read_dir(chat_folder).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if !path.is_file() || CHAT_FILES.contains(&name.as_str()) || name.ends_with(".tmp") {
                continue;
            }
            let Some(number) = name
                .split('-')
                .next()
                .filter(|number| number.parse::<u32>().is_ok())
            else {
                continue;
            };
            let Ok(text) = extract::text(&path) else {
                continue;
            };
            let copy = attachments.join(format!("{number}.txt"));
            write(
                &copy,
                &gate.veil_at(&text, level_of(chat_folder, &name))?.text,
            )?;
            renamed.push((
                path.to_string_lossy().into_owned(),
                copy.to_string_lossy().into_owned(),
            ));
        }
        let mirror = Self {
            root,
            attachments,
            state,
            renamed,
        };
        mirror.save()?;
        Ok(mirror)
    }

    fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&self.state).map_err(failed)?;
        keep(&self.root.join(STATE), &bytes).map_err(failed)
    }

    pub fn rewrite(&self, prompt: &str) -> String {
        self.renamed
            .iter()
            .fold(prompt.to_string(), |prompt, (real, copy)| {
                prompt.replace(real, copy)
            })
    }

    pub fn text(&self, page: &str) -> String {
        page_path(&self.root, page)
            .and_then(|file| fs::read_to_string(file).ok())
            .unwrap_or_default()
    }

    pub fn sync(
        &mut self,
        workspace: &Path,
        page: &str,
        gate: &Gate,
    ) -> Result<Option<Event>, String> {
        let Some(file) = page_path(&self.root, page) else {
            return Ok(None);
        };
        let Ok(veiled) = fs::read_to_string(&file) else {
            return Ok(None);
        };
        let theirs = gate.unveil_typst(&veiled);
        let exists = workspace::page_exists(workspace, page);
        let now = if exists {
            workspace::read_text(workspace, page)?
        } else {
            String::new()
        };
        let known = self.state.pages.get(page).cloned();
        let copy = Copy {
            real: hash(&theirs),
            veiled: hash(&veiled),
            level: gate.page_level(page),
        };
        if theirs == now {
            self.state.pages.insert(page.into(), copy);
            self.save()?;
            return Ok(None);
        }
        let moved_on = match &known {
            Some(known) => known.real != hash(&now),
            None => exists && !now.is_empty(),
        };
        if moved_on {
            return Ok(Some(Event::Conflict {
                page: page.into(),
                theirs,
            }));
        }
        workspace::ensure(workspace, page)?;
        workspace::write_text(workspace, page, &theirs)?;
        self.state.pages.insert(page.into(), copy);
        self.save()?;
        Ok(Some(Event::Edited {
            page: page.into(),
            before: Some(now),
        }))
    }

    pub fn finish(&mut self, workspace: &Path, gate: &Gate) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        for page in mirror_pages(&self.root) {
            let changed = match (
                self.state.pages.get(&page),
                fs::read_to_string(page_path(&self.root, &page).unwrap_or_default()),
            ) {
                (Some(copy), Ok(text)) => copy.veiled != hash(&text),
                (None, Ok(_)) => true,
                _ => false,
            };
            if changed {
                events.extend(self.sync(workspace, &page, gate)?);
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Knowledge, Options};
    use crate::privacy::glossary;

    fn knowledge() -> Knowledge {
        let ttl = "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:altLabel \"Acme\" ; skos:broader <urn:c> .";
        let glossary = glossary::parse(ttl.as_bytes(), oxrdfio::RdfFormat::Turtle).unwrap();
        Knowledge::in_memory(
            &glossary,
            Options {
                veil: true,
                ..Options::default()
            },
        )
    }

    fn scratch(test: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("znote-mirror-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (real, mirror, chat) = (base.join("real"), base.join("mirror"), base.join("chat"));
        for folder in [&real, &chat] {
            fs::create_dir_all(folder).unwrap();
        }
        workspace::ensure(&real, "Work/Notes/Acme").unwrap();
        workspace::write_text(
            &real,
            "Work/Notes/Acme",
            "= Acme Corporation\n- call Acme\n",
        )
        .unwrap();
        fs::write(chat.join("1-Acme report.txt"), "Acme pays € 40").unwrap();
        fs::write(chat.join("chat.json"), "{\"title\":\"Acme\"}").unwrap();
        (real, mirror, chat)
    }

    fn everything(folder: &Path) -> String {
        let mut all = String::new();
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            all.push_str(&path.to_string_lossy());
            if path.is_dir() {
                all.push_str(&everything(&path));
            } else if path.extension().is_none_or(|extension| extension != "json") {
                all.push_str(&fs::read_to_string(&path).unwrap());
            }
        }
        all
    }

    #[test]
    fn the_mirror_holds_veiled_pages_and_attachments_but_never_the_chat() {
        let (real, mirror, chat) = scratch("build");
        let knowledge = knowledge();
        let gate = knowledge.gate().unwrap();
        let built = Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate).unwrap();
        assert_eq!(
            built.text("Work/Notes/Acme"),
            "= ⟦CUSTOMER_1⟧\n- call ⟦CUSTOMER_1~1⟧\n"
        );
        let attached = fs::read_to_string(built.attachments.join("1.txt")).unwrap();
        assert_eq!(attached, "⟦CUSTOMER_1~1⟧ pays € 40");
        let readable = everything(&mirror).replace(&mirror.to_string_lossy().into_owned(), "");
        assert!(
            !readable.to_lowercase().contains("acme corporation"),
            "{readable}"
        );
        assert!(
            !mirror
                .join(ATTACHMENTS)
                .join("chat-1")
                .join("chat.json")
                .exists()
        );
        let prompt = format!(
            "Read the attached file {}",
            chat.join("1-Acme report.txt").to_string_lossy()
        );
        assert_eq!(
            built.rewrite(&prompt),
            format!(
                "Read the attached file {}",
                built.attachments.join("1.txt").to_string_lossy()
            )
        );
    }

    #[test]
    fn an_edit_in_the_mirror_lands_unveiled_and_a_conflicting_one_does_not() {
        let (real, mirror, chat) = scratch("sync");
        let knowledge = knowledge();
        let gate = knowledge.gate().unwrap();
        let mut built = Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate).unwrap();
        let file = page_path(&mirror, "Work/Notes/Acme").unwrap();
        fs::write(&file, "= ⟦CUSTOMER_1⟧\n- call ⟦CUSTOMER_1~1⟧ today\n").unwrap();
        let event = built.sync(&real, "Work/Notes/Acme", &gate).unwrap();
        assert_eq!(
            event,
            Some(Event::Edited {
                page: "Work/Notes/Acme".into(),
                before: Some("= Acme Corporation\n- call Acme\n".into())
            })
        );
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/Acme").unwrap(),
            "= Acme Corporation\n- call Acme today\n"
        );
        assert_eq!(built.sync(&real, "Work/Notes/Acme", &gate).unwrap(), None);

        workspace::write_text(&real, "Work/Notes/Acme", "typed by the user\n").unwrap();
        fs::write(&file, "= ⟦CUSTOMER_1⟧\nassistant again\n").unwrap();
        assert_eq!(
            built.finish(&real, &gate).unwrap(),
            [Event::Conflict {
                page: "Work/Notes/Acme".into(),
                theirs: "= Acme Corporation\nassistant again\n".into()
            }]
        );
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/Acme").unwrap(),
            "typed by the user\n"
        );

        let mut again = Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate).unwrap();
        assert_eq!(again.text("Work/Notes/Acme"), "typed by the user\n");
        let new_page = mirror.join("Work/Notes/New.typ");
        fs::write(&new_page, "about ⟦CUSTOMER_1⟧").unwrap();
        let events = again.finish(&real, &gate).unwrap();
        assert!(matches!(&events[..], [Event::Edited { page, .. }] if page == "Work/Notes/New"));
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/New").unwrap(),
            "about Acme Corporation"
        );
        assert!(again.finish(&real, &gate).unwrap().is_empty());
    }
}
