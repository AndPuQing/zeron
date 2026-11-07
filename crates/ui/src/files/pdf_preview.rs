//! A read-only workspace PDF. The client fetches its bytes from the owning
//! device; [`super::pdf_render`] rasterizes the pages on the list's visible
//! range, at the width they are drawn at.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui::{
    AnyElement, Bounds, Context, FocusHandle, ListAlignment, ListState, Pixels, Render,
    RenderImage, ScrollHandle, Task, Window, div, img, list, prelude::*, px,
};

use super::client::{FilesRequestContext, WorkspaceFilesClient};
use super::pdf_render::{PageSize, PdfWorker, RenderRequest, WorkerEvent, requested_width_px};
use crate::icons::{self, icon};
use crate::theme::Theme;

/// Rendered pages kept per document, counting the CPU frame GPUI retains
/// for atlas re-uploads as well as the GPU copy. Visible pages are never
/// evicted to meet it.
const MAX_RETAINED_BYTES: usize = 96 * 1024 * 1024;
const PAGE_GAP: f32 = 12.0;
const PAGE_PADDING: f32 = 16.0;
/// Room under the last page for the floating controls.
const CONTROLS_CLEARANCE: f32 = 52.0;
/// Zoom relative to fitting the widest page to the panel.
const ZOOM_STEPS: [f32; 9] = [0.5, 0.67, 0.8, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
const FIT_ZOOM: usize = 3;
const OLD_HOST_ERROR: &str = "Unsupported workspace image format";

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

/// The display scale, in logical pixels per PDF point, that draws the widest
/// page `zoom` times the width available in a `viewport_width` panel.
fn display_scale(pages: &[PageSize], viewport_width: f32, zoom: f32) -> f32 {
    let widest = pages
        .iter()
        .map(|page| page.width)
        .fold(0.0_f32, f32::max)
        .max(1.0);
    let available = (viewport_width - 2.0 * PAGE_PADDING).max(48.0);
    available * zoom / widest
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
    error: Option<String>,
    suspended: bool,
    list: ListState,
    /// The horizontal pan of pages zoomed wider than the panel.
    pan: ScrollHandle,
    zoom: usize,
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    /// The display scale the list last measured its rows at.
    measured_scale: f32,
    /// Requests from the rows drawn this frame, top first.
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
            error: None,
            suspended: false,
            list,
            pan: ScrollHandle::new(),
            zoom: FIT_ZOOM,
            bounds: Bounds::default(),
            scale_factor: 1.0,
            measured_scale: 0.0,
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

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.task = None;
        self.suspended = true;
        self.pages.clear();
        self.wanted.clear();
        self.list.reset(0);
        self.measured_scale = 0.0;
        self.release(cx);
        cx.notify();
    }

    pub fn activate(&mut self, cx: &mut Context<Self>) {
        if self.suspended {
            self.reload(cx);
        }
    }

    pub fn deleted(&mut self, cx: &mut Context<Self>) {
        self.suspend(cx);
        self.suspended = false;
        self.error = Some("This PDF was removed from the workspace.".into());
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
                futures::future::Either::Left((result, _)) => result,
                futures::future::Either::Right(_) => Err("PDF preview timed out".to_string()),
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
            let (sender, mut events) = futures::channel::mpsc::unbounded();
            let started = this.update(cx, |view, _| {
                if view.accepts(generation) {
                    view.worker = Some(PdfWorker::spawn(bytes, sender));
                    true
                } else {
                    false
                }
            });
            if !matches!(started, Ok(true)) {
                return;
            }
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |view, cx| view.handle(generation, event, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    fn accepts(&self, generation: u64) -> bool {
        !self.suspended && generation == self.generation
    }

    fn handle(&mut self, generation: u64, event: WorkerEvent, cx: &mut Context<Self>) {
        if !self.accepts(generation) {
            return;
        }
        match event {
            WorkerEvent::Opened(Ok(pages)) => {
                self.list.reset(pages.len());
                self.pages = pages;
                self.measured_scale = 0.0;
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
                let Some(frame) = image::RgbaImage::from_raw(width, height, pixels) else {
                    return;
                };
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(frame)]));
                let bytes = width as usize * height as usize * 4 * 2;
                if let Some(old) = self.rendered.insert(
                    request.page,
                    RenderedPage {
                        width_px: request.width_px,
                        image,
                        bytes,
                    },
                ) {
                    cx.defer(move |cx| cx.drop_image(old.image, None));
                }
                self.evict(cx);
            }
            WorkerEvent::Failed { request } => {
                tracing::debug!(page = request.page, "PDF page failed to render");
            }
        }
        cx.notify();
    }

    /// Drop rendered pages outside the drawn range, farthest first, until the
    /// retained pixels fit [`MAX_RETAINED_BYTES`].
    fn evict(&mut self, cx: &mut Context<Self>) {
        let visible: HashSet<usize> = self.wanted.iter().map(|request| request.page).collect();
        let anchor = self.wanted.first().map_or(0, |request| request.page);
        let mut total: usize = self.rendered.values().map(|page| page.bytes).sum();
        let mut candidates: Vec<usize> = self
            .rendered
            .keys()
            .copied()
            .filter(|page| !visible.contains(page))
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

    /// One row of the page list: the page at the current scale, its raster
    /// once one exists, and a request for the raster this width wants.
    fn render_page(&mut self, page: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let Some(size) = self.pages.get(page).copied() else {
            return div().into_any_element();
        };
        let scale = self.scale();
        let width = (size.width * scale).max(1.0);
        let height = (size.height * scale).max(1.0);
        let request = RenderRequest {
            page,
            width_px: requested_width_px(width, self.scale_factor),
        };
        if self
            .rendered
            .get(&page)
            .is_none_or(|rendered| rendered.width_px != request.width_px)
            && !self.wanted.contains(&request)
        {
            self.wanted.push(request);
            #[cfg(test)]
            self.requested.push(request);
            if let Some(worker) = &self.worker {
                worker.want(self.wanted.clone());
            }
        }
        // A stale-resolution raster stands in until the sharper one lands.
        let raster = self
            .rendered
            .get(&page)
            .map(|rendered| rendered.image.clone());
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
                    .when_some(raster, |el, raster| el.child(img(raster).size_full())),
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
            // The rows drawn this frame name the rasters still wanted.
            self.wanted.clear();
            let widest = self
                .pages
                .iter()
                .map(|page| page.width)
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
            let drawn: HashSet<_> = view.wanted.iter().map(|request| request.page).collect();
            assert!(!drawn.contains(&0), "the top page is no longer wanted");
        });

        // Zooming widens every page, so their rasters are re-requested wider.
        let before = view.read_with(cx, |view, _| view.wanted.first().copied().unwrap());
        let zoom_in = cx.debug_bounds("pdf-zoom-in").unwrap().center();
        cx.simulate_click(zoom_in, Modifiers::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.zoom, FIT_ZOOM + 1);
            let after = view.wanted.first().copied().unwrap();
            assert!(after.width_px > before.width_px, "{before:?} -> {after:?}");
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
    }
}
