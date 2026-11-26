//! Width-constrained modal primitives. Flex items default to an intrinsic minimum
//! width; a long plugin description must not make an entire scroll pane wider than
//! the dialog. List cards must also keep their natural height rather than shrinking.
use gpui::{prelude::*, *};

pub(super) fn scroll_column(id: &'static str) -> Stateful<Div> {
    div().id(id).debug_selector(move || id.to_string())
        .flex_1().w_full().min_w_0().min_h_0().overflow_y_scroll()
}

pub(super) fn list_card() -> Div {
    div().w_full().min_w_0().flex_shrink_0()
}

#[cfg(test)]
mod tests {
    use super::{list_card, scroll_column};
    use gpui::{prelude::*, div, px, relative, Context, IntoElement, Render, TestAppContext, Window};
    use crate::{gui::{icons::gui_palette, modals::modal_card}, tui::theme::Theme};

    struct LayoutFixture { width: f32, height: f32, font_size: f32 }

    impl Render for LayoutFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let p = gui_palette(&Theme::default());
            let long_description = format!("catppuccin/nvim · Bundled | Enabled | Themes: {}",
                "catppuccin-latte, catppuccin-frappe, catppuccin-macchiato, catppuccin-mocha; ".repeat(12));
            div().w(px(self.width)).h(px(self.height)).flex().items_center().justify_center()
                .font_family("monospace").text_size(px(self.font_size))
                .child(modal_card(p, 1020.0, 700.0).debug_selector(|| "dialog".into())
                    .child(div().h(px(40.0)).flex_shrink_0().child("Plugin Center"))
                    .child(div().w_full().min_w_0().flex_1().min_h_0().flex()
                        .child(div().w(px(200.0)).max_w(relative(0.30)).flex_shrink_0().child("Supported recipes"))
                        .child(scroll_column("test-list").debug_selector(|| "list".into()).p_3().flex().flex_col().gap_2()
                            .child(list_card().debug_selector(|| "row".into()).p_3().flex().flex_col().gap_1()
                                .child(div().w_full().min_w_0().flex().flex_wrap().gap_2().items_center().justify_between()
                                    .child(div().flex_basis(px(160.0)).flex_grow().min_w_0().child("catppuccin"))
                                    .child(div().flex_shrink_0().whitespace_nowrap().text_size(px(10.5))
                                        .debug_selector(|| "badge".into()).child("Community / best effort · Dependencies ready")))
                                .child(div().w_full().min_w_0().whitespace_normal()
                                    .debug_selector(|| "description".into()).child(long_description)))
                            .child(list_card().h(px(80.0)).child("Next plugin"))))
                    .child(div().h(px(40.0)).flex_shrink_0().debug_selector(|| "footer".into()).child("Close")))
        }
    }

    fn check_fixture(cx: &mut TestAppContext, width: f32, height: f32, font_size: f32) {
        let (_, window) = cx.add_window_view(|_, _| LayoutFixture { width, height, font_size });
        let dialog = window.debug_bounds("dialog").expect("dialog rendered");
        let list = window.debug_bounds("list").expect("list rendered");
        let row = window.debug_bounds("row").expect("row rendered");
        let badge = window.debug_bounds("badge").expect("badge rendered");
        let description = window.debug_bounds("description").expect("description rendered");
        let footer = window.debug_bounds("footer").expect("footer rendered");
        assert!(dialog.size.width <= px(width * 0.92 + 1.0));
        assert!(dialog.size.height <= px(height * 0.88 + 1.0));
        for (name, bounds) in [("list", list), ("row", row), ("badge", badge), ("description", description)] {
            assert!(bounds.right() <= dialog.right() + px(1.0), "{name} exceeds dialog: {bounds:?} vs {dialog:?}");
        }
        assert!(badge.right() <= row.right() + px(1.0), "status badge must remain visible");
        assert!(badge.size.width >= px(200.0), "badge must have readable width");
        assert!(badge.size.height <= px(font_size * 2.0), "badge must not wrap into a vertical ribbon");
        assert!(description.size.height > px(font_size * 2.0), "long prose must wrap, not just be clipped");
        assert!(list.bottom() <= footer.top() + px(1.0), "list must scroll above visible footer");
        assert_eq!(footer.size.height, px(40.0), "footer cannot shrink out of view");
        assert!(row.size.height > px(80.0), "wrapped rows keep their natural height");
    }

    #[gpui::test]
    fn long_modal_text_and_badges_stay_inside_small_dialog(cx: &mut TestAppContext) {
        check_fixture(cx, 720.0, 460.0, 14.0);
    }

    #[gpui::test]
    fn large_fonts_and_long_descriptions_preserve_footer(cx: &mut TestAppContext) {
        check_fixture(cx, 960.0, 640.0, 22.0);
    }

    #[gpui::test]
    fn normal_size_dialog_does_not_expand_to_intrinsic_text_width(cx: &mut TestAppContext) {
        check_fixture(cx, 1380.0, 880.0, 14.0);
    }
}
