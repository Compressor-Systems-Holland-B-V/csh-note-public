use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Event, attachment_id, keep, level_of};
use crate::extract;
use crate::knowledge::{Gate, hash};
use crate::workspace;

const STATE: &str = "mirror.json";
const ATTACHMENTS: &str = ".attachments";
const CHAT_FILES: [&str; 4] = ["chat.json", "history.json", "system.txt", "veil.txt"];

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
struct Copy {
    real: String,
    veiled: String,
}

// Keyed by the real page.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    pages: BTreeMap<String, Copy>,
}

pub struct Mirror {
    pub root: PathBuf,
    pub attachments: PathBuf,
    kept: PathBuf,
    state: State,
    renamed: Vec<(String, String)>,
    // The page as it is named in the mirror, and the real page it stands for.
    names: BTreeMap<String, String>,
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

// A project, notebook or page name can be a customer, so with hiding on each one is hidden too.
fn shown(gate: &Gate, page: &str) -> Result<Option<String>, String> {
    if !gate.on() {
        return Ok(Some(page.into()));
    }
    let mut names = Vec::new();
    for name in page.split('/') {
        let veiled = gate.veil(name)?.text;
        if workspace::check_name(&veiled).is_err() {
            return Ok(None);
        }
        names.push(veiled);
    }
    Ok(Some(names.join("/")))
}

// Folders left empty by pages that are gone, or by names that are hidden now.
fn prune(root: &Path) {
    for project in fs::read_dir(root).into_iter().flatten().flatten() {
        if project.file_name() == ATTACHMENTS || !project.path().is_dir() {
            continue;
        }
        for notebook in fs::read_dir(project.path()).into_iter().flatten().flatten() {
            let _ = fs::remove_dir(notebook.path());
        }
        let _ = fs::remove_dir(project.path());
    }
}

fn write(file: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    keep(file, text.as_bytes()).map_err(failed)
}

impl Mirror {
    // Only the pages `seen` lets through are copied, so the assistant cannot find any other page.
    pub fn build(
        root: PathBuf,
        workspace: &Path,
        chat_folder: &Path,
        chat: &str,
        gate: &Gate,
        seen: &dyn Fn(&str) -> bool,
    ) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(failed)?;
        let mut state = State::default();
        let mut names = BTreeMap::new();
        // Every page is hidden again each turn, so a name hidden since the last turn is hidden in
        // every copy too. The chat's pages are few and the name finder remembers its lines.
        for page in workspace::pages(workspace)
            .into_iter()
            .filter(|page| seen(page))
        {
            let real = workspace::read_text(workspace, &page)?;
            let Some(name) = shown(gate, &page)? else {
                continue;
            };
            let Some(file) = page_path(&root, &name) else {
                continue;
            };
            let veiled = gate.veil_at(&real, gate.page_level(&page))?.text;
            write(&file, &veiled)?;
            state.pages.insert(
                page.clone(),
                Copy {
                    real: hash(&real),
                    veiled: hash(&veiled),
                },
            );
            names.insert(name, page);
        }
        for gone in mirror_pages(&root)
            .into_iter()
            .filter(|name| !names.contains_key(name))
        {
            if let Some(file) = page_path(&root, &gone) {
                let _ = fs::remove_file(file);
            }
        }
        prune(&root);
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
            let Some(id) = attachment_id(&name) else {
                continue;
            };
            let number = id.split('.').next().unwrap_or_default();
            let copy = if gate.on() {
                let Ok(text) = extract::text(&path) else {
                    continue;
                };
                let copy = attachments.join(format!("{number}.txt"));
                write(
                    &copy,
                    &gate.veil_at(&text, level_of(chat_folder, &name))?.text,
                )?;
                copy
            } else {
                let kind = path.extension().unwrap_or_default().to_string_lossy();
                let copy = attachments.join(format!("{number}.{kind}"));
                fs::copy(&path, &copy).map_err(failed)?;
                copy
            };
            renamed.push((
                format!("the attached file {id}"),
                format!("the attached file {}", copy.to_string_lossy()),
            ));
        }
        let mirror = Self {
            root,
            attachments,
            kept: chat_folder.join(STATE),
            state,
            renamed,
            names,
        };
        mirror.save()?;
        Ok(mirror)
    }

    fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&self.state).map_err(failed)?;
        keep(&self.kept, &bytes).map_err(failed)
    }

    // Points the prompt's "the attached file 3.pdf" to the copy in the mirror.
    pub fn rewrite(&self, prompt: &str) -> String {
        self.renamed
            .iter()
            .fold(prompt.to_string(), |prompt, (id, copy)| {
                prompt.replace(id, copy)
            })
    }

    // The real page behind a page named in the mirror, also for one the assistant made itself.
    pub fn real(&self, name: &str, gate: &Gate) -> String {
        self.names
            .get(name)
            .cloned()
            .unwrap_or_else(|| gate.unveil(name).0)
    }

    // The real pages in the mirror.
    pub fn pages(&self) -> Vec<String> {
        self.names.values().cloned().collect()
    }

    // The name in the mirror of a real page.
    pub fn name(&self, page: &str) -> Option<&str> {
        self.names
            .iter()
            .find_map(|(name, real)| (real == page).then_some(name.as_str()))
    }

    pub fn text(&self, name: &str) -> String {
        page_path(&self.root, name)
            .and_then(|file| fs::read_to_string(file).ok())
            .unwrap_or_default()
    }

    pub fn sync(
        &mut self,
        workspace: &Path,
        name: &str,
        gate: &Gate,
    ) -> Result<Option<Event>, String> {
        let Some(file) = page_path(&self.root, name) else {
            return Ok(None);
        };
        let Ok(veiled) = fs::read_to_string(&file) else {
            return Ok(None);
        };
        let page = &self.real(name, gate);
        self.names.insert(name.into(), page.clone());
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
        for name in mirror_pages(&self.root) {
            let changed = match (
                self.state.pages.get(&self.real(&name, gate)),
                fs::read_to_string(page_path(&self.root, &name).unwrap_or_default()),
            ) {
                (Some(copy), Ok(text)) => copy.veiled != hash(&text),
                (None, Ok(_)) => true,
                _ => false,
            };
            if changed {
                events.extend(self.sync(workspace, &name, gate)?);
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

    // "Work/Notes/Acme" as the mirror names it.
    const SHOWN: &str = "Work/Notes/⟦CUSTOMER_1~1⟧";

    fn knowledge() -> Knowledge {
        knowledge_with("")
    }

    fn knowledge_with(more: &str) -> Knowledge {
        let ttl = format!(
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
<urn:c> skos:prefLabel \"Customer\" ; skos:topConceptOf <urn:s> .
<urn:a> skos:prefLabel \"Acme Corporation\" ; skos:altLabel \"Acme\" ; skos:broader <urn:c> .
{more}"
        );
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
            } else {
                all.push_str(&String::from_utf8_lossy(&fs::read(&path).unwrap()));
            }
        }
        all
    }

    #[test]
    fn the_mirror_holds_veiled_pages_and_attachments_but_never_the_chat() {
        let (real, mirror, chat) = scratch("build");
        let knowledge = knowledge();
        let gate = knowledge.gate().unwrap();
        let built =
            Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &|_| true).unwrap();
        assert_eq!(built.text(SHOWN), "= ⟦CUSTOMER_1⟧\n- call ⟦CUSTOMER_1~1⟧\n");
        let attached = fs::read_to_string(built.attachments.join("1.txt")).unwrap();
        assert_eq!(attached, "⟦CUSTOMER_1~1⟧ pays € 40");
        let readable = everything(&mirror).replace(&mirror.to_string_lossy().into_owned(), "");
        assert!(!readable.to_lowercase().contains("acme"), "{readable}");
        assert!(chat.join(STATE).is_file());
        assert!(
            !mirror
                .join(ATTACHMENTS)
                .join("chat-1")
                .join("chat.json")
                .exists()
        );
        assert_eq!(
            built.rewrite("Read the attached file 1.txt"),
            format!(
                "Read the attached file {}",
                built.attachments.join("1.txt").to_string_lossy()
            )
        );
    }

    #[test]
    fn the_mirror_holds_only_the_pages_the_chat_may_see() {
        let (real, mirror, chat) = scratch("scope");
        workspace::ensure(&real, "Home/Lists/Secret").unwrap();
        workspace::write_text(&real, "Home/Lists/Secret", "not for this chat").unwrap();
        let knowledge = knowledge();
        let gate = knowledge.gate().unwrap();
        let seen = |page: &str| page.starts_with("Work/");
        let built = Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &seen).unwrap();
        assert_eq!(built.text(SHOWN), "= ⟦CUSTOMER_1⟧\n- call ⟦CUSTOMER_1~1⟧\n");
        assert!(!everything(&mirror).contains("not for this chat"));

        let off = Knowledge::default();
        let plain = off.gate().unwrap();
        let built =
            Mirror::build(mirror.join("off"), &real, &chat, "chat-1", &plain, &seen).unwrap();
        assert_eq!(
            built.text("Work/Notes/Acme"),
            "= Acme Corporation\n- call Acme\n"
        );
        assert!(built.attachments.join("1.txt").exists());
        assert!(!everything(&mirror.join("off")).contains("not for this chat"));
    }

    #[test]
    fn an_edit_in_the_mirror_lands_unveiled_and_a_conflicting_one_does_not() {
        let (real, mirror, chat) = scratch("sync");
        let knowledge = knowledge();
        let gate = knowledge.gate().unwrap();
        let mut built =
            Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &|_| true).unwrap();
        let file = page_path(&mirror, SHOWN).unwrap();
        fs::write(&file, "= ⟦CUSTOMER_1⟧\n- call ⟦CUSTOMER_1~1⟧ today\n").unwrap();
        let event = built.sync(&real, SHOWN, &gate).unwrap();
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
        assert_eq!(built.sync(&real, SHOWN, &gate).unwrap(), None);

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

        let mut again =
            Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &|_| true).unwrap();
        assert_eq!(again.text(SHOWN), "typed by the user\n");
        fs::create_dir_all(mirror.join("Work/⟦CUSTOMER_1⟧")).unwrap();
        fs::write(mirror.join("Work/Notes/New.typ"), "about ⟦CUSTOMER_1⟧").unwrap();
        fs::write(mirror.join("Work/⟦CUSTOMER_1⟧/Plan.typ"), "plan").unwrap();
        let events = again.finish(&real, &gate).unwrap();
        let pages: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                Event::Edited { page, .. } => Some(page.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pages.len(), 2);
        assert!(pages.contains(&"Work/Acme Corporation/Plan") && pages.contains(&"Work/Notes/New"));
        assert_eq!(
            workspace::read_text(&real, "Work/Notes/New").unwrap(),
            "about Acme Corporation"
        );
        assert!(again.finish(&real, &gate).unwrap().is_empty());
    }

    #[test]
    fn a_name_hidden_after_the_chat_started_is_hidden_in_its_copy_too() {
        let (real, mirror, chat) = scratch("later");
        workspace::ensure(&real, "Work/Notes/Beta").unwrap();
        workspace::write_text(&real, "Work/Notes/Beta", "Beta pays").unwrap();
        let before = knowledge();
        let gate = before.gate().unwrap();
        Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &|_| true).unwrap();
        assert!(everything(&mirror).contains("Beta pays"));

        let after = knowledge_with("<urn:b> skos:prefLabel \"Beta\" ; skos:broader <urn:c> .");
        let gate = after.gate().unwrap();
        Mirror::build(mirror.clone(), &real, &chat, "chat-1", &gate, &|_| true).unwrap();
        let readable = everything(&mirror).replace(&mirror.to_string_lossy().into_owned(), "");
        assert!(!readable.contains("Beta"), "{readable}");
    }
}
