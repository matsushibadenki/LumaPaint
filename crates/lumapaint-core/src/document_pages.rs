//! Independent publication/page model. Each page retains its own layers and edit history.
use super::*;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageBinding {
    #[default]
    LeftToRight,
    RightToLeft,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageState {
    pub id: String,
    pub content: Option<Box<DocumentState>>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageBookState {
    pub facing: bool,
    pub binding: PageBinding,
    pub active: usize,
    pub next_id: u64,
    pub pages: Vec<PageState>,
}
#[derive(Clone)]
pub(super) struct PageRecord {
    id: String,
    content: Option<std::sync::Arc<Document>>,
}
#[derive(Clone)]
pub(super) struct PageBook {
    facing: bool,
    binding: PageBinding,
    active: usize,
    next_id: u64,
    pages: Vec<PageRecord>,
}
#[derive(Clone)]
pub(super) struct PageHistory {
    body: Box<Document>,
    book: Option<PageBook>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub id: String,
    pub number: usize,
    pub width: u32,
    pub height: u32,
    pub spread: usize,
    pub side: &'static str,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagesSnapshot {
    pub facing: bool,
    pub binding: PageBinding,
    pub active: usize,
    pub pages: Vec<PageSummary>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageEdit {
    pub action: String,
    pub index: Option<usize>,
    pub facing: Option<bool>,
    pub binding: Option<PageBinding>,
}
impl PageBook {
    fn new() -> Self {
        Self {
            facing: false,
            binding: PageBinding::LeftToRight,
            active: 0,
            next_id: 2,
            pages: vec![PageRecord {
                id: "page-1".into(),
                content: None,
            }],
        }
    }
    pub(super) fn from_state(state: PageBookState) -> Result<Self, String> {
        if state.pages.is_empty()
            || state.pages.len() > 512
            || state.active >= state.pages.len()
            || state.next_id < 2
            || state.next_id > 1_000_000
        {
            return Err("Invalid page collection".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut pages = Vec::new();
        for (i, page) in state.pages.into_iter().enumerate() {
            if page.id.is_empty()
                || page.id.len() > 128
                || !ids.insert(page.id.clone())
                || (i == state.active) != page.content.is_none()
            {
                return Err("Invalid page identity/content".into());
            }
            let content = match page.content {
                Some(content) => {
                    if content.pages.is_some() {
                        return Err("Nested publications are not allowed".into());
                    }
                    Some(std::sync::Arc::new(Document::from_document_state(
                        *content,
                    )?))
                }
                None => None,
            };
            pages.push(PageRecord {
                id: page.id,
                content,
            });
        }
        Ok(Self {
            facing: state.facing,
            binding: state.binding,
            active: state.active,
            next_id: state.next_id,
            pages,
        })
    }
    fn state(&self) -> PageBookState {
        PageBookState {
            facing: self.facing,
            binding: self.binding,
            active: self.active,
            next_id: self.next_id,
            pages: self
                .pages
                .iter()
                .map(|p| PageState {
                    id: p.id.clone(),
                    content: p.content.as_ref().map(|d| Box::new(d.document_state())),
                })
                .collect(),
        }
    }
}
impl Document {
    pub fn pages_snapshot(&self) -> PagesSnapshot {
        let default = PageBook::new();
        let book = self.pages.as_ref().unwrap_or(&default);
        PagesSnapshot {
            facing: book.facing,
            binding: book.binding,
            active: book.active,
            pages: book
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (width, height) = p
                        .content
                        .as_ref()
                        .map_or(self.dimensions(), |d| d.dimensions());
                    let side = if !book.facing {
                        "single"
                    } else if (i % 2 == 0) == (book.binding == PageBinding::LeftToRight) {
                        "right"
                    } else {
                        "left"
                    };
                    PageSummary {
                        id: p.id.clone(),
                        number: i + 1,
                        width,
                        height,
                        spread: if book.facing { i.div_ceil(2) } else { i },
                        side,
                    }
                })
                .collect(),
        }
    }
    pub(super) fn pages_state(&self) -> Option<PageBookState> {
        self.pages.as_ref().map(PageBook::state)
    }
    pub fn page_document(&self, index: usize) -> Option<&Document> {
        match &self.pages {
            Some(b) if index < b.pages.len() => {
                if index == b.active {
                    Some(self)
                } else {
                    b.pages[index].content.as_deref()
                }
            }
            None if index == 0 => Some(self),
            _ => None,
        }
    }
    /// Independent page copy for conversion without retaining the publication or
    /// changing its active page, selection, saved state or edit histories.
    pub fn detached_page(&self, index: usize) -> Option<Document> {
        let mut page = self.page_document(index)?.clone();
        page.pages = None;
        page.page_undo.clear();
        page.page_redo.clear();
        page.undo_order
            .retain(|kind| !matches!(kind, HistoryKind::Page));
        page.redo_order
            .retain(|kind| !matches!(kind, HistoryKind::Page));
        Some(page)
    }
    pub fn facing_neighbor(&self) -> Option<(usize, f32, &Document)> {
        let b = self.pages.as_ref()?;
        if !b.facing || b.active == 0 {
            return None;
        }
        let index = if b.active % 2 == 1 {
            b.active + 1
        } else {
            b.active - 1
        };
        let neighbor = self.page_document(index)?;
        let active_right = (b.active % 2 == 0) == (b.binding == PageBinding::LeftToRight);
        Some((
            index,
            if active_right {
                -(neighbor.width as f32)
            } else {
                self.width as f32
            },
            neighbor,
        ))
    }
    fn page_history(&self) -> PageHistory {
        let mut body = self.clone();
        body.pages = None;
        body.page_undo.clear();
        body.page_redo.clear();
        PageHistory {
            body: Box::new(body),
            book: self.pages.clone(),
        }
    }
    fn restore_page_history(&mut self, state: PageHistory) {
        let mut next = *state.body;
        next.pages = state.book;
        // Layout/history restoration must not overwrite edits made on other pages.
        if let (Some(target), Some(current)) = (&mut next.pages, &self.pages) {
            target.next_id = target.next_id.max(current.next_id);
            for (index, page) in target.pages.iter_mut().enumerate() {
                if index == target.active {
                    continue;
                }
                if let Some((old_index, old)) = current
                    .pages
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.id == page.id)
                {
                    if old_index == current.active {
                        let mut body = self.clone();
                        body.pages = None;
                        body.page_undo.clear();
                        body.page_redo.clear();
                        page.content = Some(std::sync::Arc::new(body));
                    } else {
                        page.content = old.content.clone();
                    }
                }
            }
        }
        std::mem::swap(&mut next.redo_order, &mut self.redo_order);
        std::mem::swap(&mut next.vector_redo, &mut self.vector_redo);
        std::mem::swap(&mut next.redo, &mut self.redo);
        std::mem::swap(&mut next.page_undo, &mut self.page_undo);
        std::mem::swap(&mut next.page_redo, &mut self.page_redo);
        next.revision = self.revision;
        next.saved_revision = self.saved_revision;
        next.file_name = self.file_name.clone();
        next.svg_geometry_backend = self.svg_geometry_backend.clone();
        *self = next;
    }
    pub(super) fn undo_page(&mut self) {
        if let Some(before) = self.page_undo.pop() {
            let now = self.page_history();
            self.restore_page_history(before);
            self.page_redo.push(now);
            self.redo_order.push(HistoryKind::Page);
            self.revision += 1;
        }
    }
    pub(super) fn redo_page(&mut self) {
        if let Some(next) = self.page_redo.pop() {
            let now = self.page_history();
            self.restore_page_history(next);
            self.page_undo.push(now);
            self.undo_order.push(HistoryKind::Page);
            self.revision += 1;
        }
    }
    fn switch_page(&mut self, index: usize) -> Result<(), String> {
        let count = self.pages.as_ref().map_or(1, |b| b.pages.len());
        if index >= count {
            return Err("Page not found".into());
        }
        let active = self.pages.as_ref().map_or(0, |b| b.active);
        if active == index {
            return Ok(());
        }
        self.finish();
        let mut book = self.pages.take().unwrap_or_else(PageBook::new);
        let mut next = std::sync::Arc::unwrap_or_clone(
            book.pages[index]
                .content
                .take()
                .ok_or("Page has no content")?,
        );
        next.svg_geometry_backend = self.svg_geometry_backend.clone();
        next.name = self.name.clone();
        next.file_name = self.file_name.clone();
        next.revision = self.revision + 1;
        next.saved_revision = if self.revision == self.saved_revision {
            next.revision
        } else {
            self.saved_revision
        };
        // The parked page has no nested book. Its own undo/redo remains intact.
        let previous = std::mem::replace(self, next);
        book.pages[active].content = Some(std::sync::Arc::new(previous));
        book.active = index;
        self.pages = Some(book);
        self.deselect();
        Ok(())
    }
    pub fn edit_pages(&mut self, edit: PageEdit) -> Result<(), String> {
        if edit.action == "select" {
            return self.switch_page(edit.index.ok_or("Missing page index")?);
        }
        self.finish();
        let before = self.page_history();
        if edit.action == "delete"
            && edit.index.unwrap_or(self.pages_snapshot().active) == self.pages_snapshot().active
        {
            let snapshot = self.pages_snapshot();
            if snapshot.pages.len() == 1 {
                return Err("At least one page is required".into());
            }
            let history = std::mem::take(&mut self.page_undo);
            self.switch_page(if snapshot.active + 1 < snapshot.pages.len() {
                snapshot.active + 1
            } else {
                snapshot.active - 1
            })?;
            self.page_undo = history;
        }
        let mut book = self.pages.take().unwrap_or_else(PageBook::new);
        let result = (|| -> Result<(), String> {
            match edit.action.as_str() {
                "layout" => {
                    book.facing = edit.facing.unwrap_or(book.facing);
                    book.binding = edit.binding.unwrap_or(book.binding);
                }
                "add" | "duplicate" => {
                    if book.pages.len() >= 512 {
                        return Err("Maximum 512 pages".into());
                    }
                    let index = edit.index.unwrap_or(book.active);
                    if index >= book.pages.len() {
                        return Err("Page not found".into());
                    }
                    let mut content = if edit.action == "duplicate" {
                        if index == book.active {
                            self.clone()
                        } else {
                            book.pages[index].content.as_ref().unwrap().as_ref().clone()
                        }
                    } else {
                        Document {
                            width: self.width,
                            height: self.height,
                            resolution: self.resolution,
                            unit: self.unit,
                            canvas_color: self.canvas_color,
                            color_mode: self.color_mode,
                            color_profile: self.color_profile,
                            bit_depth: self.bit_depth,
                            ..Document::default()
                        }
                    };
                    content.pages = None;
                    content.page_undo.clear();
                    content.page_redo.clear();
                    if edit.action == "duplicate" {
                        content.undo_order.clear();
                        content.redo_order.clear();
                        content.vector_undo.clear();
                        content.vector_redo.clear();
                        content.redo.clear();
                    }
                    while book
                        .pages
                        .iter()
                        .any(|p| p.id == format!("page-{}", book.next_id))
                    {
                        book.next_id += 1;
                    }
                    let id = format!("page-{}", book.next_id);
                    book.next_id += 1;
                    book.pages.insert(
                        index + 1,
                        PageRecord {
                            id,
                            content: Some(std::sync::Arc::new(content)),
                        },
                    );
                    if book.active > index {
                        book.active += 1;
                    }
                }
                "delete" => {
                    let index = edit
                        .index
                        .unwrap_or_else(|| before.book.as_ref().map_or(0, |b| b.active));
                    if book.pages.len() == 1 || index >= book.pages.len() {
                        return Err("At least one page is required".into());
                    }
                    if index == book.active {
                        return Err("Select another page before deleting this page".into());
                    }
                    book.pages.remove(index);
                    if index < book.active {
                        book.active -= 1;
                    }
                }
                "moveBefore" | "moveAfter" => {
                    let target = edit.index.ok_or("Missing destination")?;
                    if target >= book.pages.len() {
                        return Err("Page not found".into());
                    }
                    let from = book.active;
                    if target == from {
                        return Ok(());
                    }
                    let page = book.pages.remove(from);
                    let destination = if edit.action == "moveBefore" {
                        target - usize::from(from < target)
                    } else {
                        target + 1 - usize::from(from < target)
                    };
                    book.pages.insert(destination, page);
                    book.active = destination;
                }
                _ => return Err("Unknown page action".into()),
            }
            Ok(())
        })();
        self.pages = Some(book);
        if let Err(error) = result {
            self.restore_page_history(before);
            return Err(error);
        }
        self.page_undo.push(before);
        self.page_redo.clear();
        self.redo_order.clear();
        self.vector_redo.clear();
        self.redo.clear();
        self.undo_order.push(HistoryKind::Page);
        self.revision += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(d: &mut Document, action: &str, index: Option<usize>) {
        d.edit_pages(PageEdit {
            action: action.into(),
            index,
            facing: None,
            binding: None,
        })
        .unwrap();
    }
    #[test]
    fn independent_content_history_layout_and_native_roundtrip() {
        let mut d = Document::default();
        d.import_svg("Original".into(),"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect width=\"50\" height=\"40\"/></svg>".into()).unwrap();
        edit(&mut d, "add", None);
        edit(&mut d, "add", Some(1));
        edit(&mut d, "select", Some(1));
        assert_eq!(d.svg_layers().count(), 0);
        d.import_svg("Second".into(),"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><circle r=\"20\"/></svg>".into()).unwrap();
        let layer = d.add_vector_layer().unwrap();
        d.delete_layer(&layer).unwrap();
        edit(&mut d, "select", Some(0));
        assert_eq!(d.svg_layers().next().unwrap().name, "Original");
        edit(&mut d, "select", Some(1));
        d.undo();
        assert_eq!(d.svg_layers().count(), 2);
        d.redo();
        assert_eq!(d.svg_layers().count(), 1);
        d.undo();
        d.edit_pages(PageEdit {
            action: "layout".into(),
            index: None,
            facing: Some(true),
            binding: Some(PageBinding::RightToLeft),
        })
        .unwrap();
        let pages = d.pages_snapshot();
        assert_eq!(
            pages.pages.iter().map(|p| p.side).collect::<Vec<_>>(),
            vec!["left", "right", "left"]
        );
        assert_eq!(pages.pages[1].spread, pages.pages[2].spread);
        assert!(d.facing_neighbor().unwrap().1 < 0.);
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(restored.pages_snapshot().pages.len(), 3);
        assert_eq!(
            restored
                .page_document(0)
                .unwrap()
                .svg_layers()
                .next()
                .unwrap()
                .name,
            "Original"
        );
    }
    #[test]
    fn duplicate_move_delete_undo_and_validation_are_atomic() {
        let mut d = Document::default();
        edit(&mut d, "duplicate", None);
        assert_eq!(d.pages_snapshot().pages.len(), 2);
        d.undo();
        assert_eq!(d.pages_snapshot().pages.len(), 1);
        d.redo();
        assert_eq!(d.pages_snapshot().pages.len(), 2);
        edit(&mut d, "select", Some(1));
        edit(&mut d, "moveBefore", Some(0));
        assert_eq!(d.pages_snapshot().active, 0);
        d.undo();
        assert_eq!(d.pages_snapshot().active, 1);
        d.redo();
        assert_eq!(d.pages_snapshot().active, 0);
        edit(&mut d, "delete", None);
        assert_eq!(d.pages_snapshot().pages.len(), 1);
        d.undo();
        assert_eq!(d.pages_snapshot().pages.len(), 2);
        d.redo();
        assert_eq!(d.pages_snapshot().pages.len(), 1);
        let before = d.document_state();
        assert!(d
            .edit_pages(PageEdit {
                action: "delete".into(),
                index: None,
                facing: None,
                binding: None
            })
            .is_err());
        assert_eq!(
            serde_json::to_string(&d.document_state()).unwrap(),
            serde_json::to_string(&before).unwrap()
        );
        let mut bad = d.document_state();
        bad.pages.as_mut().unwrap().active = 99;
        assert!(Document::from_document_state(bad).is_err());
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSetup {
    pub count: usize,
    pub facing: bool,
    pub binding: PageBinding,
}
impl Document {
    pub(super) fn setup_pages(&mut self, setup: PageSetup) -> Result<(), String> {
        if !(1..=512).contains(&setup.count) {
            return Err("Page count must be between 1 and 512".into());
        }
        let mut book = PageBook::new();
        book.facing = setup.facing;
        book.binding = setup.binding;
        book.next_id = setup.count as u64 + 1;
        let mut blank = self.clone();
        blank.pages = None;
        blank.page_undo.clear();
        blank.page_redo.clear();
        blank.undo_order.clear();
        blank.redo_order.clear();
        let blank = std::sync::Arc::new(blank);
        for i in 1..setup.count {
            book.pages.push(PageRecord {
                id: format!("page-{}", i + 1),
                content: Some(blank.clone()),
            });
        }
        self.pages = Some(book);
        Ok(())
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn bindings_pagination_navigation_and_other_page_edits_survive_layout_undo() {
        let mut d = Document::default();
        d.setup_pages(PageSetup {
            count: 6,
            facing: true,
            binding: PageBinding::LeftToRight,
        })
        .unwrap();
        assert_eq!(
            d.pages_snapshot()
                .pages
                .iter()
                .map(|p| (p.spread, p.side))
                .collect::<Vec<_>>(),
            vec![
                (0, "right"),
                (1, "left"),
                (1, "right"),
                (2, "left"),
                (2, "right"),
                (3, "left")
            ]
        );
        d.mark_saved("test.lumapaint".into());
        d.switch_page(1).unwrap();
        assert!(!d.snapshot().dirty);
        d.edit_pages(PageEdit {
            action: "layout".into(),
            index: None,
            facing: Some(false),
            binding: None,
        })
        .unwrap();
        d.switch_page(2).unwrap();
        d.import_svg("Keep other page edits".into(),"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect width=\"30\" height=\"30\"/></svg>".into()).unwrap();
        d.switch_page(1).unwrap();
        d.undo();
        assert!(d.pages_snapshot().facing);
        assert_eq!(
            d.page_document(2)
                .unwrap()
                .svg_layers()
                .next()
                .unwrap()
                .name,
            "Keep other page edits"
        );
        let bytes = d.encode().unwrap();
        let restored = Document::decode(&bytes).unwrap();
        assert_eq!(restored.pages_snapshot().pages.len(), 6);
        assert_eq!(restored.pages_snapshot().active, 1);
        assert_eq!(restored.page_document(2).unwrap().svg_layers().count(), 1);
        assert!(d
            .setup_pages(PageSetup {
                count: 513,
                facing: false,
                binding: PageBinding::LeftToRight
            })
            .is_err());
    }
}
