//! PDF rasterization for the Files panel. One worker thread per open document
//! owns the parsed file (hayro's caches are single-threaded) and renders the
//! pages the view currently wants, most wanted first. Pure helpers here size
//! the requests; `pdf_preview.rs` owns the view.
//!
//! The file is untrusted and rendered in-process. [`super::pdf_scan`] refuses
//! documents that declare outsized streams or images before hayro reads them,
//! and the view abandons a worker that overruns [`PARSE_DEADLINE`] or
//! [`RENDER_DEADLINE`]: a thread can't be killed, so it is left to finish on
//! its own, holding nothing the UI waits on, and its file isn't opened again
//! this session.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::peniko::ImageAlphaType;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

use super::pdf_scan;

/// Longest rendered side, in pixels. hayro sizes pixmaps in `u16`, and a
/// sharper raster than this costs memory without a visible gain in a panel.
pub(super) const MAX_PAGE_SIDE: u32 = 4096;
/// Pixels in one rendered page: 32 MiB of RGBA, 64 MiB counted the way the
/// view retains it, so one page always fits its budget.
pub(super) const MAX_PAGE_PIXELS: u64 = 8 * 1024 * 1024;
/// Requested widths move in steps of this many pixels.
const WIDTH_STEP: u32 = 64;
/// The narrowest raster the budget shrinks a page to.
pub(super) const MIN_BUDGETED_WIDTH: u32 = 256;
/// How long the scan and parse of one document may take.
pub(super) const PARSE_DEADLINE: Duration = Duration::from_secs(15);
/// How long one page may take to render.
pub(super) const RENDER_DEADLINE: Duration = Duration::from_secs(20);
/// The worker's stack. hayro caps object and form nesting, so this leaves
/// room for legitimately deep documents; a stack overflow still aborts the
/// process rather than unwinding, so it is a margin, not a guard.
const WORKER_STACK: usize = 32 * 1024 * 1024;

/// A page's size in PDF points, after its rotation and crop box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// One page rasterized at one pixel width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct RenderRequest {
    pub page: usize,
    pub width_px: u32,
}

pub(super) enum WorkerEvent {
    Opened(Result<Vec<PageSize>, String>),
    Rendered {
        request: RenderRequest,
        width: u32,
        height: u32,
        /// BGRA, ready for GPUI.
        pixels: Vec<u8>,
    },
    Failed {
        request: RenderRequest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Busy {
    Parsing,
    Rendering,
}

struct Queue {
    wanted: Vec<RenderRequest>,
    /// Requests the worker has taken whose result the view hasn't handled
    /// yet; asking for them again would render them twice.
    in_flight: Vec<RenderRequest>,
    /// What the worker is doing and since when, for the view's deadlines.
    busy: Option<(Busy, Instant)>,
    closed: bool,
}

/// The worker for one document. Dropping it stops the thread after the page
/// it may be rendering.
pub(super) struct PdfWorker {
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl PdfWorker {
    pub fn spawn(
        bytes: Vec<u8>,
        events: mpsc::UnboundedSender<WorkerEvent>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new((
            Mutex::new(Queue {
                wanted: Vec::new(),
                in_flight: Vec::new(),
                busy: Some((Busy::Parsing, Instant::now())),
                closed: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("files-pdf".into())
            .stack_size(WORKER_STACK)
            .spawn(move || run(bytes, &worker, &events))?;
        Ok(Self { shared })
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replace the pending requests; the first is rendered first.
    pub fn want(&self, mut requests: Vec<RenderRequest>) {
        let mut queue = self.queue();
        requests.retain(|request| !queue.in_flight.contains(request));
        if queue.wanted != requests {
            queue.wanted = requests;
            self.shared.1.notify_one();
        }
    }

    /// The view handled the result for `request`.
    pub fn finished(&self, request: RenderRequest) {
        self.queue().in_flight.retain(|pending| *pending != request);
    }

    /// Whether the parse or the page in progress has run past its deadline.
    pub fn overdue(&self) -> bool {
        match self.queue().busy {
            Some((Busy::Parsing, since)) => since.elapsed() > PARSE_DEADLINE,
            Some((Busy::Rendering, since)) => since.elapsed() > RENDER_DEADLINE,
            None => false,
        }
    }
}

impl Drop for PdfWorker {
    fn drop(&mut self) {
        self.queue().closed = true;
        self.shared.1.notify_one();
    }
}

/// Content hashes of documents whose worker overran a deadline. Their
/// threads may still be running; each file gets at most one.
static ABANDONED: Mutex<Vec<[u8; 32]>> = Mutex::new(Vec::new());

pub(super) fn abandon(hash: [u8; 32]) {
    let mut abandoned = ABANDONED.lock().unwrap_or_else(PoisonError::into_inner);
    if !abandoned.contains(&hash) {
        abandoned.push(hash);
    }
}

pub(super) fn is_abandoned(hash: &[u8; 32]) -> bool {
    ABANDONED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(hash)
}

/// Scan, parse and measure the document.
fn open(bytes: Vec<u8>) -> Result<(Pdf, Vec<PageSize>), String> {
    let scan = pdf_scan::before_parse(&bytes)?;
    let pdf = Pdf::new(bytes).map_err(|error| load_error_message(&error))?;
    scan.after_parse(&pdf)?;
    let pages = pdf.pages();
    if pages.is_empty() {
        return Err("This PDF has no pages.".into());
    }
    let sizes = pages
        .iter()
        .map(|page| {
            let (width, height) = page.render_dimensions();
            PageSize { width, height }
        })
        .collect();
    Ok((pdf, sizes))
}

fn run(
    bytes: Vec<u8>,
    shared: &(Mutex<Queue>, Condvar),
    events: &mpsc::UnboundedSender<WorkerEvent>,
) {
    let (queue, wake) = shared;
    let lock = || queue.lock().unwrap_or_else(PoisonError::into_inner);
    let opened = catch_unwind(AssertUnwindSafe(|| open(bytes)))
        .unwrap_or_else(|_| Err("This PDF couldn't be read.".into()));
    lock().busy = None;
    let pdf = match opened {
        Ok((pdf, sizes)) => {
            if events
                .unbounded_send(WorkerEvent::Opened(Ok(sizes)))
                .is_err()
            {
                return;
            }
            pdf
        }
        Err(error) => {
            let _ = events.unbounded_send(WorkerEvent::Opened(Err(error)));
            return;
        }
    };
    let pages = pdf.pages();
    let interpreter = InterpreterSettings::default();
    let settings = RenderSettings::default();
    let mut cache = RenderCache::new();
    loop {
        let request = {
            let mut queue = lock();
            while queue.wanted.is_empty() && !queue.closed {
                queue = wake.wait(queue).unwrap_or_else(PoisonError::into_inner);
            }
            if queue.closed {
                return;
            }
            let request = queue.wanted.remove(0);
            if request.page >= pages.len() {
                continue;
            }
            queue.in_flight.push(request);
            queue.busy = Some((Busy::Rendering, Instant::now()));
            request
        };
        let page = &pages[request.page];
        let (width, height) = page.render_dimensions();
        let rendered = raster_size(PageSize { width, height }, request.width_px).map(
            |(target_width, target_height)| {
                catch_unwind(AssertUnwindSafe(|| {
                    let pixmap = render(
                        page,
                        &cache,
                        &interpreter,
                        &settings,
                        &PixmapSettings {
                            x_scale: target_width as f32 / width,
                            y_scale: target_height as f32 / height,
                            bg_color: WHITE,
                        },
                    );
                    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
                    // Takes the pixmap's buffer rather than copying it.
                    (
                        width,
                        height,
                        pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied),
                    )
                }))
            },
        );
        lock().busy = None;
        let event = match rendered {
            Some(Ok((width, height, mut pixels))) if width > 0 && height > 0 => {
                // Opaque white background: premultiplied RGBA is plain RGBA.
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                WorkerEvent::Rendered {
                    request,
                    width,
                    height,
                    pixels,
                }
            }
            Some(Err(_)) => {
                // A panic may leave the shared caches half-written.
                cache = RenderCache::new();
                WorkerEvent::Failed { request }
            }
            _ => WorkerEvent::Failed { request },
        };
        if events.unbounded_send(event).is_err() {
            return;
        }
    }
}

fn load_error_message(error: &LoadPdfError) -> String {
    match error {
        LoadPdfError::Decryption(DecryptionError::PasswordProtected) => {
            "This PDF is password-protected and can't be previewed.".into()
        }
        LoadPdfError::Decryption(_) => {
            "This PDF's encryption isn't supported by the preview.".into()
        }
        LoadPdfError::Invalid => "This file isn't a readable PDF.".into(),
    }
}

/// The pixel size to rasterize `page` at for a request `width_px` wide: its
/// aspect ratio kept, at least one pixel per side, no side over
/// [`MAX_PAGE_SIDE`] and no more than [`MAX_PAGE_PIXELS`] in total. `None`
/// for a degenerate page.
pub(super) fn raster_size(page: PageSize, width_px: u32) -> Option<(u32, u32)> {
    if !(page.width.is_finite() && page.height.is_finite())
        || page.width <= 0.0
        || page.height <= 0.0
    {
        return None;
    }
    let aspect = f64::from(page.height) / f64::from(page.width);
    let mut width = f64::from(width_px.max(1)).min(f64::from(MAX_PAGE_SIDE));
    let mut height = width * aspect;
    if height > f64::from(MAX_PAGE_SIDE) {
        height = f64::from(MAX_PAGE_SIDE);
        width = height / aspect;
    }
    let pixels = width * height;
    if pixels > MAX_PAGE_PIXELS as f64 {
        let shrink = (MAX_PAGE_PIXELS as f64 / pixels).sqrt();
        width *= shrink;
        height *= shrink;
    }
    Some((
        (width.floor() as u32).clamp(1, MAX_PAGE_SIDE),
        (height.floor() as u32).clamp(1, MAX_PAGE_SIDE),
    ))
}

/// The pixel width to request for a page drawn `display_width` logical pixels
/// wide on a `scale_factor` display. Rounded up to a 64 px step so a small
/// resize reuses the pages already rendered.
pub(super) fn requested_width_px(display_width: f32, scale_factor: f32) -> u32 {
    let device = (display_width.max(1.0) * scale_factor.max(1.0)).ceil() as u32;
    device
        .div_ceil(WIDTH_STEP)
        .saturating_mul(WIDTH_STEP)
        .clamp(WIDTH_STEP, MAX_PAGE_SIDE)
}

/// Bytes the view retains for a `width × height` raster: the CPU frame GPUI
/// keeps for atlas re-uploads as well as the GPU copy.
pub(super) fn retained_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4 * 2
}

/// The pixel widths to request for the pages drawn this frame, each given as
/// its size and its drawn width in logical pixels: their device widths,
/// shrunk together in 64 px steps (to no less than [`MIN_BUDGETED_WIDTH`])
/// until their rasters fit `budget` bytes.
pub(super) fn budgeted_widths(
    pages: &[(PageSize, f32)],
    scale_factor: f32,
    budget: usize,
) -> Vec<u32> {
    let cost = |widths: &[u32]| -> usize {
        pages
            .iter()
            .zip(widths)
            .filter_map(|((size, _), &width)| raster_size(*size, width))
            .map(|(width, height)| retained_bytes(width, height))
            .sum()
    };
    let mut widths: Vec<u32> = pages
        .iter()
        .map(|(_, display)| requested_width_px(*display, scale_factor))
        .collect();
    // A raster held back by the pixel cap shrinks less than its width, so one
    // pass may not be enough.
    for _ in 0..8 {
        let total = cost(&widths);
        if total <= budget {
            break;
        }
        let shrink = (budget as f64 / total as f64).sqrt();
        let mut changed = false;
        for width in &mut widths {
            let shrunk = ((f64::from(*width) * shrink) as u32 / WIDTH_STEP * WIDTH_STEP)
                .max(MIN_BUDGETED_WIDTH)
                .min(*width);
            changed |= shrunk != *width;
            *width = shrunk;
        }
        if !changed {
            break;
        }
    }
    widths
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use futures::StreamExt;

    /// A one-page PDF drawing a black square on a 200×100 pt page, with a
    /// correct cross-reference table.
    pub(crate) fn square_pdf() -> Vec<u8> {
        pages_pdf(1)
    }

    /// `count` identical 200×100 pt pages, each with the black square.
    pub(crate) fn pages_pdf(count: usize) -> Vec<u8> {
        let content = b"0 0 0 rg 20 20 60 60 re f";
        let contents = 3 + count;
        let kids = (0..count)
            .map(|page| format!("{} 0 R", 3 + page))
            .collect::<Vec<_>>()
            .join(" ");
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>"),
        ];
        objects.extend((0..count).map(|_| {
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents {contents} 0 R >>"
            )
        }));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        ));
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn raster_sizes_keep_the_aspect_and_stay_bounded() {
        let letter = PageSize {
            width: 612.0,
            height: 792.0,
        };
        assert_eq!(raster_size(letter, 1224), Some((1224, 1584)));
        // A tall page is capped by its height, a huge request by both sides.
        let (w, h) = raster_size(
            PageSize {
                width: 100.0,
                height: 10_000.0,
            },
            2000,
        )
        .unwrap();
        assert_eq!(h, MAX_PAGE_SIDE);
        assert!((1..100).contains(&w));
        let (w, h) = raster_size(letter, u32::MAX).unwrap();
        assert!(w <= MAX_PAGE_SIDE && h <= MAX_PAGE_SIDE);
        assert!(u64::from(w) * u64::from(h) <= MAX_PAGE_PIXELS);
        // A sliver still gets a pixel; a degenerate page none.
        assert_eq!(
            raster_size(
                PageSize {
                    width: 10_000.0,
                    height: 0.5,
                },
                100,
            ),
            Some((100, 1))
        );
        for degenerate in [(0.0, 10.0), (10.0, -1.0), (f32::NAN, 10.0)] {
            assert_eq!(
                raster_size(
                    PageSize {
                        width: degenerate.0,
                        height: degenerate.1,
                    },
                    100,
                ),
                None
            );
        }
    }

    #[test]
    fn requested_widths_follow_device_pixels_in_steps() {
        assert_eq!(requested_width_px(500.0, 2.0), 1024);
        assert_eq!(requested_width_px(510.0, 2.0), 1024);
        assert_eq!(requested_width_px(513.0, 2.0), 1088);
        assert_eq!(requested_width_px(0.0, 1.0), 64);
        assert_eq!(requested_width_px(100_000.0, 2.0), MAX_PAGE_SIDE);
    }

    #[test]
    fn drawn_pages_are_budgeted_together() {
        let letter = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let budget = 96 * 1024 * 1024;
        // One Letter page 1152 pt wide on a 2x display fits as drawn.
        assert_eq!(
            budgeted_widths(&[(letter, 1152.0)], 2.0, budget),
            vec![2304]
        );
        // Two of them don't, so both shrink to the same narrower width.
        let two = budgeted_widths(&[(letter, 1152.0), (letter, 1152.0)], 2.0, budget);
        assert_eq!(two[0], two[1]);
        assert!(two[0] < 2304 && two[0] % 64 == 0, "{two:?}");
        let total: usize = two
            .iter()
            .map(|&width| {
                let (width, height) = raster_size(letter, width).unwrap();
                retained_bytes(width, height)
            })
            .sum();
        assert!(total <= budget, "{total}");
        // However many are drawn, none goes below the minimum.
        let many = budgeted_widths(&[(letter, 1152.0); 400], 2.0, budget);
        assert!(many.iter().all(|&width| width == MIN_BUDGETED_WIDTH));
        // A single raster can't outgrow the budget on its own.
        let (width, height) = raster_size(letter, MAX_PAGE_SIDE).unwrap();
        assert!(retained_bytes(width, height) <= budget);
    }

    fn next(events: &mut mpsc::UnboundedReceiver<WorkerEvent>) -> WorkerEvent {
        futures::executor::block_on(events.next()).expect("worker event")
    }

    #[test]
    fn the_worker_opens_a_document_and_renders_requested_pages() {
        let (sender, mut events) = mpsc::unbounded();
        let worker = PdfWorker::spawn(square_pdf(), sender).unwrap();
        let WorkerEvent::Opened(Ok(sizes)) = next(&mut events) else {
            panic!("expected an opened document");
        };
        assert_eq!(
            sizes,
            vec![PageSize {
                width: 200.0,
                height: 100.0,
            }]
        );
        let request = RenderRequest {
            page: 0,
            width_px: 400,
        };
        worker.want(vec![request]);
        let WorkerEvent::Rendered {
            request: rendered,
            width,
            height,
            pixels,
        } = next(&mut events)
        else {
            panic!("expected a rendered page");
        };
        assert_eq!((rendered, width, height), (request, 400, 200));
        assert_eq!(pixels.len(), 400 * 200 * 4);
        // BGRA: white margin, black square in the middle.
        let at = |x: usize, y: usize| &pixels[(y * 400 + x) * 4..][..4];
        assert_eq!(at(4, 4), [255, 255, 255, 255]);
        assert_eq!(at(100, 100), [0, 0, 0, 255]);
        // Until the view has handled a result, asking again doesn't queue a
        // second render.
        worker.want(vec![request]);
        assert!(worker.queue().wanted.is_empty());
        worker.finished(request);
        assert!(!worker.overdue());
        // Out-of-range pages are skipped, not fatal.
        worker.want(vec![
            RenderRequest {
                page: 7,
                width_px: 400,
            },
            request,
        ]);
        assert!(matches!(next(&mut events), WorkerEvent::Rendered { .. }));
        drop(worker);
    }

    #[test]
    fn encrypted_documents_say_whether_a_password_is_needed() {
        assert!(
            load_error_message(&LoadPdfError::Decryption(
                DecryptionError::PasswordProtected
            ))
            .contains("password-protected")
        );
        assert!(
            load_error_message(&LoadPdfError::Decryption(
                DecryptionError::UnsupportedAlgorithm
            ))
            .contains("encryption isn't supported")
        );
    }

    #[test]
    fn unreadable_documents_report_why() {
        for (bytes, expected) in [
            (b"not a pdf".to_vec(), "isn't a readable PDF"),
            (Vec::new(), "isn't a readable PDF"),
        ] {
            let (sender, mut events) = mpsc::unbounded();
            let _worker = PdfWorker::spawn(bytes, sender).unwrap();
            let WorkerEvent::Opened(Err(message)) = next(&mut events) else {
                panic!("expected an open failure");
            };
            assert!(message.contains(expected), "{message}");
        }
    }
}
