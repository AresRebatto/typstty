use eframe::egui::{
    self,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
    text_edit::TextEditState,
    *,
};
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};
use typst_live::commands::{self, Command};
use typst_live::doc::*;
use typst_live::engine::*;
use typst_syntax::{LinkedNode, SyntaxKind, Tag, highlight};

const WELCOME: &str = "= Typst Live\n\nWrite *real* Typst here. Blocks you are not editing are _typeset_ in place; click one (or move the caret into it) to see and edit its source.\n\n$ integral_0^1 x^2 dif x = 1/3 $\n\n- `Ctrl+B` bold, `Ctrl+I` italic\n- `Alt+C` inline code, `Alt+Shift+C` code block\n- `Alt+M` inline math, `Alt+Shift+M` display math\n";

/// Bold families for the live-formatted `*strong*` text (U2). Taken from the
/// fonts Typst already embeds; falls back to the regular families.
fn register_bold_fonts(ctx: &egui::Context) {
    use typst::foundations::Bytes;
    use typst::text::Font;
    let mut defs = FontDefinitions::default();
    let mut found: HashMap<&'static str, Vec<u8>> = HashMap::new();
    for data in typst_assets::fonts() {
        for f in Font::iter(Bytes::new(data)) {
            let info = f.info();
            let bold = info.variant.weight.to_number() >= 700
                && info.variant.style == typst::text::FontStyle::Normal;
            if !bold {
                continue;
            }
            match info.family.as_str() {
                "DejaVu Sans Mono" => {
                    found.entry("bold-mono").or_insert_with(|| data.to_vec());
                }
                "Libertinus Serif" => {
                    found.entry("bold-prop").or_insert_with(|| data.to_vec());
                }
                _ => {}
            }
        }
    }
    for (fam, regular) in [
        ("bold-mono", FontFamily::Monospace),
        ("bold-prop", FontFamily::Proportional),
    ] {
        let mut list = defs.families.get(&regular).cloned().unwrap_or_default();
        if let Some(bytes) = found.get(fam) {
            defs.font_data.insert(
                fam.to_string(),
                Arc::new(FontData::from_owned(bytes.clone())),
            );
            list.insert(0, fam.to_string());
        }
        defs.families.insert(FontFamily::Name(fam.into()), list);
    }
    ctx.set_fonts(defs);
}

fn editor_id() -> Id {
    static ID: std::sync::OnceLock<Id> = std::sync::OnceLock::new();
    *ID.get_or_init(|| Id::unique("active_editor"))
}

enum Job {
    Render {
        key: u64,
        block: String,
        defs: String,
        style: RenderStyle,
        root: Option<PathBuf>,
    },
    Full {
        rev: u64,
        text: String,
        root: Option<PathBuf>,
    },
}
enum Done {
    Render {
        key: u64,
        res: Result<RenderedBlock, Vec<Diag>>,
    },
    /// Job dropped because its block is no longer on screen / wanted (P2).
    Skipped {
        key: u64,
    },
    Full {
        rev: u64,
        diags: Vec<Diag>,
    },
}
/// A pending job that has not answered within this time is considered lost (P4).
const PENDING_TIMEOUT: Duration = Duration::from_secs(30);
enum Cached {
    Pending(Instant),
    Ready { tex: TextureHandle, size: Vec2 },
    Failed(Vec<Diag>),
}

struct Settings {
    dark: bool,
    font_size: f32,
    line_spacing: f32,
    monospace: bool,
    autosave: bool,
    autosave_secs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: false,
            font_size: 15.0,
            line_spacing: 1.45,
            monospace: true,
            autosave: false,
            autosave_secs: 5,
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("typst-live"))
}

impl Settings {
    fn serialize(&self, last_file: Option<&PathBuf>) -> String {
        let mut s = format!(
            "dark={}\nfont_size={}\nline_spacing={}\nmonospace={}\nautosave={}\nautosave_secs={}\n",
            self.dark,
            self.font_size,
            self.line_spacing,
            self.monospace,
            self.autosave,
            self.autosave_secs
        );
        if let Some(p) = last_file {
            s.push_str(&format!("last_file={}\n", p.display()));
        }
        s
    }
    /// F4: parse the small `key=value` settings file; unknown / invalid keys are ignored.
    fn parse(text: &str) -> (Settings, Option<PathBuf>) {
        let mut st = Settings::default();
        let mut last = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "dark" => st.dark = v.trim().parse().unwrap_or(st.dark),
                "font_size" => {
                    st.font_size = v
                        .trim()
                        .parse::<f32>()
                        .map(|x| x.clamp(10.0, 28.0))
                        .unwrap_or(st.font_size)
                }
                "line_spacing" => {
                    st.line_spacing = v
                        .trim()
                        .parse::<f32>()
                        .map(|x| x.clamp(1.0, 2.2))
                        .unwrap_or(st.line_spacing)
                }
                "monospace" => st.monospace = v.trim().parse().unwrap_or(st.monospace),
                "autosave" => st.autosave = v.trim().parse().unwrap_or(st.autosave),
                "autosave_secs" => {
                    st.autosave_secs = v
                        .trim()
                        .parse::<u32>()
                        .map(|x| x.clamp(1, 60))
                        .unwrap_or(st.autosave_secs)
                }
                "last_file" => last = Some(PathBuf::from(v)),
                _ => {}
            }
        }
        (st, last)
    }
    fn load() -> (Settings, Option<PathBuf>) {
        config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("settings.txt")).ok())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }
}

/// F3: recovery snapshot next to the document (or in the temp dir for untitled).
fn recovery_path(doc_path: Option<&PathBuf>) -> PathBuf {
    match doc_path {
        Some(p) => {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "doc".into());
            p.with_file_name(format!(".{name}.recovery"))
        }
        None => std::env::temp_dir().join("typst-live-untitled.recovery"),
    }
}

struct App {
    doc: Document,
    path: Option<PathBuf>,
    settings: Settings,
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    cache: HashMap<u64, Cached>,
    /// Last good image per block position, valid only while the neighbours are
    /// unchanged (C7): (texture, size, neighbour hash).
    last_good: HashMap<usize, (TextureHandle, Vec2, u64)>,
    diags: Vec<Diag>,
    diag_requested: u64,
    /// Highest `Full` generation accepted so far (C6).
    diag_received: u64,
    live: Arc<Mutex<HashSet<u64>>>,
    pending_cmd: Option<Command>,
    cmd_ctx: commands::CommandContext,
    disk_mtime: Option<SystemTime>,
    recovery_version: u64,
    last_settings: String,
    last_change: Instant,
    show_diags: bool,
    show_outline: bool,
    show_find: bool,
    show_settings: bool,
    find: String,
    replace: String,
    pos_cache: Option<(u64, usize, usize, usize)>,
    hash_cache: Option<(u64, usize, Arc<Vec<u64>>)>,
    sync_cursor: bool,
    want_focus: bool,
    last_active: Range<usize>,
    scroll_to: Option<usize>,
    status: String,
    title: String,
    last_visuals_dark: Option<bool>,
}

fn spawn_workers(
    live: Arc<Mutex<HashSet<u64>>>,
) -> (
    mpsc::Sender<Job>,
    mpsc::Receiver<Done>,
    Arc<Mutex<Option<egui::Context>>>,
) {
    let (jtx, jrx) = mpsc::channel::<Job>();
    let (dtx, drx) = mpsc::channel::<Done>();
    let jrx = Arc::new(Mutex::new(jrx));
    let ctx_slot: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 6);
    for _ in 0..n {
        let jrx = jrx.clone();
        let dtx = dtx.clone();
        let slot = ctx_slot.clone();
        let live = live.clone();
        std::thread::spawn(move || {
            loop {
                let job = { jrx.lock().unwrap().recv() };
                let Ok(job) = job else { break };
                let done = match job {
                    Job::Render {
                        key,
                        block,
                        defs,
                        style,
                        root,
                    } => {
                        // P2: skip jobs whose block left the screen / changed meanwhile.
                        if live.lock().map(|l| l.contains(&key)).unwrap_or(true) {
                            Done::Render {
                                key,
                                res: render_block_safe(&block, &defs, &style, root),
                            }
                        } else {
                            Done::Skipped { key }
                        }
                    }
                    Job::Full { rev, text, root } => Done::Full {
                        rev,
                        diags: compile_full_safe(&text, root, false).diags,
                    },
                };
                let _ = dtx.send(done);
                if let Some(c) = slot.lock().unwrap().as_ref() {
                    c.request_repaint();
                }
            }
        });
    }
    (jtx, drx, ctx_slot)
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        register_bold_fonts(&cc.egui_ctx);
        let live: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));
        let (jobs, done, slot) = spawn_workers(live.clone());
        *slot.lock().unwrap() = Some(cc.egui_ctx.clone());
        let (settings, last_file) = Settings::load();
        let mut app = App {
            doc: Document::new(WELCOME.to_string()),
            path: None,
            settings,
            jobs,
            done,
            cache: HashMap::new(),
            last_good: HashMap::new(),
            diags: vec![],
            diag_requested: u64::MAX,
            diag_received: 0,
            live,
            pending_cmd: None,
            cmd_ctx: commands::CommandContext::default(),
            disk_mtime: None,
            recovery_version: u64::MAX,
            last_settings: String::new(),
            last_change: Instant::now(),
            show_diags: false,
            show_outline: false,
            show_find: false,
            show_settings: false,
            find: String::new(),
            replace: String::new(),
            pos_cache: None,
            hash_cache: None,
            sync_cursor: true,
            want_focus: true,
            last_active: 0..0,
            scroll_to: None,
            status: String::new(),
            title: String::new(),
            last_visuals_dark: None,
        };
        if let Some(p) = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .or(last_file.filter(|p| p.is_file()))
        {
            app.open_path(p);
        }
        app
    }

    fn root(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }

    // ---------- files ----------
    fn confirm_discard(&self) -> bool {
        if !self.doc.is_dirty() {
            return true;
        }
        rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Discard unsaved changes?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }
    fn new_doc(&mut self) {
        if self.confirm_discard() {
            self.doc.load(String::new());
            self.disk_mtime = None;
            self.reset_doc_state();
            self.path = None;
            self.sync_cursor = true;
            self.want_focus = true;
        }
    }
    fn reset_doc_state(&mut self) {
        self.diag_requested = u64::MAX;
        self.diag_received = 0;
        self.diags.clear();
        self.last_good.clear();
        self.recovery_version = u64::MAX;
        self.last_change = Instant::now();
    }
    /// F3: a newer recovery snapshot exists next to the file → offer to restore it.
    fn offer_recovery(&mut self, p: &PathBuf) {
        let rp = recovery_path(Some(p));
        let newer = match (
            std::fs::metadata(&rp).and_then(|m| m.modified()),
            std::fs::metadata(p).and_then(|m| m.modified()),
        ) {
            (Ok(r), Ok(f)) => r > f,
            _ => false,
        };
        if !newer {
            return;
        }
        let yes = rfd::MessageDialog::new()
            .set_title("Recovery file found")
            .set_description("Unsaved changes from a previous session were found. Restore them?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes;
        if let (true, Ok(raw)) = (yes, std::fs::read_to_string(&rp)) {
            let (t, _, _) = normalize_input(&raw);
            self.doc.text = t;
            self.doc.set_selection(0, 0);
            self.doc.commit(true);
            self.status = "Recovered unsaved changes".into();
        }
    }
    fn open_path(&mut self, p: PathBuf) {
        match std::fs::read(&p) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => {
                    self.doc.load_raw(&t); // E7: BOM / CRLF remembered, text LF-normalised
                    self.doc.mark_saved();
                    self.disk_mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                    self.reset_doc_state();
                    self.status = "Opened".into();
                    self.offer_recovery(&p);
                    self.path = Some(p);
                    self.sync_cursor = true;
                }
                Err(_) => self.status = "File is not valid UTF-8".into(),
            },
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }
    fn open(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Typst", &["typ"])
            .pick_file()
        {
            self.open_path(p);
        }
    }
    fn save(&mut self) -> bool {
        let Some(p) = self.path.clone() else {
            return self.save_as();
        };
        // F2: refuse to silently overwrite a file changed by someone else.
        let on_disk = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
        if self.disk_mtime.is_some() && on_disk.is_some() && on_disk != self.disk_mtime {
            let r = rfd::MessageDialog::new()
                .set_title("File changed on disk")
                .set_description(
                    "The file was modified outside Typst Live. Overwrite it with your version?",
                )
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if r != rfd::MessageDialogResult::Yes {
                self.status = "Save cancelled: file changed on disk".into();
                return false;
            }
        }
        // F1: atomic write (temp file + fsync + rename).
        match typst_live::doc::atomic_write(&p, &self.doc.disk_bytes()) {
            Ok(_) => {
                self.doc.mark_saved();
                self.disk_mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                let _ = std::fs::remove_file(recovery_path(Some(&p)));
                self.status = "Saved".into();
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                false
            }
        }
    }
    fn save_as(&mut self) -> bool {
        if let Some(mut p) = rfd::FileDialog::new()
            .add_filter("Typst", &["typ"])
            .set_file_name("document.typ")
            .save_file()
        {
            if p.extension().is_none() {
                p.set_extension("typ");
            }
            self.path = Some(p);
            self.disk_mtime = None; // new target: nothing to compare against
            return self.save();
        }
        self.status = "Save cancelled".into(); // F6
        false
    }
    fn export_pdf(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("document.pdf")
            .save_file()
        {
            self.status = match export_pdf(&self.doc.text, self.root(), &p) {
                Ok(_) => "PDF exported".into(),
                Err(e) => format!("PDF export failed: {e}"),
            };
        }
    }

    // ---------- editing ----------
    fn run_command(&mut self, c: Command) {
        let e = commands::apply_ctx(
            c,
            &self.doc.text,
            self.doc.anchor,
            self.doc.head,
            &mut self.cmd_ctx,
        );
        self.doc.apply_edit(e);
        self.after_external_edit();
    }
    fn after_external_edit(&mut self) {
        self.sync_cursor = true;
        self.want_focus = true;
        self.last_change = Instant::now();
    }
    fn find_next(&mut self, backwards: bool) {
        if self.find.is_empty() {
            return;
        }
        let t = &self.doc.text;
        let sel = self.doc.sel();
        let hit = if backwards {
            t[..sel.start]
                .rfind(&self.find)
                .or_else(|| t.rfind(&self.find))
        } else {
            t[sel.end..]
                .find(&self.find)
                .map(|i| i + sel.end)
                .or_else(|| t.find(&self.find))
        };
        if let Some(i) = hit {
            self.doc.set_selection(i, i + self.find.len());
            self.scroll_to = Some(i);
            self.after_external_edit();
        } else {
            self.status = "No match".into();
        }
    }
    fn replace_one(&mut self) {
        let sel = self.doc.sel();
        if !self.find.is_empty() && self.doc.text[sel.clone()] == self.find {
            self.doc.text.replace_range(sel.clone(), &self.replace);
            let e = sel.start + self.replace.len();
            self.doc.set_selection(e, e);
            self.doc.commit(true);
            self.after_external_edit();
        }
        self.find_next(false);
    }
    fn replace_all(&mut self) {
        if self.find.is_empty() {
            return;
        }
        let n = self.doc.text.matches(&self.find).count();
        self.doc.text = self.doc.text.replace(&self.find, &self.replace);
        self.doc.set_selection(0, 0);
        self.doc.commit(true);
        self.status = format!("Replaced {n}");
        self.after_external_edit();
    }

    // ---------- rendering ----------
    fn style(&self, width: f32, ppp: f32) -> RenderStyle {
        RenderStyle {
            width_pt: width,
            font_pt: self.settings.font_size * 0.95,
            line_spacing: self.settings.line_spacing,
            dark: self.settings.dark,
            pixels_per_pt: ppp,
        }
    }
    fn key_for(block_hash: u64, defs_hash: u64, s: &RenderStyle) -> u64 {
        let mut h = DefaultHasher::new();
        block_hash.hash(&mut h);
        defs_hash.hash(&mut h);
        (
            s.width_pt.to_bits(),
            s.font_pt.to_bits(),
            s.line_spacing.to_bits(),
            s.dark,
            s.pixels_per_pt.to_bits(),
        )
            .hash(&mut h);
        h.finish()
    }
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(d) = self.done.try_recv() {
            match d {
                Done::Render { key, res } => {
                    let c = match res {
                        Ok(r) => {
                            if r.truncated {
                                self.status = format!(
                                    "A block has more than {MAX_BLOCK_PAGES} pages: only the first ones are shown"
                                );
                            }
                            let ppp = ctx.pixels_per_point();
                            let img =
                                ColorImage::from_rgba_premultiplied([r.width, r.height], &r.rgba);
                            let tex =
                                ctx.load_texture(format!("b{key}"), img, TextureOptions::LINEAR);
                            Cached::Ready {
                                tex,
                                size: vec2(
                                    r.width as f32 / self.render_ppp(ppp),
                                    r.height as f32 / self.render_ppp(ppp),
                                ),
                            }
                        }
                        Err(d) => Cached::Failed(d),
                    };
                    self.cache.insert(key, c);
                }
                Done::Skipped { key } => {
                    self.cache.remove(&key);
                }
                Done::Full { rev, diags } => {
                    // C6: accept the newest result that is not older than the last
                    // accepted one, even if the user kept typing meanwhile.
                    if rev >= self.diag_received {
                        self.diag_received = rev;
                        self.diags = diags;
                    }
                }
            }
        }
    }
    fn render_ppp(&self, ppp: f32) -> f32 {
        (ppp * 2.0).round() / 2.0
    }

    /// Highlighted layout of the active block. With `reveal = Some(selection)` the
    /// delimiters of `*bold*`, `_italic_` and inline `$math$` are hidden (and the
    /// content styled) unless the selection touches them, i.e. lies within one
    /// byte of the node (U2, option A). The source text itself is never changed.
    fn syntax_job(
        &self,
        text: &str,
        wrap: f32,
        ui: &Ui,
        reveal: Option<(usize, usize)>,
    ) -> LayoutJob {
        let mono = self.settings.monospace;
        let font = FontId::new(
            self.settings.font_size,
            if mono {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            },
        );
        let bold_font = FontId::new(
            self.settings.font_size,
            FontFamily::Name(if mono { "bold-mono" } else { "bold-prop" }.into()),
        );
        let tiny = FontId::new(0.5, FontFamily::Monospace);
        let lh = self.settings.font_size * self.settings.line_spacing;
        let base = ui.visuals().text_color();
        let dark = self.settings.dark;
        let col = |t: Tag| -> Color32 {
            let (l, d) = match t {
                Tag::Comment => (Color32::from_gray(130), Color32::from_gray(120)),
                Tag::Heading | Tag::Strong | Tag::Emph | Tag::ListMarker | Tag::ListTerm => (
                    Color32::from_rgb(0, 82, 163),
                    Color32::from_rgb(110, 175, 255),
                ),
                Tag::Raw => (
                    Color32::from_rgb(30, 120, 60),
                    Color32::from_rgb(130, 200, 140),
                ),
                Tag::MathDelimiter | Tag::MathOperator | Tag::MathGroupingParens => (
                    Color32::from_rgb(176, 90, 0),
                    Color32::from_rgb(240, 170, 90),
                ),
                Tag::Keyword | Tag::Operator => (
                    Color32::from_rgb(130, 40, 160),
                    Color32::from_rgb(200, 140, 230),
                ),
                Tag::Number | Tag::String => (
                    Color32::from_rgb(0, 120, 130),
                    Color32::from_rgb(110, 200, 210),
                ),
                Tag::Function | Tag::Interpolated => (
                    Color32::from_rgb(40, 80, 200),
                    Color32::from_rgb(140, 170, 255),
                ),
                Tag::Label | Tag::Ref | Tag::Link => (
                    Color32::from_rgb(0, 130, 160),
                    Color32::from_rgb(100, 200, 230),
                ),
                Tag::Error => (
                    Color32::from_rgb(200, 30, 30),
                    Color32::from_rgb(255, 110, 110),
                ),
                _ => return base,
            };
            if dark { d } else { l }
        };
        #[derive(Clone, Copy, Default)]
        struct Deco {
            bold: bool,
            italic: bool,
            hidden: bool,
        }
        struct Ctx<'a> {
            col: &'a dyn Fn(Tag) -> Color32,
            base: Color32,
            reveal: Option<(usize, usize)>,
        }
        let root = typst_syntax::parse(text);
        let linked = LinkedNode::new(&root);
        let mut spans: Vec<(Range<usize>, Color32, Deco)> = vec![];
        fn near(r: &Range<usize>, sel: (usize, usize)) -> bool {
            sel.0 <= r.end + 1 && sel.1 + 1 >= r.start
        }
        fn walk(
            n: &LinkedNode,
            cx: &Ctx,
            deco: Deco,
            out: &mut Vec<(Range<usize>, Color32, Deco)>,
        ) {
            let kind = n.kind();
            let mut d = deco;
            let mut hide_delims = false;
            if let Some(sel) = cx.reveal {
                let r = n.range();
                match kind {
                    SyntaxKind::Strong => {
                        d.bold = true;
                        hide_delims = !near(&r, sel);
                    }
                    SyntaxKind::Emph => {
                        d.italic = true;
                        hide_delims = !near(&r, sel);
                    }
                    SyntaxKind::Equation => {
                        // inline math only: `$x$`, not the display form `$ x $`
                        let inline = n
                            .children()
                            .nth(1)
                            .is_some_and(|c| c.kind() != SyntaxKind::Space);
                        hide_delims = inline && !near(&r, sel);
                    }
                    _ => {}
                }
            }
            if n.children().len() == 0 {
                let mut dd = d;
                if hide_delims_leaf(kind) && deco.hidden {
                    dd.hidden = true;
                }
                out.push((n.range(), highlight(n).map(cx.col).unwrap_or(cx.base), dd));
            } else {
                for c in n.children() {
                    let mut cd = d;
                    if hide_delims
                        && matches!(
                            c.kind(),
                            SyntaxKind::Star | SyntaxKind::Underscore | SyntaxKind::Dollar
                        )
                    {
                        cd.hidden = true;
                    }
                    walk(&c, cx, cd, out);
                }
            }
        }
        fn hide_delims_leaf(k: SyntaxKind) -> bool {
            matches!(
                k,
                SyntaxKind::Star | SyntaxKind::Underscore | SyntaxKind::Dollar
            )
        }
        let cx = Ctx {
            col: &col,
            base,
            reveal,
        };
        walk(&linked, &cx, Deco::default(), &mut spans);
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap;
        let mut pos = 0;
        let fmt = |c: Color32, d: Deco| {
            if d.hidden {
                TextFormat {
                    font_id: tiny.clone(),
                    color: Color32::TRANSPARENT,
                    line_height: Some(lh),
                    ..Default::default()
                }
            } else {
                TextFormat {
                    font_id: if d.bold {
                        bold_font.clone()
                    } else {
                        font.clone()
                    },
                    color: c,
                    italics: d.italic,
                    line_height: Some(lh),
                    ..Default::default()
                }
            }
        };
        for (r, c, d) in spans {
            if r.start > pos {
                job.append(&text[pos..r.start], 0.0, fmt(base, Deco::default()));
            }
            if r.end > r.start && r.start >= pos {
                job.append(&text[r.clone()], 0.0, fmt(c, d));
                pos = r.end;
            }
        }
        if pos < text.len() {
            job.append(&text[pos..], 0.0, fmt(base, Deco::default()));
        }
        if text.is_empty() {
            job.append("", 0.0, fmt(base, Deco::default()));
        }
        job
    }

    fn editor_ok(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused().is_none_or(|f| f == editor_id()))
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut cmd: Option<Command> = None;
        let mut act = 0u8;
        let mut select_all = false;
        let editor_ok = self.editor_ok(ctx);
        ctx.input_mut(|i| {
            let (c, s) = (Modifiers::COMMAND, Modifiers::SHIFT);
            if editor_ok {
                // Alt+C / Alt+M are matched on the *physical* key so that macOS
                // Option+C ("ç") works (D1); AltGr (Ctrl+Alt on Windows) is never a
                // shortcut, it types characters such as { } [ ] @ # (U3).
                let mut hit = None;
                for e in &i.events {
                    if let Event::Key { key, physical_key, pressed: true, modifiers, .. } = e
                        && modifiers.alt && !modifiers.ctrl && !modifiers.command && !modifiers.mac_cmd {
                            let k = physical_key.unwrap_or(*key);
                            if k == Key::C || k == Key::M {
                                hit = Some((k, modifiers.shift));
                                break;
                            }
                        }
                }
                if let Some((k, sh)) = hit {
                    cmd = Some(match (k, sh) {
                        (Key::C, true) => Command::CodeBlock,
                        (Key::C, false) => Command::InlineCode,
                        (_, true) => Command::DisplayMath,
                        _ => Command::InlineMath,
                    });
                    // drop the key and any stray text it produced (e.g. "ç")
                    i.events.retain(|e| !matches!(e, Event::Text(_)) && !matches!(e, Event::Key { modifiers, .. } if modifiers.alt && !modifiers.ctrl));
                }
                if cmd.is_none() {
                    if i.consume_key(c, Key::B) {
                        cmd = Some(Command::Bold);
                    } else if i.consume_key(c, Key::I) {
                        cmd = Some(Command::Italic);
                    } else if i.consume_key(c, Key::A) {
                        select_all = true;
                    }
                }
            }
            if cmd.is_none() && !select_all {
                if i.consume_key(c | s, Key::Z) || i.consume_key(c, Key::Y) {
                    act = 1;
                } else if i.consume_key(c, Key::Z) {
                    act = 2;
                } else if i.consume_key(c | s, Key::S) {
                    act = 3;
                } else if i.consume_key(c, Key::S) {
                    act = 4;
                } else if i.consume_key(c, Key::O) {
                    act = 5;
                } else if i.consume_key(c, Key::N) {
                    act = 6;
                } else if i.consume_key(c, Key::F) || i.consume_key(c, Key::H) {
                    act = 7;
                }
            }
            if cmd.is_some() {
                i.events.retain(|e| !matches!(e, Event::Text(_)));
            }
        });
        if let Some(c) = cmd {
            if self.doc.multi {
                self.run_command(c);
            } else {
                // E6: run after the editor processed this frame's input, so the
                // selection / text it operates on are current.
                self.pending_cmd = Some(c);
            }
        }
        if select_all {
            let blocks = self.doc.blocks_rc();
            let caret = self.doc.head.min(self.doc.text.len());
            let active = active_range(&self.doc.text, &blocks, caret);
            self.doc.select_all_step(active); // U4: block, then whole document
            self.after_external_edit();
        }
        match act {
            1 => {
                if self.doc.redo() {
                    self.after_external_edit();
                }
            }
            2 => {
                if self.doc.undo() {
                    self.after_external_edit();
                }
            }
            3 => {
                self.save_as();
            }
            4 => {
                self.save();
            }
            5 => self.open(),
            6 => self.new_doc(),
            7 => self.show_find = true,
            _ => {}
        }
        if self.doc.multi && editor_ok {
            self.handle_multi_input(ctx);
        }
    }

    /// Keyboard handling while the selection is owned by the document model and
    /// may span several blocks (U4 / E10 / D3).
    fn handle_multi_input(&mut self, ctx: &egui::Context) {
        let mut typed = String::new();
        let mut paste: Option<String> = None;
        let (mut copy, mut cut, mut delete) = (false, false, false);
        let mut nav: Option<(Dir, bool)> = None; // (direction, shift)
        let mut collapse_to: Option<usize> = None;
        let sel = self.doc.sel();
        ctx.input_mut(|i| {
            for e in &i.events {
                match e {
                    Event::Text(t) => typed.push_str(t),
                    Event::Paste(t) => paste = Some(t.clone()),
                    Event::Copy => copy = true,
                    Event::Cut => cut = true,
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let ctrl = modifiers.command || modifiers.ctrl;
                        if modifiers.alt {
                            continue;
                        }
                        match key {
                            Key::Backspace | Key::Delete => delete = true,
                            Key::Enter => typed.push('\n'),
                            Key::Escape => collapse_to = Some(self.doc.head),
                            Key::ArrowLeft => nav = Some((Dir::Left, modifiers.shift)),
                            Key::ArrowRight => nav = Some((Dir::Right, modifiers.shift)),
                            Key::ArrowUp => nav = Some((Dir::Up, modifiers.shift)),
                            Key::ArrowDown => nav = Some((Dir::Down, modifiers.shift)),
                            Key::Home => {
                                nav = Some((
                                    if ctrl { Dir::DocStart } else { Dir::Home },
                                    modifiers.shift,
                                ))
                            }
                            Key::End => {
                                nav = Some((
                                    if ctrl { Dir::DocEnd } else { Dir::End },
                                    modifiers.shift,
                                ))
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            i.events.retain(|e| match e {
                Event::Text(_) | Event::Paste(_) | Event::Copy | Event::Cut => false,
                Event::Key { key, modifiers, .. } => {
                    modifiers.alt
                        || !matches!(
                            key,
                            Key::Backspace
                                | Key::Delete
                                | Key::Enter
                                | Key::Escape
                                | Key::ArrowLeft
                                | Key::ArrowRight
                                | Key::ArrowUp
                                | Key::ArrowDown
                                | Key::Home
                                | Key::End
                        )
                }
                _ => true,
            });
        });
        if copy || cut {
            ctx.copy_text(self.doc.text[sel.clone()].to_string());
        }
        if let Some(p) = paste {
            self.doc.replace_selection(&p.replace("\r\n", "\n"));
            self.after_external_edit();
        } else if cut || delete {
            self.doc.replace_selection("");
            self.after_external_edit();
        } else if !typed.is_empty() {
            self.doc.replace_selection(&typed);
            self.after_external_edit();
        } else if let Some((dir, shift)) = nav {
            let blocks = self.doc.blocks_rc();
            if shift {
                let h = move_head(&self.doc.text, &blocks, self.doc.head, dir);
                self.set_selection_smart(self.doc.anchor, h, &blocks);
            } else {
                let c = match dir {
                    Dir::Left | Dir::Up | Dir::Home => sel.start,
                    Dir::Right | Dir::Down | Dir::End => sel.end,
                    Dir::DocStart => 0,
                    Dir::DocEnd => self.doc.text.len(),
                };
                self.doc.set_selection(c, c);
                self.after_external_edit();
            }
        } else if let Some(c) = collapse_to {
            self.doc.set_selection(c, c);
            self.after_external_edit();
        }
    }

    /// Selection owned by the block editor when it fits in one block, by the
    /// document model otherwise.
    fn set_selection_smart(&mut self, anchor: usize, head: usize, blocks: &[Block]) {
        if within_one_block(blocks, anchor, head) {
            self.doc.set_selection(anchor, head);
        } else {
            self.doc.set_multi_selection(anchor, head);
        }
        self.after_external_edit();
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").clicked() {
                self.new_doc();
            }
            if ui.button("Open").clicked() {
                self.open();
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            if ui
                .button("PDF")
                .on_hover_text("Export PDF with the Typst compiler")
                .clicked()
            {
                self.export_pdf();
            }
            ui.separator();
            let tb = |ui: &mut Ui, label: RichText, tip: &str| {
                ui.button(label).on_hover_text(tip).clicked()
            };
            if tb(ui, RichText::new("B").strong(), "Bold  Ctrl+B") {
                self.run_command(Command::Bold);
            }
            if tb(ui, RichText::new("I").italics(), "Italic  Ctrl+I") {
                self.run_command(Command::Italic);
            }
            if tb(ui, RichText::new("`c`").monospace(), "Inline code  Alt+C") {
                self.run_command(Command::InlineCode);
            }
            if tb(
                ui,
                RichText::new("```").monospace(),
                "Code block  Alt+Shift+C",
            ) {
                self.run_command(Command::CodeBlock);
            }
            if tb(ui, RichText::new("$x$"), "Inline math  Alt+M") {
                self.run_command(Command::InlineMath);
            }
            if tb(ui, RichText::new("$ x $"), "Display math  Alt+Shift+M") {
                self.run_command(Command::DisplayMath);
            }
            ui.separator();
            ui.toggle_value(&mut self.show_outline, "Outline");
            ui.toggle_value(&mut self.show_find, "Find");
            ui.toggle_value(&mut self.show_settings, "Settings");
        });
        if self.show_find {
            ui.horizontal(|ui| {
                let r = ui.add(
                    TextEdit::singleline(&mut self.find)
                        .hint_text("Find")
                        .desired_width(160.0),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.find_next(false);
                    r.request_focus();
                }
                ui.add(
                    TextEdit::singleline(&mut self.replace)
                        .hint_text("Replace")
                        .desired_width(160.0),
                );
                if ui.button("◀").clicked() {
                    self.find_next(true);
                }
                if ui.button("▶").clicked() {
                    self.find_next(false);
                }
                if ui.button("Replace").clicked() {
                    self.replace_one();
                }
                if ui.button("All").clicked() {
                    self.replace_all();
                }
                if ui.button("✕").clicked() {
                    self.show_find = false;
                }
            });
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.settings.dark, "Dark theme");
                ui.add(Slider::new(&mut self.settings.font_size, 10.0..=28.0).text("Font size"));
                ui.add(
                    Slider::new(&mut self.settings.line_spacing, 1.0..=2.2).text("Line spacing"),
                );
                ui.checkbox(&mut self.settings.monospace, "Monospace source font");
                ui.checkbox(&mut self.settings.autosave, "Autosave (needs a file path)");
                ui.add_enabled(
                    self.settings.autosave,
                    Slider::new(&mut self.settings.autosave_secs, 1..=60)
                        .text("seconds after last edit"),
                );
            });
        self.show_settings = open;
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.is_dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            match rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("Save before closing?")
                .set_buttons(rfd::MessageButtons::YesNoCancel)
                .show()
            {
                rfd::MessageDialogResult::Yes => {
                    if self.save() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                        self.doc.mark_saved();
                    } else {
                        self.status = "Close cancelled: document not saved".into(); // F6
                    }
                }
                rfd::MessageDialogResult::No => {
                    let _ = std::fs::remove_file(recovery_path(self.path.as_ref()));
                    self.doc.mark_saved();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                _ => {}
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.last_visuals_dark != Some(self.settings.dark) {
            ctx.set_visuals(if self.settings.dark {
                Visuals::dark()
            } else {
                Visuals::light()
            });
            self.last_visuals_dark = Some(self.settings.dark);
        }
        self.drain(&ctx);
        self.handle_shortcuts(&ctx);

        // title / dirty indicator
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled.typ".into());
        let title = format!(
            "{}{} — Typst Live",
            if self.doc.is_dirty() { "● " } else { "" },
            name
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        // autosave
        if self.settings.autosave && self.path.is_some() && self.doc.is_dirty() {
            let wait = Duration::from_secs(self.settings.autosave_secs as u64);
            if self.last_change.elapsed() >= wait {
                self.save();
            } else {
                ctx.request_repaint_after(
                    wait - self.last_change.elapsed() + Duration::from_millis(20),
                );
            }
        }

        // F4: persist settings and last file when they change
        let ser = self.settings.serialize(self.path.as_ref());
        if ser != self.last_settings {
            if let Some(d) = config_dir() {
                let _ = std::fs::create_dir_all(&d);
                let _ = typst_live::doc::atomic_write(&d.join("settings.txt"), ser.as_bytes());
            }
            self.last_settings = ser;
        }

        // F3: recovery snapshot, separate from the real file
        if self.doc.is_dirty() && self.recovery_version != self.doc.version {
            if self.last_change.elapsed() > Duration::from_secs(3) {
                let _ = typst_live::doc::atomic_write(
                    &recovery_path(self.path.as_ref()),
                    &self.doc.disk_bytes(),
                );
                self.recovery_version = self.doc.version;
            } else {
                ctx.request_repaint_after(Duration::from_millis(3100));
            }
        }

        // debounced full-document diagnostics
        if self.diag_requested != self.doc.version {
            if self.last_change.elapsed() > Duration::from_millis(350) {
                self.diag_requested = self.doc.version;
                let _ = self.jobs.send(Job::Full {
                    rev: self.doc.version,
                    text: self.doc.text.clone(),
                    root: self.root(),
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(380));
            }
        }

        Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let errs = self.diags.iter().filter(|d| d.error).count();
                let warns = self.diags.len() - errs;
                let label = if errs > 0 {
                    RichText::new(format!("● {errs} error(s)"))
                        .color(Color32::from_rgb(220, 70, 70))
                } else if warns > 0 {
                    RichText::new(format!("● {warns} warning(s)"))
                        .color(Color32::from_rgb(210, 150, 30))
                } else {
                    RichText::new("✓ compiles").weak()
                };
                if ui.selectable_label(self.show_diags, label).clicked() {
                    self.show_diags = !self.show_diags;
                }
                ui.separator();
                let head = self.doc.head.min(self.doc.text.len());
                // P1: line/column only recomputed when version or caret changed
                let (line, col) = match self.pos_cache {
                    Some((v, h, l, c)) if v == self.doc.version && h == head => (l, c),
                    _ => {
                        let line = self.doc.text[..head].matches('\n').count() + 1;
                        let col = self.doc.text[..head]
                            .rsplit('\n')
                            .next()
                            .map(|l| l.chars().count())
                            .unwrap_or(0)
                            + 1;
                        self.pos_cache = Some((self.doc.version, head, line, col));
                        (line, col)
                    }
                };
                ui.label(RichText::new(format!("Ln {line}, Col {col}")).weak());
                ui.label(
                    RichText::new(if self.doc.is_dirty() {
                        "● unsaved"
                    } else {
                        "saved"
                    })
                    .weak(),
                );
                ui.label(RichText::new(&self.status).weak());
            });
        });
        if self.show_diags {
            Panel::bottom("diags")
                .resizable(true)
                .default_size(120.0)
                .show(ui, |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        if self.diags.is_empty() {
                            ui.weak("No diagnostics.");
                        }
                        let mut jump = None;
                        for d in &self.diags {
                            let t = format!(
                                "{} {}{}{}",
                                if d.error { "error" } else { "warning" },
                                d.file
                                    .as_ref()
                                    .map(|f| format!("[{f}] "))
                                    .unwrap_or_default(),
                                d.line.map(|l| format!("line {l}: ")).unwrap_or_default(),
                                d.message
                            );
                            if ui
                                .link(RichText::new(t).color(if d.error {
                                    Color32::from_rgb(220, 70, 70)
                                } else {
                                    Color32::from_rgb(210, 150, 30)
                                }))
                                .clicked()
                            {
                                jump = d.range.clone();
                            }
                        }
                        if let Some(r) = jump {
                            self.doc.set_selection(r.start, r.end.max(r.start));
                            self.scroll_to = Some(r.start);
                            self.after_external_edit();
                        }
                    });
                });
        }
        let blocks_rc = self.doc.blocks_rc();
        let blocks: &[Block] = &blocks_rc;
        if self.show_outline {
            Panel::left("outline").default_size(200.0).show(ui, |ui| {
                ui.strong("Outline");
                ScrollArea::vertical().show(ui, |ui| {
                    for b in blocks.iter().filter(|b| b.kind == BlockKind::Heading) {
                        let t = self.doc.text[b.range.clone()].to_string();
                        let lvl = t.chars().take_while(|c| *c == '=').count().max(1);
                        ui.horizontal(|ui| {
                            ui.add_space((lvl - 1) as f32 * 12.0);
                            if ui
                                .link(
                                    t.trim_start_matches('=')
                                        .trim()
                                        .lines()
                                        .next()
                                        .unwrap_or(""),
                                )
                                .clicked()
                            {
                                self.doc.set_selection(b.range.end, b.range.end);
                                self.scroll_to = Some(b.range.start);
                                self.after_external_edit();
                            }
                        });
                    }
                });
            });
        }
        self.settings_window(&ctx);

        CentralPanel::default().show(ui, |ui| self.document_surface(ui, blocks));
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Approximate pixel→source mapping for a click on a rendered block (C9): the
/// vertical position picks the source line, the horizontal one the column.
/// Exact mapping would need `typst-ide::jump_from_click`.
fn click_to_caret(text: &str, start: usize, rect: Rect, pos: Pos2) -> usize {
    let n = text.lines().count().max(1);
    let fy = ((pos.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 0.999);
    let li = ((fy * n as f32) as usize).min(n - 1);
    let mut off = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == li {
            let body = l.trim_end_matches(['\n', '\r']);
            let fx = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            let col = (fx * body.chars().count() as f32).round() as usize;
            let byte = body
                .char_indices()
                .nth(col)
                .map(|(i, _)| i)
                .unwrap_or(body.len());
            return start + off + byte;
        }
        off += l.len();
    }
    start + text.len()
}

impl App {
    fn block_hashes(&mut self, blocks: &[Block]) -> Arc<Vec<u64>> {
        if let Some((v, l, h)) = &self.hash_cache
            && *v == self.doc.version
            && *l == self.doc.text.len()
        {
            return h.clone();
        }
        let h = Arc::new(
            blocks
                .iter()
                .map(|b| hash_str(&self.doc.text[b.range.clone()]))
                .collect::<Vec<_>>(),
        );
        self.hash_cache = Some((self.doc.version, self.doc.text.len(), h.clone()));
        h
    }

    fn document_surface(&mut self, ui: &mut Ui, blocks: &[Block]) {
        let ctx = ui.ctx().clone();
        let multi = self.doc.multi;
        let len = self.doc.text.len();
        let caret = self.doc.head.min(len);
        let active = if multi {
            usize::MAX..usize::MAX
        } else {
            active_range(&self.doc.text, blocks, caret)
        };
        let sel = self.doc.sel();
        let avail = ui.available_width();
        let col_w = ((avail - 24.0).min(800.0) / 20.0).floor() * 20.0;
        let col_w = col_w.max(200.0);
        let margin = 8.0;
        let style = self.style(
            col_w - 2.0 * margin,
            self.render_ppp(ctx.pixels_per_point()),
        );
        let side = ((avail - col_w) / 2.0).max(0.0);
        let id = editor_id();
        let hashes = self.block_hashes(blocks);
        let mut defs = String::new();
        let mut defs_h = DefaultHasher::new();
        let mut used: HashSet<u64> = HashSet::new();
        let mut navigate: Option<usize> = None;
        let mut new_caret: Option<usize> = None;
        let mut extend: Option<(usize, usize)> = None;
        let mut hold_active = false;
        let editor_w = col_w - 2.0 * margin;

        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    ui.spacing_mut().item_spacing.y = self.settings.font_size * 0.8;
                    let virtual_active = !multi
                        && !blocks
                            .iter()
                            .any(|b| b.range.start <= active.end && active.start <= b.range.end);
                    let mut virtual_done = !virtual_active;
                    for (idx, b) in blocks.iter().enumerate() {
                        if !virtual_done && active.start < b.range.start {
                            virtual_done = true;
                            self.active_editor(
                                ui,
                                id,
                                active.clone(),
                                editor_w,
                                &mut navigate,
                                blocks,
                                0.0,
                                &mut hold_active,
                                &mut extend,
                            );
                        }
                        let is_active = !multi
                            && b.range.start <= active.end
                            && active.start <= b.range.end
                            && !(virtual_active && active.is_empty());
                        let scroll_hit = self
                            .scroll_to
                            .is_some_and(|s| s >= b.range.start && s <= b.range.end);
                        if is_active {
                            // D7: keep the height of the rendered image so the layout does not jump
                            let min_h = self
                                .last_good
                                .get(&idx)
                                .map(|g| g.1.y)
                                .unwrap_or(0.0)
                                .min(400.0);
                            let resp = self.active_editor(
                                ui,
                                id,
                                active.clone(),
                                editor_w,
                                &mut navigate,
                                blocks,
                                min_h,
                                &mut hold_active,
                                &mut extend,
                            );
                            if scroll_hit {
                                resp.scroll_to_me(Some(Align::Center));
                            }
                        } else {
                            let nb = {
                                let p = if idx > 0 { hashes[idx - 1] } else { 0 };
                                let n = hashes.get(idx + 1).copied().unwrap_or(0);
                                p.rotate_left(17) ^ n
                            };
                            let selected = multi
                                && sel.start < b.range.end.max(b.range.start + 1)
                                && b.range.start < sel.end.max(sel.start + 1)
                                && sel.start != sel.end;
                            let resp = self.rendered_block(
                                ui,
                                idx,
                                b,
                                hashes[idx],
                                defs_h.finish(),
                                &defs,
                                &style,
                                margin,
                                &mut used,
                                nb,
                                selected,
                            );
                            if resp.clicked() {
                                if ui.input(|i| i.modifiers.shift) {
                                    // D3: shift+click extends the selection across blocks
                                    let anchor = self.doc.anchor;
                                    extend = Some((
                                        anchor,
                                        if b.range.start >= anchor {
                                            b.range.end
                                        } else {
                                            b.range.start
                                        },
                                    ));
                                } else if let Some(pos) = resp.interact_pointer_pos() {
                                    let t = &self.doc.text[b.range.clone()];
                                    new_caret =
                                        Some(click_to_caret(t, b.range.start, resp.rect, pos));
                                } else {
                                    new_caret = Some(b.range.end);
                                }
                            }
                            if scroll_hit {
                                resp.scroll_to_me(Some(Align::Center));
                            }
                        }
                        if b.kind == BlockKind::Definition {
                            let t = &self.doc.text[b.range.clone()];
                            defs.push_str(t);
                            defs.push('\n');
                            t.hash(&mut defs_h);
                            '\n'.hash(&mut defs_h);
                        }
                    }
                    if !virtual_done {
                        self.active_editor(
                            ui,
                            id,
                            active.clone(),
                            editor_w,
                            &mut navigate,
                            blocks,
                            0.0,
                            &mut hold_active,
                            &mut extend,
                        );
                    }
                    let rest = ui.allocate_response(
                        vec2(col_w, ui.available_height().max(160.0)),
                        Sense::click(),
                    );
                    if rest.clicked() {
                        new_caret = Some(self.doc.text.len());
                    }
                });
            });
        });
        self.scroll_to = None; // D2: never leave a stale scroll target behind
        if self.cache.len() > 600 {
            self.cache.retain(|k, _| used.contains(k));
        }
        // P4: drop stale "last good" images of blocks that no longer exist
        self.last_good.retain(|i, _| *i < blocks.len());
        if let Ok(mut l) = self.live.lock() {
            *l = used;
        }
        // E6: shortcut commands run now, after the editor applied this frame's input
        if let Some(c) = self.pending_cmd.take() {
            self.run_command(c);
        } else if let Some((a, h)) = extend {
            self.set_selection_smart(a, h, blocks);
        } else if let Some(c) = navigate.or(new_caret) {
            self.doc.set_selection(c, c);
            self.after_external_edit();
        }
        self.last_active = active;
    }

    #[allow(clippy::too_many_arguments)]
    fn rendered_block(
        &mut self,
        ui: &mut Ui,
        idx: usize,
        b: &Block,
        bh: u64,
        dh: u64,
        defs: &str,
        style: &RenderStyle,
        margin: f32,
        used: &mut HashSet<u64>,
        nb: u64,
        selected: bool,
    ) -> Response {
        let is_def = b.kind == BlockKind::Definition;
        let mono_size = self.settings.font_size * 0.85;
        let inner = Frame::new().inner_margin(margin).corner_radius(4.0).show(ui, |ui| {
            let src = |s: &Self| s.doc.text[b.range.clone()].to_string();
            if is_def {
                return ui.add(Label::new(RichText::new(src(self)).monospace().size(mono_size).weak()).sense(Sense::click()));
            }
            let key = Self::key_for(bh, dh, style);
            used.insert(key);
            // P4: a job that never answered must not leave a placeholder forever
            if matches!(self.cache.get(&key), Some(Cached::Pending(t)) if t.elapsed() > PENDING_TIMEOUT) {
                self.cache.remove(&key);
            }
            if let std::collections::hash_map::Entry::Vacant(e) = self.cache.entry(key) {
                // P3: only blocks near the viewport are rendered
                let near = ui.clip_rect().expand2(vec2(0.0, 1200.0));
                let y = ui.cursor().min.y;
                if y >= near.top() && y <= near.bottom() {
                    e.insert(Cached::Pending(Instant::now()));
                    let _ = self.jobs.send(Job::Render { key, block: src(self), defs: defs.to_string(), style: style.clone(), root: self.root() });
                }
            }
            let ready = match self.cache.get(&key) {
                Some(Cached::Ready { tex, size }) => Some((tex.clone(), *size)),
                _ => None,
            };
            if let Some((tex, size)) = ready {
                self.last_good.insert(idx, (tex.clone(), size, nb));
                return ui.add(Image::new(load::SizedTexture::new(tex.id(), size)).sense(Sense::click()));
            }
            if let Some(Cached::Failed(d)) = self.cache.get(&key) {
                let msg = d.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("\n");
                return ui.add(Label::new(RichText::new(src(self)).monospace().size(self.settings.font_size * 0.9).color(Color32::from_rgb(200, 90, 90))).sense(Sense::click())).on_hover_text(msg);
            }
            // pending / not yet requested: previous image only if the neighbourhood is unchanged (C7)
            if let Some((tex, size, h)) = self.last_good.get(&idx)
                && *h == nb {
                    return ui.add(Image::new(load::SizedTexture::new(tex.id(), *size)).sense(Sense::click()));
                }
            ui.add(Label::new(RichText::new(src(self)).monospace().size(mono_size).weak()).sense(Sense::click()))
        });
        let r = inner.response.interact(Sense::click());
        if selected {
            ui.painter().rect_filled(
                r.rect,
                4.0,
                ui.visuals().selection.bg_fill.gamma_multiply(0.5),
            );
        }
        if r.hovered() {
            ui.painter().rect_stroke(
                r.rect,
                4.0,
                Stroke::new(1.0, ui.visuals().widgets.hovered.bg_stroke.color),
                StrokeKind::Inside,
            );
        }
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn active_editor(
        &mut self,
        ui: &mut Ui,
        id: Id,
        active: Range<usize>,
        width: f32,
        navigate: &mut Option<usize>,
        blocks: &[Block],
        min_h: f32,
        _hold: &mut bool,
        extend: &mut Option<(usize, usize)>,
    ) -> Response {
        let ctx = ui.ctx().clone();
        let mut buf = self.doc.text[active.clone()].to_string();
        let before = buf.clone();
        let local = |abs: usize, buf: &str| -> usize {
            buf[..abs.saturating_sub(active.start).min(buf.len())]
                .chars()
                .count()
        };
        let to_abs = |chars: usize, buf: &str| -> usize {
            active.start
                + buf
                    .char_indices()
                    .nth(chars)
                    .map(|(i, _)| i)
                    .unwrap_or(buf.len())
        };
        if self.sync_cursor || self.last_active != active {
            let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
            st.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(local(self.doc.anchor, &buf)),
                CCursor::new(local(self.doc.head, &buf)),
            )));
            st.store(&ctx, id);
            if self.want_focus {
                ctx.memory_mut(|m| m.request_focus(id));
            }
            self.sync_cursor = false;
            self.want_focus = false;
        }

        // U1/E6: caret of *this* frame (after the sync above), not the one stored
        // in the document at the end of the previous frame.
        let st = TextEditState::load(&ctx, id).unwrap_or_default();
        let nchars = buf.chars().count();
        let (anchor_l, head_l) = st
            .cursor
            .char_range()
            .map(|r| (usize::from(r.secondary.index), usize::from(r.primary.index)))
            .unwrap_or((local(self.doc.anchor, &buf), local(self.doc.head, &buf)));
        let (anchor_l, head_l) = (anchor_l.min(nchars), head_l.min(nchars));
        let collapsed = anchor_l == head_l;
        let at_start = head_l == 0;
        let at_end = head_l == nchars;
        // E8: visual row of the caret, laid out with the current width and text
        let (cur_row, n_rows) = {
            let job = self.syntax_job(&buf, width, ui, None);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            (
                galley.layout_from_cursor(CCursor::new(head_l)).row,
                galley.rows.len().max(1),
            )
        };
        let prev = blocks
            .iter()
            .rev()
            .find(|b| b.range.end <= active.start && b.range != active);
        let next = blocks
            .iter()
            .find(|b| b.range.start >= active.end && b.range != active);

        enum Act {
            Nav(usize),
            Extend(usize),
            DocTo(usize, bool),
            Merge(bool),
        }
        let mut act: Option<Act> = None;
        if ctx.memory(|m| m.has_focus(id)) {
            let doc_len = self.doc.text.len();
            ctx.input_mut(|i| {
                let m = i.modifiers;
                let plain = !m.any();
                let shift_only = m.shift && !m.ctrl && !m.alt && !m.command;
                let cmd_nav = (m.ctrl || m.command) && !m.alt;
                let take = |i: &mut InputState, k: Key| i.consume_key(m, k);
                if plain {
                    if i.key_pressed(Key::ArrowUp) && cur_row == 0 && prev.is_some() {
                        take(i, Key::ArrowUp);
                        act = prev.map(|p| Act::Nav(p.range.end));
                    } else if i.key_pressed(Key::ArrowDown)
                        && cur_row + 1 >= n_rows
                        && next.is_some()
                    {
                        take(i, Key::ArrowDown);
                        act = next.map(|n| Act::Nav(n.range.start));
                    } else if i.key_pressed(Key::ArrowLeft)
                        && at_start
                        && collapsed
                        && prev.is_some()
                    {
                        take(i, Key::ArrowLeft);
                        act = prev.map(|p| Act::Nav(p.range.end));
                    } else if i.key_pressed(Key::ArrowRight)
                        && at_end
                        && collapsed
                        && next.is_some()
                    {
                        take(i, Key::ArrowRight);
                        act = next.map(|n| Act::Nav(n.range.start));
                    } else if i.key_pressed(Key::Backspace)
                        && at_start
                        && collapsed
                        && active.start > 0
                    {
                        take(i, Key::Backspace);
                        act = Some(Act::Merge(false));
                    } else if i.key_pressed(Key::Delete)
                        && at_end
                        && collapsed
                        && active.end < doc_len
                    {
                        take(i, Key::Delete);
                        act = Some(Act::Merge(true));
                    }
                } else if shift_only {
                    // E10/D3: Shift+arrows leave the block and extend across blocks
                    if i.key_pressed(Key::ArrowUp) && cur_row == 0 && prev.is_some() {
                        take(i, Key::ArrowUp);
                        act = prev.map(|p| Act::Extend(p.range.start));
                    } else if i.key_pressed(Key::ArrowDown)
                        && cur_row + 1 >= n_rows
                        && next.is_some()
                    {
                        take(i, Key::ArrowDown);
                        act = next.map(|n| Act::Extend(n.range.end));
                    } else if i.key_pressed(Key::ArrowLeft) && at_start && prev.is_some() {
                        take(i, Key::ArrowLeft);
                        act = prev.map(|p| Act::Extend(p.range.end));
                    } else if i.key_pressed(Key::ArrowRight) && at_end && next.is_some() {
                        take(i, Key::ArrowRight);
                        act = next.map(|n| Act::Extend(n.range.start));
                    }
                }
                if act.is_none() && cmd_nav {
                    if i.key_pressed(Key::Home) {
                        take(i, Key::Home);
                        act = Some(Act::DocTo(0, m.shift));
                    } else if i.key_pressed(Key::End) {
                        take(i, Key::End);
                        act = Some(Act::DocTo(doc_len, m.shift));
                    }
                }
            });
        }
        let anchor_abs = to_abs(anchor_l, &buf);
        match act {
            Some(Act::Nav(c)) => *navigate = Some(c),
            Some(Act::Extend(h)) => *extend = Some((anchor_abs, h)),
            Some(Act::DocTo(c, true)) => *extend = Some((anchor_abs, c)),
            Some(Act::DocTo(c, false)) => *navigate = Some(c),
            Some(Act::Merge(fwd)) => {
                // U1/E9: remove the whole gap between the blocks and land at the end
                // of the previous one; one undo step restores text and caret.
                let t = &self.doc.text;
                let out = if fwd {
                    delete_at_block_end(t, blocks, active.end)
                } else {
                    backspace_at_block_start(t, blocks, active.start)
                };
                let out = out.unwrap_or_else(|| {
                    // caret in a whitespace gap (virtual block): delete one char
                    let (a, b) = if fwd {
                        let n = t[active.end..]
                            .chars()
                            .next()
                            .map(char::len_utf8)
                            .unwrap_or(0);
                        (active.end, active.end + n)
                    } else {
                        let n = t[..active.start]
                            .chars()
                            .next_back()
                            .map(char::len_utf8)
                            .unwrap_or(0);
                        (active.start - n, active.start)
                    };
                    let mut nt = t.clone();
                    nt.replace_range(a..b, "");
                    MergeOutcome {
                        text: nt,
                        caret: if fwd { active.end } else { a },
                    }
                });
                self.doc.apply_merge(out);
                self.after_external_edit();
            }
            None => {}
        }

        let this: &App = self;
        let wrap = width;
        // U2: delimiters of *…*, _…_ and $…$ are hidden while the caret is away from them
        let reveal = {
            let (a, h) = (local(self.doc.anchor, &buf), local(self.doc.head, &buf));
            let byte = |c: usize| {
                buf.char_indices()
                    .nth(c)
                    .map(|(i, _)| i)
                    .unwrap_or(buf.len())
            };
            Some((byte(a.min(h)), byte(a.max(h))))
        };
        let mut layouter = |ui: &Ui, text: &dyn TextBuffer, w: f32| -> Arc<Galley> {
            let job = this.syntax_job(text.as_str(), w.min(wrap), ui, reveal);
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        let frame = Frame::new()
            .inner_margin(8.0)
            .corner_radius(4.0)
            .fill(ui.visuals().faint_bg_color)
            .stroke(Stroke::new(1.0, ui.visuals().selection.bg_fill));
        let out = frame
            .show(ui, |ui| {
                ui.set_min_height(min_h);
                TextEdit::multiline(&mut buf)
                    .id(id)
                    .frame(Frame::NONE)
                    .desired_width(width)
                    .desired_rows(1)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner;

        if buf != before {
            self.doc.replace_range(active.clone(), &buf, false);
            self.last_change = Instant::now();
        }
        if let Some(r) = out.cursor_range
            && (out.response.response.has_focus() || buf != before)
        {
            let (a, h) = (
                to_abs(usize::from(r.secondary.index), &buf),
                to_abs(usize::from(r.primary.index), &buf),
            );
            if (a, h) != (self.doc.anchor, self.doc.head) || buf != before {
                self.doc.set_selection(a, h);
                self.doc.commit(false); // P1: only when something changed
            }
        }
        out.response.response
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1000.0, 760.0])
            .with_title("Typst Live"),
        ..Default::default()
    };
    eframe::run_native(
        "Typst Live",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_clamp() {
        let st = Settings {
            dark: true,
            font_size: 18.0,
            line_spacing: 1.6,
            monospace: false,
            autosave: true,
            autosave_secs: 9,
        };
        let last = PathBuf::from("/tmp/a b.typ");
        let (back, l) = Settings::parse(&st.serialize(Some(&last)));
        assert!(back.dark && !back.monospace && back.autosave);
        assert_eq!(
            (back.font_size, back.line_spacing, back.autosave_secs),
            (18.0, 1.6, 9)
        );
        assert_eq!(l, Some(last));
        let (bad, _) = Settings::parse("font_size=999\nautosave_secs=0\nnonsense\ndark=maybe\n");
        assert_eq!(bad.font_size, 28.0);
        assert_eq!(bad.autosave_secs, 1);
        assert!(!bad.dark);
    }

    #[test]
    fn click_maps_to_a_source_line() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 40.0));
        let t = "ab\ncd";
        assert!(click_to_caret(t, 10, r, pos2(1.0, 5.0)) < 13);
        assert!(click_to_caret(t, 10, r, pos2(99.0, 39.0)) >= 13);
    }
}
