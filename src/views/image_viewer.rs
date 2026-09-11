//! Read-only raster image tabs for files the text editor cannot open.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, IntoElement, ObjectFit, Render, Task, Window,
    div, img, prelude::*, px,
};

use crate::services::project_files::{self, FileError, ImageKind, ImageSnapshot};
use crate::theme;
use crate::theme::terminal_manager as chrome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageViewerEvent {
    Changed,
}

struct Source {
    project: PathBuf,
    path: PathBuf,
}

struct LoadedImage {
    format: ImageKind,
    width: Option<u32>,
    height: Option<u32>,
    byte_len: usize,
    image: Arc<gpui::Image>,
}

pub(crate) struct ImageViewer {
    source: Source,
    focus: FocusHandle,
    loaded: Option<LoadedImage>,
    fit: bool,
    loading: bool,
    error: Option<FileError>,
    generation: u64,
    _load_task: Option<Task<()>>,
}

impl ImageViewer {
    pub(crate) fn open(
        project: PathBuf,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut viewer = Self {
            source: Source { project, path },
            focus: cx.focus_handle(),
            loaded: None,
            fit: true,
            loading: false,
            error: None,
            generation: 0,
            _load_task: None,
        };
        viewer.load(window, cx);
        viewer
    }

    pub(crate) fn title(&self) -> String {
        self.source
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Image".to_owned())
    }

    pub(crate) fn focus(&self, window: &mut Window, _: &mut Context<Self>) {
        window.focus(&self.focus);
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.loading
    }

    pub(crate) fn retarget(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.source.path = path;
        cx.notify();
    }

    pub(crate) fn reload_clean(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(window, cx);
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let project = self.source.project.clone();
        let path = self.source.path.clone();
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        let work = cx
            .background_executor()
            .spawn(async move { project_files::load_image_file(&project, &path) });
        let handle = window.window_handle();
        self._load_task = Some(cx.spawn(async move |viewer, cx| {
            let result = work.await;
            let _ = handle.update(cx, |_, _, cx| {
                let _ = viewer.update(cx, |viewer, cx| {
                    if viewer.generation != generation {
                        return;
                    }
                    viewer.loading = false;
                    match result {
                        Ok(snapshot) => {
                            viewer.loaded = Some(LoadedImage::from_snapshot(snapshot));
                            viewer.error = None;
                        }
                        Err(error) => viewer.error = Some(error),
                    }
                    cx.emit(ImageViewerEvent::Changed);
                    cx.notify();
                });
            });
        }));
        cx.emit(ImageViewerEvent::Changed);
        cx.notify();
    }

    fn status(&self) -> String {
        if self.loading && self.loaded.is_none() {
            return "Opening image…".to_owned();
        }
        if self.loading {
            return "Reloading image…".to_owned();
        }
        let Some(loaded) = &self.loaded else {
            return "Image unavailable".to_owned();
        };
        let size = format_bytes(loaded.byte_len);
        match (loaded.width, loaded.height) {
            (Some(width), Some(height)) => {
                format!("{} · {width} × {height} · {size}", loaded.format.label())
            }
            _ => format!("{} · {size}", loaded.format.label()),
        }
    }
}

impl LoadedImage {
    fn from_snapshot(snapshot: ImageSnapshot) -> Self {
        let byte_len = snapshot.bytes.len();
        Self {
            format: snapshot.format,
            width: snapshot.width,
            height: snapshot.height,
            byte_len,
            image: Arc::new(gpui::Image::from_bytes(
                gpui_format(snapshot.format),
                snapshot.bytes,
            )),
        }
    }
}

fn gpui_format(kind: ImageKind) -> gpui::ImageFormat {
    match kind {
        ImageKind::Png => gpui::ImageFormat::Png,
        ImageKind::Jpeg => gpui::ImageFormat::Jpeg,
        ImageKind::Gif => gpui::ImageFormat::Gif,
        ImageKind::Webp => gpui::ImageFormat::Webp,
        ImageKind::Bmp => gpui::ImageFormat::Bmp,
        ImageKind::Tiff => gpui::ImageFormat::Tiff,
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

impl EventEmitter<ImageViewerEvent> for ImageViewer {}

impl Render for ImageViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.loaded.is_some();
        let fit = self.fit;
        div()
            .id("image-viewer")
            .key_context("ImageViewer")
            .track_focus(&self.focus)
            .tab_index(0)
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .when(ready, |view| {
                view.child(
                    div()
                        .id("image-stage")
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|viewer, _, _, cx| {
                            viewer.fit = !viewer.fit;
                            cx.notify();
                        }))
                        .child(self.render_image()),
                )
            })
            .when(!ready, |view| {
                view.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .items_center()
                        .p(px(chrome::CONTENT_INSET))
                        .font_family(chrome::CHROME_FONT)
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(chrome::CONTROL_TEXT_SIZE))
                        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                        .text_color(theme::ash())
                        .child(if self.loading {
                            "Opening image…"
                        } else {
                            "This image could not be opened."
                        }),
                )
            })
            .when_some(self.error.as_ref(), |view, error| {
                view.child(
                    div()
                        .flex_shrink_0()
                        .px(px(chrome::CONTENT_INSET))
                        .py(px(chrome::INSET))
                        .flex()
                        .items_center()
                        .gap(px(chrome::GAP))
                        .bg(theme::error_wash())
                        .font_family(chrome::CHROME_FONT)
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(chrome::CONTROL_TEXT_SIZE))
                        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                        .text_color(theme::error())
                        .child(div().flex_1().min_w_0().child(error.to_string()))
                        .child(
                            chrome_button("retry-image")
                                .on_click(
                                    cx.listener(|viewer, _, window, cx| viewer.load(window, cx)),
                                )
                                .child("Retry"),
                        ),
                )
            })
            .child(
                div()
                    .h(px(chrome::CONTROL_HEIGHT + chrome::SMALL_GAP * 2.0))
                    .flex_shrink_0()
                    .px(px(chrome::CONTENT_INSET))
                    .flex()
                    .items_center()
                    .gap(px(chrome::GAP))
                    .border_t_1()
                    .border_color(theme::edge_soft())
                    .bg(theme::floor())
                    .font_family(theme::mono())
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                    .text_color(theme::ash())
                    .child(div().flex_1().min_w_0().truncate().child(self.status()))
                    .when(ready, |bar| {
                        bar.child(
                            mode_button("fit-image", fit)
                                .on_click(cx.listener(|viewer, _, _, cx| {
                                    viewer.fit = true;
                                    cx.notify();
                                }))
                                .child("Fit"),
                        )
                        .child(
                            mode_button("actual-image", !fit)
                                .on_click(cx.listener(|viewer, _, _, cx| {
                                    viewer.fit = false;
                                    cx.notify();
                                }))
                                .child("100%"),
                        )
                    })
                    .child(
                        chrome_button("reload-image")
                            .on_click(cx.listener(|viewer, _, window, cx| viewer.load(window, cx)))
                            .child("Reload"),
                    ),
            )
    }
}

impl ImageViewer {
    fn render_image(&self) -> impl IntoElement {
        let Some(loaded) = &self.loaded else {
            return div().into_any_element();
        };
        let image = img(loaded.image.clone())
            .id("open-image")
            .object_fit(if self.fit {
                ObjectFit::Contain
            } else {
                ObjectFit::None
            })
            .with_loading(|| {
                div()
                    .text_color(theme::ash())
                    .child("Decoding image…")
                    .into_any_element()
            })
            .with_fallback(|| {
                div()
                    .text_color(theme::ash())
                    .child("This image could not be decoded.")
                    .into_any_element()
            });
        if self.fit {
            image.size_full().into_any_element()
        } else {
            div()
                .id("image-actual")
                .size_full()
                .overflow_scroll()
                .child(image)
                .into_any_element()
        }
    }
}

fn chrome_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .tab_index(0)
        .cursor_pointer()
        .flex_shrink_0()
        .h(px(chrome::CONTROL_HEIGHT))
        .px(px(chrome::INSET))
        .flex()
        .items_center()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::edge_soft())
        .font_family(chrome::CHROME_FONT)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(chrome::CONTROL_TEXT_SIZE))
        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
        .text_color(theme::bone())
        .hover(|button| button.bg(theme::panel_hover()))
        .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
}

fn mode_button(id: &'static str, selected: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .tab_index(0)
        .cursor_pointer()
        .flex_shrink_0()
        .h(px(chrome::CONTROL_HEIGHT))
        .px(px(chrome::INSET))
        .flex()
        .items_center()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(if selected {
            theme::focus()
        } else {
            theme::edge_soft()
        })
        .bg(if selected {
            theme::panel()
        } else {
            theme::canvas()
        })
        .font_family(chrome::CHROME_FONT)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(chrome::CONTROL_TEXT_SIZE))
        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
        .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x18, 0xDD, 0x8D, 0xB4, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn status_copy_includes_format_pixels_and_size() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
        assert_eq!(gpui_format(ImageKind::Webp), gpui::ImageFormat::Webp);
    }

    #[gpui::test]
    fn viewer_loads_png_and_keeps_the_last_image_on_reload_error(cx: &mut TestAppContext) {
        struct TestDirectory(PathBuf);
        impl Drop for TestDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let directory = TestDirectory(std::env::temp_dir().join(format!(
            "pideck-image-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join("pixel.png");
        std::fs::write(&path, TINY_PNG).unwrap();
        let project = directory.0.clone();
        let file = path.clone();
        let (viewer, cx) =
            cx.add_window_view(move |window, cx| ImageViewer::open(project, file, window, cx));
        cx.run_until_parked();
        viewer.read_with(cx, |viewer, _| {
            assert!(!viewer.is_busy());
            assert_eq!(viewer.title(), "pixel.png");
            let loaded = viewer.loaded.as_ref().expect("loaded image");
            assert_eq!(loaded.format, ImageKind::Png);
            assert_eq!((loaded.width, loaded.height), (Some(1), Some(1)));
            assert!(viewer.status().contains("PNG"));
            assert!(viewer.fit);
        });

        std::fs::write(&path, b"broken").unwrap();
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| viewer.load(window, cx));
        });
        cx.run_until_parked();
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(viewer.error, Some(FileError::NotImage));
            assert!(viewer.loaded.is_some(), "keep the last valid image");
            assert!(viewer.status().contains("PNG"));
        });
    }
}
