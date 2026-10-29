//! Typst compilation: real `typst` compiler, real `typst-render` / `typst-pdf`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use typst::diag::{FileError, FileResult, Severity};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

pub struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

pub fn shared() -> Arc<Shared> {
    static S: OnceLock<Arc<Shared>> = OnceLock::new();
    S.get_or_init(|| {
        let mut book = FontBook::new();
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            for f in Font::iter(Bytes::new(data)) {
                book.push(f.info().clone());
                fonts.push(f);
            }
        }
        Arc::new(Shared {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
        })
    })
    .clone()
}

pub struct LiveWorld {
    shared: Arc<Shared>,
    main: FileId,
    source: Source,
    root: Option<PathBuf>,
}

impl LiveWorld {
    pub fn new(text: String, root: Option<PathBuf>) -> Self {
        let main = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("/main.typ").unwrap(),
        ));
        Self {
            shared: shared(),
            main,
            source: Source::new(main, text),
            root,
        }
    }
}

impl World for LiveWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.shared.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.shared.book
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(self.source.clone());
        }
        let bytes = self.file(id)?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text))
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let vp = id.get().vpath();
        let Some(root) = &self.root else {
            return Err(FileError::NotFound(vp.get_with_slash().into()));
        };
        let path = vp.realize(root).map_err(|_| FileError::AccessDenied)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|e| FileError::from_io(e, &path))
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.shared.fonts.get(index).cloned()
    }
    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        let secs = secs + offset.map(|d| d.seconds() as i64).unwrap_or(0);
        let days = secs.div_euclid(86400);
        // civil-from-days (Howard Hinnant)
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z.rem_euclid(146097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let y = if m <= 2 { y + 1 } else { y };
        Datetime::from_ymd(y as i32, m, d)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub error: bool,
    pub message: String,
    /// Byte range in the *user's* document, when it could be mapped.
    pub range: Option<std::ops::Range<usize>>,
    pub line: Option<usize>,
    /// Origin file when the diagnostic is not in the edited document (C5),
    /// e.g. `/template.typ`. `None` means the main document.
    pub file: Option<String>,
}

pub struct RenderedBlock {
    pub width: usize,
    pub height: usize,
    /// premultiplied RGBA
    pub rgba: Vec<u8>,
    /// Number of pages stacked into this image (C1).
    pub pages: usize,
    /// More pages existed than could be shown.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderStyle {
    pub width_pt: f32,
    pub font_pt: f32,
    pub line_spacing: f32,
    pub dark: bool,
    pub pixels_per_pt: f32,
}

impl RenderStyle {
    pub fn preamble(&self) -> String {
        let c = if self.dark { "#e6e6e6" } else { "#1b1b1b" };
        let lead = (self.line_spacing * 0.5).max(0.2);
        format!(
            "#set page(width: {:.0}pt, height: auto, margin: (x: 0pt, y: 1pt), fill: none)\n#set text(size: {:.1}pt, fill: rgb(\"{c}\"))\n#set par(leading: {:.2}em)\n#set table(stroke: 0.5pt + rgb(\"{c}\"))\n",
            self.width_pt, self.font_pt, lead
        )
    }
}

fn diags_from(
    world: &LiveWorld,
    errs: impl IntoIterator<Item = typst::diag::SourceDiagnostic>,
    prefix_len: usize,
    base: usize,
    limit: usize,
) -> Vec<Diag> {
    errs.into_iter()
        .map(|d| {
            // A span in an imported file must not be read as an offset into the
            // edited document (C5): only map spans that belong to the main file.
            let foreign = d.span.id().filter(|id| *id != world.main());
            let (range, file) = match foreign {
                Some(id) => (None, Some(id.vpath().get_with_slash().to_string())),
                None => {
                    let range = world.range(d.span).and_then(|r| {
                        if r.start >= prefix_len {
                            Some(
                                base + (r.start - prefix_len)
                                    ..base + (r.end - prefix_len).max(r.start - prefix_len),
                            )
                        } else {
                            None
                        }
                    });
                    (range.map(|r| r.start..r.end.min(limit.max(r.start))), None)
                }
            };
            Diag {
                error: d.severity == Severity::Error,
                message: d.message.to_string(),
                range,
                line: None,
                file,
            }
        })
        .collect()
}

/// Upper bound on the stacked height of a multi-page block, in pixels (C1).
const MAX_STACK_PX: usize = 16_000;
/// Rendered pages beyond this are dropped and reported (C1).
pub const MAX_BLOCK_PAGES: usize = 12;

/// Rasterize every page of `doc` and stack them vertically (C1).
fn render_pages(doc: &PagedDocument, ppp: f32) -> RenderedBlock {
    let opts = typst_render::RenderOptions {
        pixel_per_pt: typst::utils::Scalar::new(ppp as f64),
        ..Default::default()
    };
    let mut width = 1usize;
    let mut parts: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    for page in doc.pages().iter().take(MAX_BLOCK_PAGES) {
        let pix = typst_render::render(page, &opts);
        let (w, h) = (pix.width() as usize, pix.height() as usize);
        if !parts.is_empty() && total + h > MAX_STACK_PX {
            break;
        }
        width = width.max(w);
        total += h;
        parts.push((w, h, pix.data().to_vec()));
    }
    if parts.is_empty() {
        return RenderedBlock {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
            pages: 0,
            truncated: false,
        };
    }
    let mut rgba = vec![0u8; width * total * 4];
    let mut y = 0usize;
    for (w, h, data) in &parts {
        for row in 0..*h {
            let dst = ((y + row) * width) * 4;
            let src = row * w * 4;
            rgba[dst..dst + w * 4].copy_from_slice(&data[src..src + w * 4]);
        }
        y += h;
    }
    RenderedBlock {
        width,
        height: total,
        rgba,
        pages: parts.len(),
        truncated: doc.pages().len() > parts.len(),
    }
}

/// Compile one block (with the definitions seen so far) and rasterize all its pages, stacked.
pub fn render_block(
    block: &str,
    definitions: &str,
    style: &RenderStyle,
    root: Option<PathBuf>,
) -> Result<RenderedBlock, Vec<Diag>> {
    let pre = style.preamble();
    let src = format!("{pre}{definitions}{block}\n");
    let prefix = pre.len() + definitions.len();
    let world = LiveWorld::new(src, root);
    let out = typst::compile::<PagedDocument>(&world);
    match out.output {
        Ok(doc) => Ok(render_pages(&doc, style.pixels_per_pt)),
        Err(errs) => Err(diags_from(&world, errs, prefix, 0, block.len())),
    }
}

pub struct FullResult {
    pub diags: Vec<Diag>,
    pub pdf: Option<Vec<u8>>,
}

/// Compile the whole document (diagnostics, optionally PDF).
pub fn compile_full(text: &str, root: Option<PathBuf>, want_pdf: bool) -> FullResult {
    let world = LiveWorld::new(text.to_string(), root);
    let out = typst::compile::<PagedDocument>(&world);
    let mut diags = diags_from(&world, out.warnings, 0, 0, text.len());
    let mut pdf = None;
    match out.output {
        Ok(doc) => {
            if want_pdf {
                match typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()) {
                    Ok(b) => pdf = Some(b),
                    Err(e) => diags.extend(diags_from(&world, e, 0, 0, text.len())),
                }
            }
        }
        Err(errs) => diags.extend(diags_from(&world, errs, 0, 0, text.len())),
    }
    let src = Source::detached(text);
    for d in &mut diags {
        if let (Some(r), None) = (&d.range, &d.file) {
            d.line = src.lines().byte_to_line(r.start).map(|l| l + 1);
        }
    }
    FullResult { diags, pdf }
}

pub fn export_pdf(text: &str, root: Option<PathBuf>, path: &Path) -> Result<(), String> {
    let r = compile_full_safe(text, root, true);
    match r.pdf {
        Some(bytes) => crate::doc::atomic_write(path, &bytes).map_err(|e| e.to_string()),
        None => Err(r
            .diags
            .iter()
            .filter(|d| d.error)
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "internal error".into())
}

fn panic_diag(what: &str, p: Box<dyn std::any::Any + Send>) -> Diag {
    Diag {
        error: true,
        message: format!("{what} panicked: {}", panic_message(p)),
        range: None,
        line: None,
        file: None,
    }
}

/// [`render_block`] that turns a panic inside Typst/typst-render into an error
/// result instead of killing the worker thread (P5).
pub fn render_block_safe(
    block: &str,
    definitions: &str,
    style: &RenderStyle,
    root: Option<PathBuf>,
) -> Result<RenderedBlock, Vec<Diag>> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_block(block, definitions, style, root)
    })) {
        Ok(r) => r,
        Err(p) => Err(vec![panic_diag("render", p)]),
    }
}

/// [`compile_full`] with panic isolation (P5).
pub fn compile_full_safe(text: &str, root: Option<PathBuf>, want_pdf: bool) -> FullResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compile_full(text, root, want_pdf)
    })) {
        Ok(r) => r,
        Err(p) => FullResult {
            diags: vec![panic_diag("compile", p)],
            pdf: None,
        },
    }
}
