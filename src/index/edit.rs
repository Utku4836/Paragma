//! Changing a note file and its index entry together.

use super::Index;
use crate::note::{Kind, Note, Props};
use crate::project::{Project, files};
use anyhow::Result;

/// Changes a note under the project's write lock and indexes it at once,
/// so the next command (or a parallel one) sees it. `change` gets the note
/// (empty when the file does not exist yet) and whether it existed; a new
/// note gets the next id of its kind.
pub fn update_note<T>(
    project: &Project,
    rel: &str,
    change: impl FnOnce(&mut Note, bool, &Index) -> Result<T>,
) -> Result<(String, T)> {
    let _lock = files::WriteLock::take(&project.meta_dir())?;
    let mut index = Index::open(project)?;
    let file = project.note_file(rel);
    let rel = project.rel(&file);
    let (mut note, exists) = match files::read_text(&file) {
        Ok(old) => (Note::parse(&old), true),
        Err(_) => (Note::default(), false),
    };
    let result = change(&mut note, exists, &index)?;
    let id = match note.id() {
        Some(id) => id.to_string(),
        None if note.kind() == Kind::Rule => String::new(), // a rules note: its rules have the ids
        None => {
            let id = index.next_id(note.kind())?;
            let mut props = Props::default();
            props.set_text("id", id.clone());
            for (k, v) in note.props.iter() {
                props.set(k, v.clone());
            }
            note.props = props;
            id
        }
    };
    if !note.body.is_empty() && !note.body.ends_with('\n') {
        note.body.push('\n');
    }
    files::write_atomic(&file, note.render().as_bytes())?;
    index.refresh(project, &rel)?;
    Ok((id, result))
}
