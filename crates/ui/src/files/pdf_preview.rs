//! A read-only workspace PDF. The client fetches its bytes from the owning
//! device; [`super::pdf_render`] rasterizes the pages on the list's visible
//! range, at the width they are drawn at.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::future::Either;
use gpui::{
    AnyElement, Bounds, Context, FocusHandle, ListAlignment, ListOffset, ListState, Pixels, Render,
    RenderImage, ScrollHandle, Task, Window, div, img, list, prelude::*, px,
};
use sha2::{Digest, Sha256};

use super::client::{FilesRequestContext, WorkspaceFilesClient};
use super::pdf_render::{
    PageSize, PdfWorker, RenderRequest, WorkerEvent, abandon, budgeted_widths, is_abandoned,
    retained_bytes,
};
use crate::icons::{self, icon};
use crate::theme::Theme;

/// Rendered pages kept per document, counting the CPU frame GPUI retains
/// for atlas re-uploads as well as the GPU copy. The pages drawn are sized
/// to fit it together and are never evicted; the rest are, farthest first.
const MAX_RETAINED_BYTES: usize = 96 * 1024 * 1024;
const PAGE_GAP: f32 = 12.0;
const PAGE_PADDING: f32 = 16.0;
/// Room under the last page for the floating controls.
const CONTROLS_CLEARANCE: f32 = 52.0;
/// Zoom relative to fitting the widest page to the panel.
const ZOOM_STEPS: [f32; 9] = [0.5, 0.67, 0.8, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
const FIT_ZOOM: usize = 3;
/// The largest page side, in points, the layout trusts: 200 inches, PDF's
/// own limit. Larger, empty or non-finite sizes come from broken files.
const MAX_PAGE_POINTS: f32 = 14_400.0;
/// How often the view checks the worker against its deadlines.
const WATCHDOG_TICK: Duration = Duration::from_millis(500);
const OLD_HOST_ERROR: &str = "Unsupported workspace image format";
const TOO_SLOW: &str = "This PDF took too long to render.";
const UNREADABLE: &str = "This PDF couldn't be read.";
const PAGE_FAILED: &str = "This page couldn't be rendered";

pub(super) fn is_pdf(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pdf"))
}

/// What a failed load says. A host from before PDF previews refuses the
/// format outright.
fn load_error_message(error: String) -> String {
    if error.contains(OLD_HOST_ERROR) {
        "Update Zerun on the device that holds this file to preview PDFs.".into()
    } else {
        error
    }
}

fn plausible(points: f32) -> bool {
    points.is_finite() && points > 0.0 && points <= MAX_PAGE_POINTS
}

/// The display scale, in logical pixels per PDF point, that draws the widest
/// page `zoom` times the width available in a `viewport_width` panel. A page
/// with an implausible width doesn't count, so one broken page can't shrink
/// the rest.
fn display_scale(pages: &[PageSize], viewport_width: f32, zoom: f32) -> f32 {
    let widest = pages
        .iter()
        .map(|page| page.width)
        .filter(|&width| plausible(width))
        .fold(0.0_f32, f32::max);
    let widest = if widest > 0.0 { widest } else { 612.0 };
    let available = (viewport_width - 2.0 * PAGE_PADDING).max(48.0);
    available * zoom / widest
}

/// The size a page is laid out at, in points: an implausible side is clamped
/// (or, when empty or non-finite, replaced by Letter's) so it can't break
/// the list's layout.
fn layout_size(page: PageSize) -> PageSize {
    let side = |points: f32, fallback: f32| {
        if points.is_finite() && points > 0.0 {
            points.clamp(1.0, MAX_PAGE_POINTS)
        } else {
            fallback
        }
    };
    PageSize {
        width: side(page.width, 612.0),
        height: side(page.height, 792.0),
    }
}

struct RenderedPage {
    width_px: u32,
    image: Arc<RenderImage>,
    bytes: usize,
}

pub(super) struct PdfPreview {
    pub focus: FocusHandle,
    path: String,
    context: FilesRequestContext,
    client: WorkspaceFilesClient,
    generation: u64,
    task: Option<Task<()>>,
    worker: Option<PdfWorker>,
    pages: Vec<PageSize>,
    rendered: HashMap<usize, RenderedPage>,
    /// Rasters that failed to render, never requested again.
    failed: HashSet<RenderRequest>,
    error: Option<String>,
    suspended: bool,
    list: ListState,
    /// Where the list was when the view was suspended, to return to.
    resume: Option<ListOffset>,
    /// The horizontal pan of pages zoomed wider than the panel.
    pan: ScrollHandle,
    zoom: usize,
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    /// The display scale the list last measured its rows at.
    measured_scale: f32,
    /// The rows drawn this frame, top first, whatever their raster's state.
    drawn: Vec<usize>,
    /// The raster width each drawn page wants, within the memory budget.
    targets: HashMap<usize, u32>,
    /// The drawn pages' missing rasters, top first.
    wanted: Vec<RenderRequest>,
    #[cfg(test)]
    requested: Vec<RenderRequest>,
}

impl PdfPreview {
    pub fn new(
        path: String,
        context: FilesRequestContext,
        client: WorkspaceFilesClient,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|view, cx| view.release(cx)).detach();
        let list = ListState::new(0, ListAlignment::Top, px(1200.0));
        let entity = cx.weak_entity();
        // The page indicator follows the scroll position.
        list.set_scroll_handler(move |_, _, cx| {
            let _ = entity.update(cx, |_, cx| cx.notify());
        });
        let mut view = Self {
            focus: cx.focus_handle(),
            path,
            context,
            client,
            generation: 0,
            task: None,
            worker: None,
            pages: Vec::new(),
            rendered: HashMap::new(),
            failed: HashSet::new(),
            error: None,
            suspended: false,
            list,
            resume: None,
            pan: ScrollHandle::new(),
            zoom: FIT_ZOOM,
            bounds: Bounds::default(),
            scale_factor: 1.0,
            measured_scale: 0.0,
            drawn: Vec::new(),
            targets: HashMap::new(),
            wanted: Vec::new(),
            #[cfg(test)]
            requested: Vec::new(),
        };
        view.reload(cx);
        view
    }

    fn release(&mut self, cx: &mut gpui::App) {
        self.worker = None;
        let images: Vec<_> = self.rendered.drain().map(|(_, page)| page.image).collect();
        if !images.is_empty() {
            cx.defer(move |cx| {
                for image in images {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    /// Drop the document and its rasters, keeping the reading position.
    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        if !self.pages.is_empty() {
            self.resume = Some(self.list.logical_scroll_top());
        }
        self.generation = self.generation.wrapping_add(1);
        self.task = None;
        self.suspended = true;
        self.clear(cx);
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.pages.clear();
        self.failed.clear();
        self.drawn.clear();
        self.targets.clear();
        self.wanted.clear();
        self.list.reset(0);
        self.measured_scale = 0.0;
        self.release(cx);
    }

    pub fn activate(&mut self, cx: &mut Context<Self>) {
        if self.suspended {
            self.reload(cx);
        }
    }

    pub fn deleted(&mut self, cx: &mut Context<Self>) {
        self.suspend(cx);
        self.suspended = false;
        self.resume = None;
        self.error = Some("This PDF was removed from the workspace.".into());
    }

    /// Show `message` in place of the document and let its worker go.
    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.clear(cx);
        self.error = Some(message);
        cx.notify();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.suspend(cx);
        self.suspended = false;
        self.error = None;
        let generation = self.generation;
        let path = self.path.clone();
        let context = self.context.clone();
        let client = self.client.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let load = async {
                // As for images: outside files read by absolute path with no
                // checkout; legacy targets learn their checkout from the host.
                let outside = super::path_is_outside(&path);
                let checkout = if outside {
                    String::new()
                } else {
                    match context.checkout_id.clone().filter(|id| !id.is_empty()) {
                        Some(id) => id,
                        None => {
                            client
                                .read_file(zeron_proto::ReadWorkspaceFileRequest {
                                    target: context.target.clone(),
                                    path: path.clone(),
                                })
                                .await
                                .map_err(|e| e.to_string())?
                                .checkout_id
                        }
                    }
                };
                if checkout.is_empty() && !outside {
                    return Err("Workspace checkout identity unavailable".to_string());
                }
                let (mime, bytes) = client
                    .read_image(path, checkout)
                    .await
                    .map_err(|e| e.to_string())?;
                if mime != "application/pdf" {
                    return Err("This file isn't a readable PDF.".to_string());
                }
                Ok(bytes)
            };
            let result = match futures::future::select(
                Box::pin(load),
                Box::pin(executor.timer(Duration::from_secs(30))),
            )
            .await
            {
                Either::Left((result, _)) => result,
                Either::Right(_) => Err("PDF preview timed out".to_string()),
            };
            let bytes = match result {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        if view.accepts(generation) {
                            view.task = None;
                            view.error = Some(load_error_message(error));
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            let (bytes, hash) = executor
                .spawn(async move {
                    let hash: [u8; 32] = Sha256::digest(&bytes).into();
                    (bytes, hash)
                })
                .await;
            let (sender, mut events) = futures::channel::mpsc::unbounded();
            let started = this.update(cx, |view, cx| {
                if !view.accepts(generation) {
                    return false;
                }
                // A file whose renderer already overran a deadline keeps at
                // most that one thread.
                if is_abandoned(&hash) {
                    view.fail(TOO_SLOW.into(), cx);
                    return false;
                }
                match PdfWorker::spawn(bytes, sender) {
                    Ok(worker) => {
                        view.worker = Some(worker);
                        true
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not start the PDF renderer");
                        view.fail("The PDF renderer couldn't start.".into(), cx);
                        false
                    }
                }
            });
            if !matches!(started, Ok(true)) {
                return;
            }
            let mut tick = Box::pin(executor.timer(WATCHDOG_TICK));
            loop {
                match futures::future::select(events.next(), tick).await {
                    Either::Left((Some(event), pending)) => {
                        tick = pending;
                        if this
                            .update(cx, |view, cx| view.handle(generation, event, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Either::Left((None, _)) => {
                        let _ = this.update(cx, |view, cx| view.events_ended(generation, cx));
                        return;
                    }
                    Either::Right(((), _)) => {
                        let overdue =
                            this.update(cx, |view, cx| view.check_deadline(generation, hash, cx));
                        if !matches!(overdue, Ok(false)) {
                            // Dropping the receiver drops whatever the
                            // abandoned worker still sends.
                            return;
                        }
                        tick = Box::pin(executor.timer(WATCHDOG_TICK));
                    }
                }
            }
        }));
    }

    fn accepts(&self, generation: u64) -> bool {
        !self.suspended && generation == self.generation
    }

    /// The worker exited; nothing more will arrive from it.
    fn events_ended(&mut self, generation: u64, cx: &mut Context<Self>) {
        if !self.accepts(generation) {
            return;
        }
        self.worker = None;
        if self.pages.is_empty() && self.error.is_none() {
            self.error = Some(UNREADABLE.into());
            cx.notify();
        }
    }

    /// Abandon a worker that overran its deadline: its thread can't be
    /// stopped, so the view stops waiting on it and its file isn't opened
    /// again this session. Returns whether the view stopped watching.
    fn check_deadline(&mut self, generation: u64, hash: [u8; 32], cx: &mut Context<Self>) -> bool {
        if !self.accepts(generation) {
            return true;
        }
        if !self.worker.as_ref().is_some_and(PdfWorker::overdue) {
            return false;
        }
        abandon(hash);
        self.fail(TOO_SLOW.into(), cx);
        true
    }

    fn handle(&mut self, generation: u64, event: WorkerEvent, cx: &mut Context<Self>) {
        if !self.accepts(generation) {
            return;
        }
        match event {
            WorkerEvent::Opened(Ok(pages)) => {
                self.list.reset(pages.len());
                if let Some(resume) = self.resume.take()
                    && resume.item_ix < pages.len()
                {
                    self.list.scroll_to(resume);
                }
                self.pages = pages;
                // `reset` left every row unmeasured, so the first frame
                // needn't remeasure (which would rescale a restored offset).
                self.measured_scale = self.scale();
            }
            WorkerEvent::Opened(Err(error)) => {
                self.worker = None;
                self.error = Some(error);
            }
            WorkerEvent::Rendered {
                request,
                width,
                height,
                pixels,
            } => {
                if let Some(worker) = &self.worker {
                    worker.finished(request);
                }
                // A raster for a width the page no longer wants, after a zoom
                // or a scroll, is dropped rather than shown.
                if self.targets.get(&request.page) == Some(&request.width_px)
                    && let Some(frame) = image::RgbaImage::from_raw(width, height, pixels)
                {
                    let image = Arc::new(RenderImage::new(vec![image::Frame::new(frame)]));
                    if let Some(old) = self.rendered.insert(
                        request.page,
                        RenderedPage {
                            width_px: request.width_px,
                            image,
                            bytes: retained_bytes(width, height),
                        },
                    ) {
                        cx.defer(move |cx| cx.drop_image(old.image, None));
                    }
                    self.evict(cx);
                }
            }
            WorkerEvent::Failed { request } => {
                if let Some(worker) = &self.worker {
                    worker.finished(request);
                }
                tracing::debug!(page = request.page, "PDF page failed to render");
                self.failed.insert(request);
            }
        }
        cx.notify();
    }

    /// Drop rendered pages that weren't drawn, farthest first, until the
    /// retained pixels fit [`MAX_RETAINED_BYTES`]. The drawn pages' widths
    /// are budgeted to fit it on their own.
    fn evict(&mut self, cx: &mut Context<Self>) {
        let drawn: HashSet<usize> = self.drawn.iter().copied().collect();
        let anchor = self
            .drawn
            .first()
            .copied()
            .unwrap_or_else(|| self.list.logical_scroll_top().item_ix);
        let mut total: usize = self.rendered.values().map(|page| page.bytes).sum();
        let mut candidates: Vec<usize> = self
            .rendered
            .keys()
            .copied()
            .filter(|page| !drawn.contains(page))
            .collect();
        candidates.sort_by_key(|page| std::cmp::Reverse(page.abs_diff(anchor)));
        let mut dropped = Vec::new();
        for page in candidates {
            if total <= MAX_RETAINED_BYTES {
                break;
            }
            if let Some(evicted) = self.rendered.remove(&page) {
                total -= evicted.bytes;
                dropped.push(evicted.image);
            }
        }
        if !dropped.is_empty() {
            cx.defer(move |cx| {
                for image in dropped {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    /// Once the list has laid out: size the drawn pages' rasters to fit the
    /// memory budget together, and hand the ones missing to the worker. Runs
    /// every frame, so requests for rows no longer drawn are withdrawn.
    fn sync_requests(&mut self) {
        let scale = self.scale();
        let drawn: Vec<(usize, PageSize, f32)> = self
            .drawn
            .iter()
            .filter_map(|&page| {
                let size = *self.pages.get(page)?;
                Some((page, size, (layout_size(size).width * scale).max(1.0)))
            })
            .collect();
        let sizes: Vec<(PageSize, f32)> = drawn
            .iter()
            .map(|&(_, size, width)| (size, width))
            .collect();
        let widths = budgeted_widths(&sizes, self.scale_factor, MAX_RETAINED_BYTES);
        self.targets = drawn.iter().map(|&(page, ..)| page).zip(widths).collect();
        self.wanted = drawn
            .iter()
            .filter_map(|&(page, ..)| {
                let request = RenderRequest {
                    page,
                    width_px: *self.targets.get(&page)?,
                };
                let current = self
                    .rendered
                    .get(&page)
                    .is_some_and(|rendered| rendered.width_px == request.width_px);
                (!current && !self.failed.contains(&request)).then_some(request)
            })
            .collect();
        #[cfg(test)]
        for request in &self.wanted {
            if !self.requested.contains(request) {
                self.requested.push(*request);
            }
        }
        if let Some(worker) = &self.worker {
            worker.want(self.wanted.clone());
        }
    }

    fn set_zoom(&mut self, zoom: usize, cx: &mut Context<Self>) {
        let zoom = zoom.min(ZOOM_STEPS.len() - 1);
        if zoom != self.zoom {
            self.zoom = zoom;
            cx.notify();
        }
    }

    fn scale(&self) -> f32 {
        display_scale(
            &self.pages,
            f32::from(self.bounds.size.width),
            ZOOM_STEPS[self.zoom],
        )
    }

    /// One row of the page list: the page at the current scale, with its
    /// raster once one exists. Drawing it records it for the frame's
    /// requests, made once the whole list has laid out.
    fn render_page(&mut self, page: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let Some(size) = self.pages.get(page).copied().map(layout_size) else {
            return div().into_any_element();
        };
        if !self.drawn.contains(&page) {
            self.drawn.push(page);
        }
        let scale = self.scale();
        let width = (size.width * scale).max(1.0);
        let height = (size.height * scale).max(1.0);
        // A stale-resolution raster stands in until the sharper one lands.
        let raster = self
            .rendered
            .get(&page)
            .map(|rendered| rendered.image.clone());
        let failed = raster.is_none()
            && self
                .targets
                .get(&page)
                .is_some_and(|&width_px| self.failed.contains(&RenderRequest { page, width_px }));
        div()
            .w_full()
            .pt(px(if page == 0 { PAGE_PADDING } else { 0.0 }))
            .pb(px(if page + 1 == self.pages.len() {
                CONTROLS_CLEARANCE
            } else {
                PAGE_GAP
            }))
            .flex()
            .justify_center()
            .child(
                div()
                    .id(("pdf-page", page))
                    .w(px(width))
                    .h(px(height))
                    .flex_none()
                    .bg(gpui::white())
                    .border_1()
                    .border_color(theme.border)
                    .when_some(raster, |el, raster| el.child(img(raster).size_full()))
                    .when(failed, |el| {
                        el.flex()
                            .items_center()
                            .justify_center()
                            .px(px(8.0))
                            .text_size(px(11.0))
                            .font_family(theme.font_sans.clone())
                            // The page stays white in either theme.
                            .text_color(gpui::rgb(0x6e6e6e))
                            .child(PAGE_FAILED)
                    }),
            )
            .into_any_element()
    }

    fn render_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let current = self
            .list
            .logical_scroll_top()
            .item_ix
            .min(self.pages.len().saturating_sub(1))
            + 1;
        let label = |text: String| {
            div()
                .px(px(6.0))
                .text_size(px(11.0))
                .text_color(theme.text_muted)
                .font_family(theme.font_sans.clone())
                .child(text)
        };
        div()
            .id("pdf-controls")
            .absolute()
            .bottom(px(12.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .p(px(3.0))
                    .rounded(px(8.0))
                    .bg(theme.surface_raised)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_sm()
                    .child(label(format!("{current} / {}", self.pages.len())))
                    .child(
                        super::toolbar_button("pdf-zoom-out", "Zoom out")
                            .when(self.zoom == 0, |el| el.opacity(0.4))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_zoom(this.zoom.saturating_sub(1), cx)
                            }))
                            .child(
                                icon(icons::MINUS)
                                    .size(px(crate::surface_chrome::ICON_SIZE))
                                    .text_color(theme.text_muted),
                            ),
                    )
                    .child(
                        div()
                            .id("pdf-zoom-fit")
                            .cursor_pointer()
                            .rounded(px(crate::surface_chrome::CONTROL_RADIUS))
                            .hover(|style| style.bg(crate::theme::wash(0.14)))
                            .role(gpui::Role::Button)
                            .aria_label("Fit width")
                            .on_click(cx.listener(|this, _, _, cx| this.set_zoom(FIT_ZOOM, cx)))
                            .child(label(format!(
                                "{}%",
                                (ZOOM_STEPS[self.zoom] * 100.0).round() as u32
                            ))),
                    )
                    .child(
                        super::toolbar_button("pdf-zoom-in", "Zoom in")
                            .debug_selector(|| "pdf-zoom-in".into())
                            .when(self.zoom + 1 == ZOOM_STEPS.len(), |el| el.opacity(0.4))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_zoom(this.zoom + 1, cx)),
                            )
                            .child(
                                icon(icons::PLUS)
                                    .size(px(crate::surface_chrome::ICON_SIZE))
                                    .text_color(theme.text_muted),
                            ),
                    ),
            )
    }
}

impl Render for PdfPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        self.scale_factor = window.scale_factor();
        // The list's rows record themselves as they draw; the canvas below,
        // laid out after the list, turns them into requests.
        self.drawn.clear();
        let mut root = div()
            .id("file-pdf-preview")
            .debug_selector(|| "file-pdf-preview".into())
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(theme.surface);
        // Rows are sized from the panel width, which the canvas below learns
        // after the first frame; measuring them before would request every
        // page at once.
        if self.pages.is_empty() || self.bounds.size.width <= px(0.0) {
            root = root.flex().items_center().justify_center().child(
                div()
                    .px(px(16.0))
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(self.error.clone().unwrap_or_else(|| "Loading PDF…".into())),
            );
        } else {
            let scale = self.scale();
            if (scale - self.measured_scale).abs() > f32::EPSILON {
                self.measured_scale = scale;
                self.list.remeasure();
            }
            let widest = self
                .pages
                .iter()
                .map(|&page| layout_size(page).width)
                .fold(0.0_f32, f32::max);
            let content_width =
                (widest * scale + 2.0 * PAGE_PADDING).max(f32::from(self.bounds.size.width));
            let entity = cx.weak_entity();
            let mut pages = div()
                .id("pdf-pages-scroll")
                .size_full()
                .overflow_x_scroll()
                .track_scroll(&self.pan)
                .child(
                    div().w(px(content_width)).h_full().child(
                        list(self.list.clone(), move |page, _, cx| {
                            entity
                                .update(cx, |view, cx| view.render_page(page, cx))
                                .unwrap_or_else(|_| div().into_any_element())
                        })
                        .size_full(),
                    ),
                );
            // Vertical wheel input belongs to the page list, not the pan.
            pages.style().restrict_scroll_to_axis = Some(true);
            root = root.child(pages).child(self.render_controls(cx));
        }
        let entity = cx.weak_entity();
        root.child(
            gpui::canvas(
                move |bounds, _, cx| {
                    let _ = entity.update(cx, |view, cx| {
                        if view.bounds != bounds {
                            view.bounds = bounds;
                            cx.notify();
                        }
                        view.sync_requests();
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::pdf_render::tests::pages_pdf;

    #[test]
    fn recognizes_pdf_paths() {
        for path in ["a.pdf", "docs/A.PDF", "x.Pdf"] {
            assert!(is_pdf(path), "{path}");
        }
        for path in ["a.pdf.txt", "pdf", "a.png", "a.pd"] {
            assert!(!is_pdf(path), "{path}");
        }
    }

    #[test]
    fn old_hosts_are_told_to_update() {
        assert!(
            load_error_message("ReadWorkspaceImage: Unsupported workspace image format".into())
                .contains("Update Zerun")
        );
        assert_eq!(
            load_error_message("PDF exceeds 8 MiB preview limit".into()),
            "PDF exceeds 8 MiB preview limit"
        );
    }

    #[test]
    fn pages_fit_the_panel_by_the_widest_page() {
        let pages = [
            PageSize {
                width: 612.0,
                height: 792.0,
            },
            PageSize {
                width: 792.0,
                height: 612.0,
            },
        ];
        let fit = display_scale(&pages, 824.0, 1.0);
        assert!((792.0 * fit - (824.0 - 2.0 * PAGE_PADDING)).abs() < 0.01);
        assert!((display_scale(&pages, 824.0, 2.0) - 2.0 * fit).abs() < 1e-4);
        // A panel narrower than the padding still draws something.
        assert!(display_scale(&pages, 10.0, 1.0) > 0.0);
        // A broken page neither shrinks the others nor breaks the layout.
        let broken = [
            pages[0],
            PageSize {
                width: f32::INFINITY,
                height: 10.0,
            },
            PageSize {
                width: 1e30,
                height: f32::NAN,
            },
        ];
        assert_eq!(
            display_scale(&broken, 824.0, 1.0),
            display_scale(&pages[..1], 824.0, 1.0)
        );
        assert_eq!(
            layout_size(broken[2]),
            PageSize {
                width: MAX_PAGE_POINTS,
                height: 792.0,
            }
        );
    }

    #[derive(Default)]
    struct PdfTransport {
        calls: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
        mime: &'static str,
        pages: usize,
    }

    #[async_trait::async_trait]
    impl super::super::client::WorkspaceFilesTransport for PdfTransport {
        async fn call(
            &self,
            method: &str,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, zeron_rpc::RpcError> {
            use base64::Engine as _;
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), params.clone()));
            assert_eq!(params["targetDeviceId"], "owner");
            assert_eq!(method, zeron_rpc::methods::READ_WORKSPACE_IMAGE);
            assert_eq!(params["expectedCheckoutId"], "checkout");
            let bytes = pages_pdf(self.pages.max(1));
            Ok(serde_json::json!({
                "checkoutId": "checkout",
                "contentHash": "version",
                "mimeType": self.mime,
                "data": base64::engine::general_purpose::STANDARD.encode(&bytes),
                "nextOffset": bytes.len(),
                "size": bytes.len(),
                "done": true,
            }))
        }

        async fn subscribe(
            &self,
            _: &str,
            _: serde_json::Value,
        ) -> Result<tokio::sync::mpsc::Receiver<serde_json::Value>, zeron_rpc::RpcError> {
            unreachable!()
        }
    }

    fn context() -> FilesRequestContext {
        FilesRequestContext {
            target: zeron_proto::WorkspaceTarget {
                chat_id: Some("chat".into()),
                space_id: None,
                checkout_path: None,
            },
            target_device_id: Some("owner".into()),
            cwd: "/not/on/ui/device".into(),
            checkout_id: Some("checkout".into()),
        }
    }

    /// Waits for the background worker, which runs on its own thread.
    fn settle(
        cx: &mut gpui::TestAppContext,
        done: impl Fn(&PdfPreview) -> bool,
        view: &gpui::Entity<PdfPreview>,
    ) {
        for _ in 0..200 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| done(view)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[gpui::test]
    fn a_pdf_loads_through_the_owner_and_reports_its_pages(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        // The renderer runs on its own thread.
        cx.background_executor.allow_parking();
        let transport = Arc::new(PdfTransport {
            mime: "application/pdf",
            ..Default::default()
        });
        let view = cx.new(|cx| {
            let client = WorkspaceFilesClient::with_transport(transport.clone(), context());
            PdfPreview::new("docs/square.pdf".into(), context(), client, cx)
        });
        settle(
            cx,
            |view| !view.pages.is_empty() || view.error.is_some(),
            &view,
        );
        view.read_with(cx, |view, _| {
            assert!(view.error.is_none(), "{:?}", view.error);
            assert_eq!(
                view.pages,
                vec![PageSize {
                    width: 200.0,
                    height: 100.0,
                }]
            );
            assert_eq!(view.list.item_count(), 1);
        });
        assert_eq!(transport.calls.lock().unwrap().len(), 1);
        // A requested page arrives as a raster; suspension drops it and the
        // worker, and late events from the old generation are ignored.
        view.update(cx, |view, _| {
            view.targets = HashMap::from([(0, 256)]);
            view.wanted = vec![RenderRequest {
                page: 0,
                width_px: 256,
            }];
            view.worker.as_ref().unwrap().want(view.wanted.clone());
        });
        settle(cx, |view| view.rendered.contains_key(&0), &view);
        view.update(cx, |view, cx| {
            assert_eq!(view.rendered[&0].width_px, 256);
            let generation = view.generation;
            view.suspend(cx);
            assert!(view.worker.is_none() && view.rendered.is_empty() && view.pages.is_empty());
            view.handle(
                generation,
                WorkerEvent::Opened(Ok(vec![PageSize {
                    width: 1.0,
                    height: 1.0,
                }])),
                cx,
            );
            assert!(
                view.pages.is_empty(),
                "late events from a suspended load are dropped"
            );
        });
    }

    #[gpui::test]
    fn a_non_pdf_reply_is_an_error(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let transport = Arc::new(PdfTransport {
            mime: "image/png",
            ..Default::default()
        });
        let view = cx.new(|cx| {
            let client = WorkspaceFilesClient::with_transport(transport, context());
            PdfPreview::new("docs/square.pdf".into(), context(), client, cx)
        });
        settle(cx, |view| view.error.is_some(), &view);
        view.read_with(cx, |view, _| {
            assert!(view.error.as_ref().unwrap().contains("readable PDF"));
            assert!(view.worker.is_none());
        });
    }

    #[gpui::test]
    fn deletion_is_visible_and_survives_activation(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let transport = Arc::new(PdfTransport {
            mime: "application/pdf",
            ..Default::default()
        });
        let view = cx.new(|cx| {
            let client = WorkspaceFilesClient::with_transport(transport, context());
            PdfPreview::new("docs/square.pdf".into(), context(), client, cx)
        });
        view.update(cx, |view, cx| {
            view.deleted(cx);
            view.activate(cx);
            assert!(view.task.is_none());
            assert!(view.error.as_ref().unwrap().contains("removed"));
        });
    }

    #[gpui::test]
    fn scrolling_rasterizes_only_the_pages_drawn(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;
        cx.background_executor.allow_parking();
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(crate::theme::Theme::default());
        });
        let transport = Arc::new(PdfTransport {
            mime: "application/pdf",
            pages: 40,
            ..Default::default()
        });
        let (view, cx) = cx.add_window_view(|_, cx| {
            let client = WorkspaceFilesClient::with_transport(transport, context());
            PdfPreview::new("docs/long.pdf".into(), context(), client, cx)
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..3 {
                cx.update(|window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                });
            }
        };
        for _ in 0..200 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.pages.len() == 40) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        draw(cx);
        let requested = |cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.requested
                    .iter()
                    .map(|request| request.page)
                    .collect::<HashSet<_>>()
            })
        };
        let first = requested(cx);
        assert!(first.contains(&0), "{first:?}");
        assert!(
            !first.contains(&39),
            "far pages wait for the scroll: {first:?}"
        );
        // The first page's raster lands through the real render loop.
        for _ in 0..200 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.rendered.contains_key(&0)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        draw(cx);
        assert!(view.read_with(cx, |view, _| view.rendered.contains_key(&0)));

        let panel = cx.debug_bounds("file-pdf-preview").unwrap();
        for _ in 0..40 {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: panel.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-2000.0))),
                ..Default::default()
            });
            draw(cx);
        }
        assert!(
            requested(cx).contains(&39),
            "the last page is requested once drawn"
        );
        view.read_with(cx, |view, _| {
            assert!(view.list.logical_scroll_top().item_ix > 30);
            assert!(!view.drawn.contains(&0), "the top page is no longer drawn");
            assert!(view.wanted.iter().all(|request| request.page != 0));
        });

        // Zooming widens every page; the drawn pages' rasters still fit the
        // budget together.
        let scale = view.read_with(cx, |view, _| view.scale());
        let zoom_in = cx.debug_bounds("pdf-zoom-in").unwrap().center();
        cx.simulate_click(zoom_in, Modifiers::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.zoom, FIT_ZOOM + 1);
            assert!((view.scale() / scale - 1.25).abs() < 1e-3);
            assert!(!view.drawn.is_empty());
            let retained: usize = view
                .drawn
                .iter()
                .map(|page| {
                    let (width, height) = crate::files::pdf_render::raster_size(
                        view.pages[*page],
                        view.targets[page],
                    )
                    .unwrap();
                    retained_bytes(width, height)
                })
                .sum();
            assert!(retained <= MAX_RETAINED_BYTES, "{retained}");
        });
        // Zoomed past the panel, the vertical wheel still pages and only a
        // horizontal one pans.
        let wheel = |cx: &mut gpui::VisualTestContext, x: f32, y: f32| {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: panel.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(x), px(y))),
                ..Default::default()
            });
            draw(cx);
        };
        wheel(cx, 0.0, -400.0);
        assert_eq!(view.read_with(cx, |view, _| view.pan.offset().x), px(0.0));
        wheel(cx, -400.0, 0.0);
        assert!(view.read_with(cx, |view, _| view.pan.offset().x) < px(0.0));

        // Switching away and back returns to the same page.
        let top = view.read_with(cx, |view, _| view.list.logical_scroll_top().item_ix);
        view.update(cx, |view, cx| {
            view.suspend(cx);
            view.activate(cx);
        });
        for _ in 0..200 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.pages.len() == 40) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        draw(cx);
        let restored = view.read_with(cx, |view, _| view.list.logical_scroll_top().item_ix);
        assert!(top > 30 && restored + 1 >= top, "{top} -> {restored}");
    }

    /// A view with `pages` loaded and its worker running.
    fn loaded(cx: &mut gpui::TestAppContext, pages: usize) -> gpui::Entity<PdfPreview> {
        use gpui::AppContext as _;
        cx.background_executor.allow_parking();
        let transport = Arc::new(PdfTransport {
            mime: "application/pdf",
            pages,
            ..Default::default()
        });
        let view = cx.new(|cx| {
            let client = WorkspaceFilesClient::with_transport(transport, context());
            PdfPreview::new("docs/pages.pdf".into(), context(), client, cx)
        });
        settle(cx, |view| view.pages.len() == pages, &view);
        view.update(cx, |view, _| {
            view.bounds = Bounds::new(
                gpui::point(px(0.0), px(0.0)),
                gpui::size(px(800.0), px(600.0)),
            );
            view.scale_factor = 2.0;
        });
        view
    }

    fn placeholder_image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(1, 1),
        )]))
    }

    #[gpui::test]
    fn drawn_pages_over_the_budget_are_not_evicted(cx: &mut gpui::TestAppContext) {
        let view = loaded(cx, 3);
        view.update(cx, |view, cx| {
            // Two drawn pages over the budget between them, and one that
            // isn't drawn.
            for page in 0..3 {
                view.rendered.insert(
                    page,
                    RenderedPage {
                        width_px: 64,
                        image: placeholder_image(),
                        bytes: 60 * 1024 * 1024,
                    },
                );
            }
            view.drawn = vec![1, 2];
            view.evict(cx);
            let mut kept: Vec<_> = view.rendered.keys().copied().collect();
            kept.sort_unstable();
            assert_eq!(kept, vec![1, 2]);
            view.evict(cx);
            assert_eq!(view.rendered.len(), 2, "they don't evict each other later");
        });
    }

    #[gpui::test]
    fn results_follow_the_current_targets(cx: &mut gpui::TestAppContext) {
        let view = loaded(cx, 2);
        view.update(cx, |view, cx| {
            let generation = view.generation;
            view.drawn = vec![0];
            view.sync_requests();
            let request = view.wanted[0];
            assert_eq!(view.targets[&0], request.width_px);

            // A raster for another width, from before a zoom, is dropped.
            let rendered = |width_px: u32| WorkerEvent::Rendered {
                request: RenderRequest { page: 0, width_px },
                width: width_px,
                height: width_px / 2,
                pixels: vec![255; width_px as usize * (width_px / 2) as usize * 4],
            };
            view.handle(generation, rendered(request.width_px + 64), cx);
            assert!(!view.rendered.contains_key(&0));
            view.handle(generation, rendered(request.width_px), cx);
            assert_eq!(view.rendered[&0].width_px, request.width_px);
            view.sync_requests();
            assert!(view.wanted.is_empty());

            // A page that fails isn't asked for again.
            view.drawn = vec![1];
            view.sync_requests();
            let failed = view.wanted[0];
            view.handle(generation, WorkerEvent::Failed { request: failed }, cx);
            view.sync_requests();
            assert!(view.wanted.is_empty(), "{:?}", view.wanted);
            assert!(view.failed.contains(&failed));
        });
    }

    #[gpui::test]
    fn a_worker_that_stops_early_is_an_error(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let transport = Arc::new(PdfTransport {
            mime: "application/pdf",
            ..Default::default()
        });
        let view = cx.new(|cx| {
            let client = WorkspaceFilesClient::with_transport(transport, context());
            PdfPreview::new("docs/square.pdf".into(), context(), client, cx)
        });
        view.update(cx, |view, cx| {
            assert!(view.pages.is_empty() && view.error.is_none());
            // Stands in for the load task, which would start a real worker.
            view.task = None;
            let generation = view.generation;
            view.events_ended(generation, cx);
            assert_eq!(view.error.as_deref(), Some(UNREADABLE));
        });
    }
}
