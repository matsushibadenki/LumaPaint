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
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageTextFrame {
    pub page_id: String,
    pub object_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageTextThread {
    pub id: String,
    pub name: String,
    pub frames: Vec<PageTextFrame>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageBookState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_threads: Vec<PageTextThread>,
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
pub(super) struct StoryEdit {
    token: u64,
    participants: std::collections::BTreeSet<String>,
    scope: PageTextThread,
    thread_before: Option<PageTextThread>,
    thread_after: Option<PageTextThread>,
    before: Vec<(PageTextFrame, VectorObject)>,
    after: Vec<(PageTextFrame, VectorObject)>,
}
#[derive(Clone)]
pub(super) struct PageBook {
    text_threads: Vec<PageTextThread>,
    story_undo: Vec<std::sync::Arc<StoryEdit>>,
    story_redo: Vec<std::sync::Arc<StoryEdit>>,
    story_epoch: u64,
    history_frames: std::collections::BTreeSet<(String, String)>,
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
            text_threads: Vec::new(),
            story_undo: Vec::new(),
            story_redo: Vec::new(),
            story_epoch: 0,
            history_frames: Default::default(),
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
            text_threads: state.text_threads,
            story_undo: Vec::new(),
            story_redo: Vec::new(),
            story_epoch: 0,
            history_frames: Default::default(),
            facing: state.facing,
            binding: state.binding,
            active: state.active,
            next_id: state.next_id,
            pages,
        })
    }
    fn state(&self) -> PageBookState {
        PageBookState {
            text_threads: self.text_threads.clone(),
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
    pub(super) fn page_history(&self) -> PageHistory {
        let mut body = self.clone();
        body.pages = None;
        body.page_undo.clear();
        body.page_redo.clear();
        PageHistory {
            body: Box::new(body),
            book: self.pages.clone(),
        }
    }
    pub(super) fn restore_page_history(&mut self, state: PageHistory) {
        let mut next = *state.body;
        next.pages = state.book;
        // Layout/history restoration must not overwrite edits made on other pages.
        if let (Some(target), Some(current)) = (&mut next.pages, &self.pages) {
            target.next_id = target.next_id.max(current.next_id);
            target.story_undo = current.story_undo.clone();
            target.story_redo = current.story_redo.clone();
            target.story_epoch = current.story_epoch;
            target.history_frames = current.history_frames.clone();
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
        next.text_layout_backend = self.text_layout_backend;
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
        next.text_layout_backend = self.text_layout_backend;
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
        if edit.action == "delete"
            && self.pages.as_ref().is_some_and(|b| {
                let id = &b.pages[edit.index.unwrap_or(b.active).min(b.pages.len() - 1)].id;
                b.text_threads
                    .iter()
                    .any(|t| t.frames.iter().any(|f| &f.page_id == id))
            })
        {
            return Err("Detach the publication story before deleting its page / 連結を解除してからページを削除してください / 请先解除跨页文本链接再删除页面".into());
        }
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
        self.prune_publication_text_history();
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

impl Document {
    /// Validate page-qualified frame identities against the complete publication.
    pub(super) fn validate_publication_text(&self) -> Result<(), String> {
        let Some(book) = &self.pages else {
            return Ok(());
        };
        let mut ids = std::collections::BTreeSet::new();
        let mut frames = std::collections::BTreeSet::new();
        if book.text_threads.len() > 512 {
            return Err("Too many publication stories".into());
        }
        for thread in &book.text_threads {
            if thread.id.is_empty()
                || thread.id.len() > 128
                || !ids.insert(&thread.id)
                || thread.name.is_empty()
                || thread.name.chars().count() > 120
                || thread.frames.len() < 2
                || thread.frames.len() > 64
            {
                return Err("Invalid publication story".into());
            }
            let mut resolution = None;
            let mut direction = None;
            let mut length = 0;
            for frame in &thread.frames {
                if !frames.insert((&frame.page_id, &frame.object_id)) {
                    return Err("Duplicate publication text frame".into());
                }
                let index = book
                    .pages
                    .iter()
                    .position(|p| p.id == frame.page_id)
                    .ok_or("Missing story page")?;
                let page = self
                    .page_document(index)
                    .ok_or("Missing story page content")?;
                if resolution
                    .replace(page.resolution)
                    .is_some_and(|r| r != page.resolution)
                {
                    return Err("Linked pages require the same resolution".into());
                }
                let object = page
                    .svg_layers
                    .iter()
                    .flat_map(|l| &l.vector_objects)
                    .find(|o| o.id == frame.object_id)
                    .ok_or("Missing publication text frame")?;
                let text = object
                    .text
                    .as_ref()
                    .ok_or("Publication frame is not text")?;
                if text.point_text
                    || text.box_height.is_none()
                    || direction
                        .replace(text.writing_mode)
                        .is_some_and(|mode| mode != text.writing_mode)
                    || page
                        .vector_assets
                        .threads
                        .iter()
                        .any(|t| t.frames.contains(&frame.object_id))
                {
                    return Err("Invalid publication area text frame".into());
                }
                length += text.content.encode_utf16().count();
            }
            if length > 65536 {
                return Err("Publication story exceeds 65536 UTF-16 units".into());
            }
        }
        Ok(())
    }
    /// Import-time recomposition, before the document is adopted by an editor session.
    /// Work on detached copies, so a layout failure cannot partially replace pages.
    pub fn recompose_imported_publication_text(
        &mut self,
        layout: TextLayout,
    ) -> Result<(), String> {
        self.recompose_publication_thread(layout, None)
    }
    fn recompose_publication_thread(
        &mut self,
        layout: TextLayout,
        only: Option<&str>,
    ) -> Result<(), String> {
        self.validate_publication_text()?;
        let Some(book) = &self.pages else {
            return Ok(());
        };
        let threads: Vec<_> = book
            .text_threads
            .iter()
            .filter(|t| only.is_none_or(|id| id == t.id))
            .cloned()
            .collect();
        if threads.is_empty() {
            return Ok(());
        }
        let mut pages: Vec<_> = (0..book.pages.len())
            .map(|i| self.detached_page(i).ok_or("Missing page"))
            .collect::<Result<_, _>>()?;
        let page_ids: Vec<_> = book.pages.iter().map(|p| p.id.clone()).collect();
        let original_layers: Vec<_> = pages.iter().map(|p| p.svg_layers.clone()).collect();
        for thread in threads {
            let mut objects = Vec::new();
            for (i, f) in thread.frames.iter().enumerate() {
                let pi = page_ids
                    .iter()
                    .position(|id| id == &f.page_id)
                    .ok_or("Missing page")?;
                let mut object = pages[pi]
                    .svg_layers
                    .iter()
                    .flat_map(|l| &l.vector_objects)
                    .find(|o| o.id == f.object_id)
                    .ok_or("Missing frame")?
                    .clone();
                object.id = format!("frame-{i}");
                objects.push(object);
            }
            let mut layer = pages[0].svg_layers.first().ok_or("Missing layer")?.clone();
            layer.vector_objects = objects;
            let mut layers = vec![layer];
            let local = super::vector_assets::TextThread {
                id: thread.id.clone(),
                name: thread.name.clone(),
                frames: (0..thread.frames.len())
                    .map(|i| format!("frame-{i}"))
                    .collect(),
            };
            super::vector_assets::reflow_thread(&mut layers, &local, layout)?;
            for (f, mut object) in thread.frames.iter().zip(layers.remove(0).vector_objects) {
                let pi = page_ids
                    .iter()
                    .position(|id| id == &f.page_id)
                    .ok_or("Missing page")?;
                let target = pages[pi]
                    .svg_layers
                    .iter_mut()
                    .flat_map(|l| &mut l.vector_objects)
                    .find(|o| o.id == f.object_id)
                    .ok_or("Missing frame")?;
                target.text = object.text.take();
                target.control_points = object.control_points;
            }
        }
        for (i, page) in pages.iter_mut().enumerate() {
            for (j, layer) in page.svg_layers.iter_mut().enumerate() {
                if layer.vector_objects != original_layers[i][j].vector_objects {
                    layer.source = vector_svg(page.width, page.height, &layer.vector_objects);
                    validate_svg_layer(layer)?;
                }
            }
            page.scene_journal
                .layers_changed(&original_layers[i], &page.svg_layers);
            page.transform_slots.0.clear();
        }
        let book = self.pages.as_mut().unwrap();
        for (i, page) in pages.into_iter().enumerate() {
            if i == book.active {
                self.svg_layers = page.svg_layers;
                self.scene_journal = page.scene_journal;
                self.transform_slots.0.clear();
            } else {
                book.pages[i].content = Some(std::sync::Arc::new(page));
            }
        }
        Ok(())
    }
}

impl Document {
    pub(super) fn publication_text_epoch(&self) -> u64 {
        self.pages.as_ref().map_or(0, |b| b.story_epoch)
    }
    pub(super) fn is_publication_text_frame(&self, id: &str) -> bool {
        self.pages.as_ref().is_some_and(|b| {
            b.text_threads.iter().any(|t| {
                t.frames
                    .iter()
                    .any(|f| f.page_id == b.pages[b.active].id && f.object_id == id)
            })
        })
    }
    pub(super) fn active_publication_text_objects(
        &self,
    ) -> std::collections::BTreeMap<String, VectorObject> {
        self.svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .filter(|o| {
                self.is_managed_publication_text_frame(&o.id)
                    || self.pages.as_ref().is_some_and(|b| {
                        b.history_frames
                            .contains(&(b.pages[b.active].id.clone(), o.id.clone()))
                    })
            })
            .map(|o| (o.id.clone(), o.clone()))
            .collect()
    }
    fn story_objects(
        &self,
        thread: &PageTextThread,
    ) -> Result<Vec<(PageTextFrame, VectorObject)>, String> {
        let book = self.pages.as_ref().ok_or("Missing publication")?;
        thread
            .frames
            .iter()
            .map(|f| {
                let i = book
                    .pages
                    .iter()
                    .position(|p| p.id == f.page_id)
                    .ok_or("Missing page")?;
                let page = self.page_document(i).ok_or("Missing page")?;
                let object = page
                    .svg_layers
                    .iter()
                    .flat_map(|l| &l.vector_objects)
                    .find(|o| o.id == f.object_id)
                    .ok_or("Missing frame")?;
                Ok((f.clone(), object.clone()))
            })
            .collect()
    }
    fn page_mut(&mut self, id: &str) -> Option<&mut Document> {
        let book = self.pages.as_ref()?;
        let i = book.pages.iter().position(|p| p.id == id)?;
        let active = book.active;
        if i == active {
            Some(self)
        } else {
            Some(std::sync::Arc::make_mut(
                self.pages.as_mut()?.pages[i].content.as_mut()?,
            ))
        }
    }
    pub(super) fn update_publication_text_object(
        &mut self,
        layer_id: &str,
        object: VectorObject,
    ) -> Result<(), String> {
        let book = self.pages.as_ref().ok_or("Missing publication")?;
        let active = &book.pages[book.active].id;
        let member = |t: &&PageTextThread| {
            t.frames
                .iter()
                .any(|f| &f.page_id == active && f.object_id == object.id)
        };
        let linked = book.text_threads.iter().find(member).cloned();
        let thread = linked
            .clone()
            .or_else(|| {
                book.story_undo
                    .iter()
                    .rev()
                    .chain(book.story_redo.iter().rev())
                    .map(|e| &e.scope)
                    .find(member)
                    .cloned()
            })
            .ok_or("Missing publication story")?;
        for f in &thread.frames {
            let i = book
                .pages
                .iter()
                .position(|p| p.id == f.page_id)
                .ok_or("Missing page")?;
            let page = self.page_document(i).ok_or("Missing page")?;
            page.ensure_object_unlocked(&f.object_id)?;
            if page.svg_layers.iter().any(|l| {
                l.vector_objects.iter().any(|o| o.id == f.object_id) && (l.locked || !l.visible)
            }) {
                return Err("Linked frame is locked or hidden / 連結フレームがロックまたは非表示です / 链接文本框已锁定或隐藏".into());
            }
        }
        let before = self.story_objects(&thread)?;
        let mut candidate = self.clone();
        let target = candidate
            .svg_layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .and_then(|l| l.vector_objects.iter_mut().find(|o| o.id == object.id))
            .ok_or("Missing frame")?;
        *target = object;
        if linked.is_none() {
            let color = target
                .fill
                .map_or([0; 3], |p| [p.color[0], p.color[1], p.color[2]]);
            let text = target.text.as_mut().ok_or("Missing text")?;
            (self
                .text_layout_backend
                .ok_or("Text layout backend unavailable")?)(text, color)?;
            target.control_points = text.control_points();
        }
        let layer = candidate
            .svg_layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .ok_or("Missing layer")?;
        layer.source = vector_svg(candidate.width, candidate.height, &layer.vector_objects);
        validate_svg_layer(layer)?;
        if linked.is_some() {
            candidate.recompose_publication_thread(
                self.text_layout_backend
                    .ok_or("Text layout backend unavailable")?,
                Some(&thread.id),
            )?;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64);
        for (f, previous) in &before {
            let page = candidate.page_mut(&f.page_id).ok_or("Missing page")?;
            let object = page
                .svg_layers
                .iter_mut()
                .flat_map(|l| &mut l.vector_objects)
                .find(|o| o.id == f.object_id)
                .ok_or("Missing frame")?;
            if object.text != previous.text {
                page.text_change_generation = page
                    .text_change_generation
                    .max(previous.text.as_ref().map_or(0, |t| t.change_generation))
                    .saturating_add(1);
                if let Some(t) = &mut object.text {
                    t.change_generation = page.text_change_generation;
                    t.updated_at_ms =
                        now.max(previous.text.as_ref().map_or(0, |t| t.updated_at_ms));
                }
            }
        }
        let after = candidate.story_objects(&thread)?;
        if before == after {
            return Ok(());
        }
        candidate
            .scene_journal
            .layers_changed(&self.svg_layers, &candidate.svg_layers);
        self.svg_layers = candidate.svg_layers;
        self.scene_journal = candidate.scene_journal;
        self.transform_slots.0.clear();
        self.pages = candidate.pages;
        self.commit_story_history(linked.clone(), linked, before, after);
        Ok(())
    }
    pub(super) fn restore_publication_text(&mut self, token: u64, redo: bool) {
        let Some(book) = &mut self.pages else { return };
        let source = if redo {
            &mut book.story_redo
        } else {
            &mut book.story_undo
        };
        let Some(index) = source.iter().position(|e| e.token == token) else {
            return;
        };
        let edit = source.remove(index);
        book.story_epoch += 1;
        let desired = if redo {
            &edit.thread_after
        } else {
            &edit.thread_before
        };
        if let Some(thread) = edit.thread_after.as_ref().or(edit.thread_before.as_ref()) {
            book.text_threads.retain(|t| t.id != thread.id);
        }
        if let Some(thread) = desired {
            book.text_threads.push(thread.clone());
        }
        let (wanted, previous) = if redo {
            (&edit.after, &edit.before)
        } else {
            (&edit.before, &edit.after)
        };
        let mut affected = edit.participants.clone();
        for ((f, object), (_, old)) in wanted.iter().zip(previous) {
            affected.insert(f.page_id.clone());
            if let Some(page) = self.page_mut(&f.page_id) {
                let old_layers = page.svg_layers.clone();
                for layer in &mut page.svg_layers {
                    if let Some(target) = layer
                        .vector_objects
                        .iter_mut()
                        .find(|o| o.id == f.object_id)
                    {
                        target.text = object.text.clone();
                        target.control_points = object.control_points.clone();
                        if target.transform == old.transform {
                            target.transform = object.transform;
                        }
                        if target.fill == old.fill {
                            target.fill = object.fill;
                        }
                        layer.source = vector_svg(page.width, page.height, &layer.vector_objects);
                    }
                }
                page.scene_journal
                    .layers_changed(&old_layers, &page.svg_layers);
                page.transform_slots.0.clear();
            }
        }
        for id in affected {
            if let Some(page) = self.page_mut(&id) {
                page.undo_order
                    .retain(|k| !matches!(k,HistoryKind::PublicationText(t)if *t==token));
                page.redo_order
                    .retain(|k| !matches!(k,HistoryKind::PublicationText(t)if *t==token));
                if redo {
                    page.undo_order.push(HistoryKind::PublicationText(token));
                } else {
                    page.redo_order.push(HistoryKind::PublicationText(token));
                }
                page.revision += 1;
            }
        }
        let book = self.pages.as_mut().unwrap();
        if redo {
            book.story_undo.push(edit);
        } else {
            book.story_redo.push(edit);
        }
        self.sync_publication_history_markers();
    }
    fn sync_publication_history_markers(&mut self) {
        let Some(book) = &self.pages else { return };
        let undo: std::collections::BTreeSet<_> = book.story_undo.iter().map(|e| e.token).collect();
        let redo: std::collections::BTreeSet<_> = book.story_redo.iter().map(|e| e.token).collect();
        let ids: Vec<_> = book.pages.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            if let Some(page) = self.page_mut(&id) {
                page.undo_order.retain(|k| match k {
                    HistoryKind::PublicationText(t) => undo.contains(t),
                    _ => true,
                });
                page.redo_order.retain(|k| match k {
                    HistoryKind::PublicationText(t) => redo.contains(t),
                    _ => true,
                });
            }
        }
    }
}
impl Document {
    pub(super) fn ensure_no_publication_frames_in_layer(&self, id: &str) -> Result<(), String> {
        if self
            .svg_layers
            .iter()
            .filter(|l| l.id == id)
            .flat_map(|l| &l.vector_objects)
            .any(|o| self.is_publication_text_frame(&o.id))
        {
            return Err("Detach the publication story before removing its frames / 連結を解除してからフレームを削除してください / 请先解除跨页文本链接再删除文本框".into());
        }
        Ok(())
    }
}
impl Document {
    pub(super) fn ensure_publication_resolution(&self, resolution: u32) -> Result<(), String> {
        if resolution != self.resolution && !self.active_publication_text_objects().is_empty() {
            return Err("Linked pages must retain the same resolution / 連結されたページは同じ解像度を保持する必要があります / 链接页面必须保持相同分辨率".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationFrameSummary {
    pub target: PageTextFrame,
    pub page_number: usize,
    pub name: String,
    pub writing_mode: crate::vector::WritingMode,
    pub characters: usize,
    pub resolution: u32,
    pub available: bool,
    pub selected: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationThreadSummary {
    #[serde(flatten)]
    pub thread: PageTextThread,
    pub overset: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationTextContext {
    /// Opaque publication snapshot stamp; not the active page's revision.
    pub revision: u64,
    pub frames: Vec<PublicationFrameSummary>,
    pub threads: Vec<PublicationThreadSummary>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationTextRequest {
    /// Copy the stamp from the last PublicationTextContext without modification.
    pub revision: u64,
    pub action: String,
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub frames: Vec<PageTextFrame>,
}
fn story_error() -> String {
    "Invalid text link selection / 文字連結の選択が不正です / 文本链接选择无效".into()
}
impl Document {
    fn publication_text_revision(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut stamp = std::collections::hash_map::DefaultHasher::new();
        self.revision.hash(&mut stamp);
        if let Some(book) = &self.pages {
            book.active.hash(&mut stamp);
            book.story_epoch.hash(&mut stamp);
            for (i, page) in book.pages.iter().enumerate() {
                page.id.hash(&mut stamp);
                self.page_document(i).map(|d| d.revision).hash(&mut stamp);
            }
        }
        // An opaque snapshot stamp, kept within JavaScript's exact integer range.
        // Page-local revisions alone can alias after navigating to another page.
        stamp.finish() & ((1u64 << 53) - 1)
    }
    pub fn publication_text_context(&self) -> PublicationTextContext {
        let book = self.pages_snapshot();
        let mut frames = Vec::new();
        for (i, summary) in book.pages.iter().enumerate() {
            if let Some(page) = self.page_document(i) {
                for layer in &page.svg_layers {
                    for object in &layer.vector_objects {
                        if let Some(text) = &object.text {
                            let target = PageTextFrame {
                                page_id: summary.id.clone(),
                                object_id: object.id.clone(),
                            };
                            let linked = self.pages.as_ref().is_some_and(|b| {
                                b.text_threads.iter().any(|t| t.frames.contains(&target))
                            }) || page
                                .vector_assets
                                .threads
                                .iter()
                                .any(|t| t.frames.contains(&object.id));
                            frames.push(PublicationFrameSummary {
                                target,
                                page_number: i + 1,
                                name: object.name.clone(),
                                writing_mode: text.writing_mode,
                                characters: text.content.chars().count(),
                                resolution: page.resolution,
                                available: layer.vector_layer
                                    && layer.visible
                                    && !layer.locked
                                    && object.visible
                                    && !page.object_is_locked(&object.id)
                                    && !text.point_text
                                    && text.box_height.is_some()
                                    && !linked,
                                selected: i == book.active
                                    && self.selected_vector_objects.contains(&object.id),
                            });
                        }
                    }
                }
            }
        }
        PublicationTextContext {
            revision: self.publication_text_revision(),
            frames,
            threads: self.pages.as_ref().map_or_else(Vec::new, |b| {
                b.text_threads
                    .iter()
                    .map(|thread| {
                        let last = thread
                            .frames
                            .last()
                            .and_then(|f| {
                                b.pages
                                    .iter()
                                    .position(|p| p.id == f.page_id)
                                    .and_then(|i| self.page_document(i))
                                    .and_then(|p| {
                                        p.svg_layers
                                            .iter()
                                            .flat_map(|l| &l.vector_objects)
                                            .find(|o| o.id == f.object_id)
                                    })
                            })
                            .and_then(|o| o.text.as_ref());
                        PublicationThreadSummary {
                            thread: thread.clone(),
                            overset: last.is_some_and(super::vector_assets::text_overset),
                        }
                    })
                    .collect()
            }),
        }
    }
    pub fn edit_publication_text(&mut self, request: PublicationTextRequest) -> Result<(), String> {
        if request.revision != self.publication_text_revision() {
            return Err("Document changed. Refresh the frame list / 文書が変更されました。フレーム一覧を更新してください / 文档已更改，请刷新文本框列表".into());
        }
        if !matches!(request.action.as_str(), "create" | "detach") {
            return Err(story_error());
        }
        let mut candidate = self.clone();
        candidate.pages.get_or_insert_with(PageBook::new);
        let (before_thread, after_thread, thread) = if request.action == "create" {
            let name = request.name.as_deref().unwrap_or("").trim();
            if name.is_empty()
                || name.chars().count() > 100
                || name.chars().any(char::is_control)
                || request.frames.len() < 2
                || request.frames.len() > 64
            {
                return Err(story_error());
            }
            let context = self.publication_text_context();
            let mut unique = std::collections::BTreeSet::new();
            for f in &request.frames {
                if !unique.insert((&f.page_id, &f.object_id))
                    || !context.frames.iter().any(|v| &v.target == f && v.available)
                {
                    return Err(story_error());
                }
            }
            let book = candidate.pages.as_mut().unwrap();
            if book.text_threads.iter().any(|t| t.name == name) {
                return Err(
                    "Story name already exists / 同じ連結名が存在します / 文本链接名称已存在"
                        .into(),
                );
            }
            let mut serial = book.story_epoch + 1;
            while book
                .text_threads
                .iter()
                .any(|t| t.id == format!("publication-ui-{serial}"))
            {
                serial += 1;
            }
            let thread = PageTextThread {
                id: format!("publication-ui-{serial}"),
                name: name.into(),
                frames: request.frames,
            };
            book.text_threads.push(thread.clone());
            (None, Some(thread.clone()), thread)
        } else {
            let id = request.id.as_deref().ok_or_else(story_error)?;
            let book = candidate.pages.as_mut().unwrap();
            let index = book
                .text_threads
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(story_error)?;
            let thread = book.text_threads.remove(index);
            (Some(thread.clone()), None, thread)
        };
        candidate.validate_publication_text().map_err(|_|"Frames must share a writing direction and resolution, within story limits / 同じ組み方向・解像度で文字数上限以内のフレームを指定してください / 请选择方向和分辨率相同且未超过文本上限的文本框".to_string())?;
        let before = self.story_objects_for_frames(&thread.frames)?;
        if after_thread.is_some() {
            candidate.recompose_publication_thread(
                self.text_layout_backend
                    .ok_or("Text layout backend unavailable")?,
                Some(&thread.id),
            )?;
        }
        let after = candidate.story_objects_for_frames(&thread.frames)?;
        if before_thread.is_none() {
            let book = candidate.pages.as_mut().unwrap();
            for history in [&mut book.story_undo, &mut book.story_redo] {
                history.retain(|e| !e.before.iter().any(|(f, _)| thread.frames.contains(f)));
            }
        }

        self.svg_layers = candidate.svg_layers;
        self.scene_journal = candidate.scene_journal;
        self.pages = candidate.pages;
        self.transform_slots.0.clear();
        self.commit_story_history(before_thread, after_thread, before, after);
        Ok(())
    }
    fn story_objects_for_frames(
        &self,
        frames: &[PageTextFrame],
    ) -> Result<Vec<(PageTextFrame, VectorObject)>, String> {
        let pages = self.pages_snapshot();
        frames
            .iter()
            .map(|f| {
                let i = pages
                    .pages
                    .iter()
                    .position(|p| p.id == f.page_id)
                    .ok_or_else(story_error)?;
                let page = self.page_document(i).ok_or_else(story_error)?;
                let object = page
                    .svg_layers
                    .iter()
                    .flat_map(|l| &l.vector_objects)
                    .find(|o| o.id == f.object_id)
                    .ok_or_else(story_error)?;
                Ok((f.clone(), object.clone()))
            })
            .collect()
    }
    fn commit_story_history(
        &mut self,
        before_thread: Option<PageTextThread>,
        after_thread: Option<PageTextThread>,
        before: Vec<(PageTextFrame, VectorObject)>,
        after: Vec<(PageTextFrame, VectorObject)>,
    ) {
        let mut affected: std::collections::BTreeSet<_> =
            before.iter().map(|(f, _)| f.page_id.clone()).collect();
        let book = self.pages.as_mut().unwrap();
        // A panel may link frames on other pages. Its invoking page must also
        // participate, so Undo there reverses the command rather than an old edit.
        affected.insert(book.pages[book.active].id.clone());
        book.story_epoch += 1;
        let token = book.story_epoch;
        book.story_redo.clear();
        let scope = after_thread
            .as_ref()
            .or(before_thread.as_ref())
            .cloned()
            .unwrap_or(PageTextThread {
                id: format!("history-{token}"),
                name: "Detached story".into(),
                frames: before.iter().map(|(f, _)| f.clone()).collect(),
            });
        for (f, _) in &before {
            book.history_frames
                .insert((f.page_id.clone(), f.object_id.clone()));
        }
        book.story_undo.push(std::sync::Arc::new(StoryEdit {
            token,
            participants: affected.clone(),
            scope,
            thread_before: before_thread,
            thread_after: after_thread,
            before,
            after,
        }));
        if book.story_undo.len() > 32 {
            book.story_undo.remove(0);
        }
        for id in affected {
            if let Some(page) = self.page_mut(&id) {
                page.redo_order.clear();
                page.vector_redo.clear();
                page.redo.clear();
                page.page_redo.clear();
                page.undo_order.push(HistoryKind::PublicationText(token));
                page.revision += 1;
            }
        }
        self.sync_publication_history_markers();
    }
    pub(super) fn is_managed_publication_text_frame(&self, id: &str) -> bool {
        self.is_publication_text_frame(id)
            || self.pages.as_ref().is_some_and(|b| {
                b.story_undo.iter().chain(&b.story_redo).any(|e| {
                    e.before
                        .iter()
                        .any(|(f, _)| f.page_id == b.pages[b.active].id && f.object_id == id)
                })
            })
    }
    pub(super) fn prune_publication_text_history(&mut self) {
        let Some(book) = &self.pages else { return };
        let valid: std::collections::BTreeSet<_> = book
            .pages
            .iter()
            .enumerate()
            .flat_map(|(i, p)| {
                self.page_document(i).into_iter().flat_map(move |d| {
                    d.svg_layers
                        .iter()
                        .flat_map(|l| &l.vector_objects)
                        .filter(|o| o.text.is_some())
                        .map(move |o| (p.id.clone(), o.id.clone()))
                })
            })
            .collect();
        let book = self.pages.as_mut().unwrap();
        let old_count = book.story_undo.len() + book.story_redo.len();
        for history in [&mut book.story_undo, &mut book.story_redo] {
            history.retain(|e| {
                e.before
                    .iter()
                    .all(|(f, _)| valid.contains(&(f.page_id.clone(), f.object_id.clone())))
            });
        }
        if old_count != book.story_undo.len() + book.story_redo.len() {
            book.story_epoch += 1;
        }
        self.sync_publication_history_markers();
    }
}
