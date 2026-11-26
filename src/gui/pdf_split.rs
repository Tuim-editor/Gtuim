//! Open a PDF beside the current source without editing a binary Neovim buffer.
use super::{
    pdf_preview::{is_pdf_path, PdfViewMode},
    GtuimView,
};
use gpui::{Context, PathPromptOptions};
use std::path::PathBuf;

impl GtuimView {
    pub fn open_pdf_in_split(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open PDF in Split".into()),
        });
        cx.spawn(async move |view, cx| {
            let result = selection.await;
            let _ = view.update(cx, |this, cx| match result {
                Ok(Ok(paths)) => this.accept_pdf_split_selection(paths, cx),
                Ok(Err(error)) => {
                    this.app
                        .set_notice("error", &format!("Could not open PDF file picker: {error}"));
                    cx.notify();
                }
                Err(error) => {
                    this.app.set_notice(
                        "error",
                        &format!("PDF file picker closed unexpectedly: {error}"),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // Keep selection handling separate from the native dialog so cancellation,
    // validation, and source preservation can be exercised on GPUI's test platform.
    pub(super) fn accept_pdf_split_selection(
        &mut self,
        paths: Option<Vec<PathBuf>>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else {
            return;
        };
        let Some(path_str) = path.to_str() else {
            self.app.set_notice("error", "This PDF path is not valid UTF-8. Rename the file or its parent directory before opening it.");
            cx.notify();
            return;
        };
        if !is_pdf_path(path_str) || !path.is_file() {
            self.app.set_notice(
                "error",
                "Choose an existing .pdf file to open in a split pane.",
            );
            cx.notify();
            return;
        }
        self.open_pdf_path(path_str.to_owned(), PdfViewMode::Split, true, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        modal_tests::fixture,
        pdf_preview::{tests::build_sample_two_page_pdf, PdfViewMode},
    };
    use gpui::TestAppContext;

    #[gpui::test]
    fn selected_pdf_opens_pinned_split_without_switching_source(cx: &mut TestAppContext) {
        let path =
            std::env::temp_dir().join(format!("gtuim-split-selection-{}.pdf", std::process::id()));
        std::fs::write(&path, build_sample_two_page_pdf()).unwrap();
        let (view, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.tabs[0].name = "source.rs".into();
            let original_bufnr = view.app.tabs[0].bufnr;
            view.accept_pdf_split_selection(Some(vec![path.clone()]), cx);
            assert_eq!(view.app.tabs[view.app.active_tab].name, "source.rs");
            assert_eq!(view.app.tabs[view.app.active_tab].bufnr, original_bufnr);
            assert!(view.pdf_preview.open && view.pdf_preview.pinned);
            assert_eq!(view.pdf_preview.mode, PdfViewMode::Split);
            assert_eq!(view.pdf_preview.path.as_deref(), path.to_str());
            view.sync_pdf_preview(cx);
            assert!(view.pdf_preview.open);
            view
        });
        window.run_until_parked();
        view.update(window, |view, cx| {
            let selected = view.pdf_preview.path.clone();
            view.accept_pdf_split_selection(None, cx);
            assert_eq!(
                view.pdf_preview.path, selected,
                "cancel must preserve existing preview"
            );
            view.accept_pdf_split_selection(
                Some(vec![std::path::PathBuf::from("not-a-pdf.rs")]),
                cx,
            );
            assert_eq!(
                view.pdf_preview.path, selected,
                "invalid selection must preserve existing preview"
            );
            assert!(view.app.notice.is_some());
            view.app.tabs[view.app.active_tab].name = "/tmp/different-active.pdf".into();
            view.sync_pdf_preview(cx);
            assert_eq!(view.pdf_preview.path, selected, "a different PDF buffer must not replace a pinned selection");
            assert!(view.pdf_preview.pinned && view.pdf_preview.open);
        });
        let _ = std::fs::remove_file(path);
    }

    #[gpui::test]
    fn split_command_is_available_to_palette_and_routes_to_gui(cx: &mut TestAppContext) {
        assert!(crate::tui::workspace::COMMANDS
            .iter()
            .any(|command| command.id == "open_pdf_in_split"));
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            crate::tui::events::execute_command(&mut view.app, "open_pdf_in_split");
            assert!(view.app.open_pdf_in_split_requested);
            view
        });
        window.run_until_parked();
    }
}
