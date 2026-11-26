//! Native, inert Markdown preview. Never executes embedded HTML or fetches remote content.
use super::{icons::gui_palette, GtuimView};
use gpui::{prelude::*, *};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use rmpv::Value;

// Read the live buffer, not the on-disk file. Coalesce insert-mode changes and
// bound payloads so opening a very large document cannot flood the RPC pipe.
const WATCH_BUFFER: &str = r#"
_G.gtuim_markdown_enabled = true
local function publish()
  if not _G.gtuim_markdown_enabled or vim.api.nvim_win_get_config(0).relative ~= '' then return end
  local name = vim.api.nvim_buf_get_name(0)
  local ext = name:lower():match('%.([^%.]+)$') or ''
  local valid = vim.bo.filetype == 'markdown' or ext == 'md' or ext == 'markdown' or ext == 'mdown' or ext == 'mkd'
  local lines, size, truncated = {}, 0, false
  if valid then
    -- Do not copy 20,000 potentially huge lines into Lua before applying the
    -- byte budget. Chunk retrieval keeps ordinary large documents bounded.
    local limit = math.min(vim.api.nvim_buf_line_count(0), 20000)
    for start = 0, limit - 1, 64 do
      for _, line in ipairs(vim.api.nvim_buf_get_lines(0, start, math.min(start + 64, limit), false)) do
        size = size + #line + 1
        if size > 1048576 then truncated = true; break end
        lines[#lines + 1] = line
      end
      if truncated then break end
    end
    if vim.api.nvim_buf_line_count(0) > #lines then truncated = true end
  end
  vim.rpcnotify(1, 'gtuim_markdown_preview', name, valid, table.concat(lines, '\n'), truncated, vim.api.nvim_get_current_buf())
end
local group = vim.api.nvim_create_augroup('GtuimMarkdownPreview', {clear=true})
local generation = 0
vim.api.nvim_create_autocmd({'BufEnter', 'BufReadPost', 'BufFilePost', 'TextChanged', 'TextChangedI', 'TextChangedP', 'BufWritePost', 'FileType'}, {
  group=group, callback=function()
    if not _G.gtuim_markdown_enabled or vim.api.nvim_win_get_config(0).relative ~= '' then return end
    generation = generation + 1
    local current = generation
    vim.defer_fn(function() if current == generation then publish() end end, 120)
  end
})
publish()
"#;

const STOP_BUFFER: &str = "_G.gtuim_markdown_enabled = false; pcall(vim.api.nvim_del_augroup_by_name, 'GtuimMarkdownPreview')";

#[derive(Default)]
pub struct MarkdownPreview {
    pub open: bool,
    pub name: String,
    pub valid: bool,
    pub loading: bool,
    pub truncated: bool,
    blocks: Vec<Block>,
    bufnr: Option<i64>,
    scroll: ScrollHandle,
}

#[derive(Debug, Default)]
struct Block {
    text: String,
    heading: u8,
    code: bool,
    quote: bool,
    rule: bool,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
}

fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut block = Block::default();
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut strike = 0usize;
    let mut quote = 0usize;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut links = Vec::new();
    let mut cell_index = 0usize;
    fn flush(block: &mut Block, blocks: &mut Vec<Block>) {
        if !block.text.is_empty() || block.rule {
            blocks.push(std::mem::take(block));
        }
    }
    for event in Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush(&mut block, &mut blocks);
                    block.heading = level as u8;
                    block.quote = quote > 0;
                }
                Tag::Paragraph => {
                    if lists.is_empty() {
                        flush(&mut block, &mut blocks);
                    }
                    if !lists.is_empty() && block.text.is_empty() {
                        block.text.push_str(&"    ".repeat(lists.len()));
                    }
                    block.quote = quote > 0;
                }
                Tag::CodeBlock(_) => {
                    flush(&mut block, &mut blocks);
                    block.code = true;
                    block.quote = quote > 0;
                }
                Tag::BlockQuote(_) => {
                    flush(&mut block, &mut blocks);
                    quote += 1;
                    block.quote = true;
                }
                Tag::List(start) => {
                    flush(&mut block, &mut blocks);
                    lists.push(start);
                }
                Tag::Item => {
                    flush(&mut block, &mut blocks);
                    block
                        .text
                        .push_str(&"    ".repeat(lists.len().saturating_sub(1)));
                    if let Some(Some(number)) = lists.last_mut() {
                        block.text.push_str(&format!("{}. ", number));
                        *number += 1;
                    } else {
                        block.text.push_str("• ");
                    }
                    block.quote = quote > 0;
                }
                Tag::Strong => bold += 1,
                Tag::Emphasis => italic += 1,
                Tag::Strikethrough => strike += 1,
                Tag::Link { dest_url, .. } => links.push((dest_url.to_string(), block.text.len())),
                Tag::Image { .. } => block.text.push_str("[Image: "),
                Tag::TableHead => {
                    flush(&mut block, &mut blocks);
                    bold += 1;
                    cell_index = 0;
                }
                Tag::TableRow => {
                    flush(&mut block, &mut blocks);
                    cell_index = 0;
                }
                Tag::TableCell => {
                    if cell_index > 0 {
                        block.text.push_str("  │  ");
                    }
                    cell_index += 1;
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow => {
                    flush(&mut block, &mut blocks)
                }
                TagEnd::TableHead => {
                    flush(&mut block, &mut blocks);
                    bold = bold.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    if block.text.ends_with('\n') {
                        block.text.pop();
                    }
                    flush(&mut block, &mut blocks);
                }
                TagEnd::BlockQuote(_) => {
                    flush(&mut block, &mut blocks);
                    quote = quote.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    flush(&mut block, &mut blocks);
                    lists.pop();
                }
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Strikethrough => strike = strike.saturating_sub(1),
                TagEnd::Link => {
                    if let Some((url, start)) = links.pop() {
                        if block.text.get(start..) != Some(url.as_str()) {
                            block.text.push_str(&format!(" ({url})"));
                        }
                    }
                }
                TagEnd::Image => block.text.push(']'),
                _ => {}
            },
            Event::Code(text) => {
                let start = block.text.len();
                block.text.push_str(&text);
                block.highlights.push((
                    start..block.text.len(),
                    HighlightStyle {
                        background_color: Some(rgba(0x80808030).into()),
                        font_weight: Some(FontWeight::MEDIUM),
                        ..HighlightStyle::default()
                    },
                ));
            }
            Event::Text(text) => {
                let start = block.text.len();
                block.text.push_str(&text);
                let mut style = HighlightStyle::default();
                if bold > 0 {
                    style.font_weight = Some(FontWeight::BOLD);
                }
                if italic > 0 {
                    style.font_style = Some(FontStyle::Italic);
                }
                if strike > 0 {
                    style.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: None,
                    });
                }
                if bold > 0 || italic > 0 || strike > 0 {
                    block.highlights.push((start..block.text.len(), style));
                }
            }
            Event::SoftBreak => block.text.push(if block.code { '\n' } else { ' ' }),
            Event::HardBreak => block.text.push('\n'),
            Event::Rule => {
                flush(&mut block, &mut blocks);
                blocks.push(Block {
                    rule: true,
                    ..Block::default()
                });
            }
            Event::TaskListMarker(checked) => {
                if block.text.ends_with("• ") {
                    block.text.truncate(block.text.len() - "• ".len());
                }
                block.text.push_str(if checked { "☑ " } else { "☐ " })
            }
            // HTML is deliberately inert, displayed literally rather than executed.
            Event::Html(text) | Event::InlineHtml(text) => block.text.push_str(&text),
            _ => {}
        }
    }
    flush(&mut block, &mut blocks);
    blocks
}

impl MarkdownPreview {
    pub fn update(&mut self, params: &[Value]) {
        if !self.open {
            return;
        }
        let name = params.first().and_then(Value::as_str).unwrap_or_default();
        let bufnr = params.get(4).and_then(Value::as_i64);
        self.loading = false;
        if self.bufnr != bufnr || (bufnr.is_none() && self.name != name) {
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        self.bufnr = bufnr;
        self.name = name.to_string();
        self.valid = params.get(1).and_then(Value::as_bool).unwrap_or(false);
        self.blocks = parse(params.get(2).and_then(Value::as_str).unwrap_or_default());
        self.truncated = params.get(3).and_then(Value::as_bool).unwrap_or(false);
    }
}

impl GtuimView {
    pub fn toggle_markdown_preview(&mut self, cx: &mut Context<Self>) {
        self.markdown_preview.open = !self.markdown_preview.open;
        let lua = if self.markdown_preview.open {
            self.markdown_preview.loading = true;
            WATCH_BUFFER
        } else {
            self.markdown_preview = MarkdownPreview::default();
            STOP_BUFFER
        };
        let _ = self.app.editor_rpc.notify(
            "nvim_exec_lua",
            vec![Value::from(lua), Value::Array(vec![])],
        );
        cx.notify();
    }
}

pub fn render_toggle(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    div()
        .id("markdown-preview-toggle")
        .debug_selector(|| "markdown-preview-toggle".into())
        .flex_shrink_0()
        .bg(rgb(p.bg_panel))
        .border_b_1()
        .border_color(rgb(p.border))
        .px_3()
        .py_1()
        .text_size(px(12.))
        .text_color(rgb(p.accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(p.bg_hover)))
        .child(if view.markdown_preview.open {
            "Hide Markdown preview · Ctrl+Shift+M"
        } else {
            "Markdown preview · Ctrl+Shift+M"
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_markdown_preview(cx)))
        .into_any_element()
}

pub fn render(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    let state = &view.markdown_preview;
    let content = div()
        .id("markdown-preview-content")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_x_hidden()
        .overflow_y_scroll()
        .track_scroll(&state.scroll)
        .p_4()
        .text_color(rgb(p.text))
        .text_size(px(view.font_size))
        .line_height(relative(1.55))
        .whitespace_normal()
        .when(state.loading, |d| d.child("Loading Markdown preview…"))
        .when(!state.loading && !state.valid, |d| {
            d.child("Open a Markdown buffer to preview it here.")
        })
        .when(
            state.valid && state.blocks.is_empty() && !state.truncated,
            |d| d.child("This Markdown document is empty."),
        )
        .when(state.truncated, |d| {
            d.child(
                div()
                    .text_color(rgb(p.warning))
                    .child("Preview limited to 1 MiB / 20,000 lines."),
            )
        })
        .children(state.blocks.iter().map(|block| {
            if block.rule {
                return div().my_3().h(px(1.)).bg(rgb(p.border)).into_any_element();
            }
            div()
                .mb_3()
                .w_full()
                .when(block.heading > 0, |d| {
                    d.text_size(px(view.font_size
                        * match block.heading {
                            1 => 2.0,
                            2 => 1.7,
                            3 => 1.4,
                            _ => 1.2,
                        }))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(p.text_strong))
                })
                .when(block.code, |d| {
                    d.p_3()
                        .bg(rgb(p.bg_surface))
                        .font_family(view.mono_font.clone())
                        .whitespace_normal()
                })
                .when(block.quote, |d| {
                    d.pl_3()
                        .border_l_2()
                        .border_color(rgb(p.accent))
                        .text_color(rgb(p.text_muted))
                })
                .child(
                    StyledText::new(block.text.clone()).with_highlights(block.highlights.clone()),
                )
                .into_any_element()
        }));
    div()
        .id("markdown-preview-pane")
        .debug_selector(|| "markdown-preview-pane".into())
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .h_full()
        .border_l_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.bg_editor))
        .child(render_toggle(view, cx))
        .child(content)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn initial_loading_and_document_identity_are_not_confused_with_truncation() {
        let mut state = MarkdownPreview {
            open: true,
            loading: true,
            ..Default::default()
        };
        state.update(&[
            "large.md".into(),
            true.into(),
            "# Large".into(),
            true.into(),
            42.into(),
        ]);
        assert!(!state.loading);
        assert!(state.valid && state.truncated);
        assert_eq!(state.bufnr, Some(42));
        state.update(&[
            "large.md".into(),
            true.into(),
            "# Large".into(),
            false.into(),
            43.into(),
        ]);
        assert_eq!(state.bufnr, Some(43));
        assert!(!state.truncated);
    }
    #[::core::prelude::v1::test]
    fn renders_common_markdown() {
        let blocks = parse("# Title\n\nHello **bold** and *italic*.\n\n- [x] Done\n- Next\n\n> Quote\n\n```rs\nlet x = 1;\n```\n\n---\n\n[Link](https://example.org)\n\n![alt](x.png)");
        assert!(blocks.iter().any(|b| b.heading == 1 && b.text == "Title"));
        assert!(blocks
            .iter()
            .any(|b| b.text.contains("Hello bold and italic.")));
        assert!(blocks.iter().any(|b| b.text.contains("☑ Done")));
        assert!(blocks.iter().any(|b| b.quote && b.text == "Quote"));
        assert!(blocks
            .iter()
            .any(|b| b.code && b.text.contains("let x = 1;")));
        assert!(blocks.iter().any(|b| b.rule));
        assert!(blocks
            .iter()
            .any(|b| b.text.contains("https://example.org")));
        assert!(blocks.iter().any(|b| b.text.contains("[Image: alt]")));
    }
    #[::core::prelude::v1::test]
    fn styles_tasks_tables_and_nested_lists_are_clean() {
        let blocks = parse("~~deleted~~ and plain\n\n- [x] Done\n- [ ] Todo\n\n| A | B |\n|---|---|\n| C | D |\n\n- outer\n  - inner\n\n<https://example.org>");
        let strike = blocks[0].highlights[0].1.strikethrough.as_ref().unwrap();
        assert_eq!(strike.thickness, px(1.0));
        assert!(blocks.iter().any(|b| b.text == "☑ Done"));
        assert!(blocks.iter().any(|b| b.text == "☐ Todo"));
        assert!(blocks.iter().any(|b| b.text == "A  │  B"));
        assert!(blocks.iter().any(|b| b.text == "C  │  D"));
        assert!(blocks.iter().any(|b| b.text == "    • inner"));
        assert_eq!(blocks.last().unwrap().text, "https://example.org");
        assert!(parse("plain")[0].highlights.is_empty());
        assert_eq!(parse("```\nx\n```")[0].text, "x");
    }

    #[::core::prelude::v1::test]
    fn quoted_headings_and_code_keep_quote_styling() {
        let blocks = parse("> # Quoted heading\n>\n> ```rust\n> let x = 1;\n> ```");
        assert!(blocks.iter().any(|b| b.quote && b.heading == 1));
        assert!(blocks.iter().any(|b| b.quote && b.code));
    }

    #[::core::prelude::v1::test]
    fn live_neovim_buffer_updates_include_unsaved_changes() {
        let check = format!(
            r#"lua
local received = {{}}
vim.rpcnotify = function(_, method, ...) if method == 'gtuim_markdown_preview' then received[#received+1] = {{...}} end end
{WATCH_BUFFER}
vim.cmd('file live.md')
vim.bo.filetype = 'markdown'
vim.api.nvim_buf_set_lines(0, 0, -1, false, {{'# Unsaved', '', '**Live** text'}})
vim.api.nvim_exec_autocmds('TextChanged', {{}})
assert(vim.wait(1000, function() local r = received[#received]; return r and r[3]:find('Live', 1, true) ~= nil end))
local r = received[#received]
assert(r[1]:match('live.md$') and r[2] == true and r[3] == '# Unsaved\n\n**Live** text' and r[4] == false)
local count_before_float = #received
local floatbuf = vim.api.nvim_create_buf(false, true)
local floatwin = vim.api.nvim_open_win(floatbuf, true, {{relative='editor', row=1, col=1, width=20, height=4, style='minimal'}})
vim.wait(250)
assert(#received == count_before_float, 'floating buffer must not wipe preview')
vim.api.nvim_win_close(floatwin, true)
vim.api.nvim_buf_set_lines(0, 0, -1, false, {{string.rep('x', 1048577)}})
vim.api.nvim_exec_autocmds('TextChangedP', {{}})
assert(vim.wait(1000, function() return received[#received][4] == true end), 'oversized first line must be truncated')
local many = {{}}
for i = 1, 20001 do many[i] = 'line' end
vim.api.nvim_buf_set_lines(0, 0, -1, false, many)
local before_cap = #received
vim.api.nvim_exec_autocmds('TextChanged', {{}})
assert(vim.wait(1000, function() return #received > before_cap end))
assert(received[#received][4] == true, 'line cap must be explicit')
local _, count_newlines = received[#received][3]:gsub('\n', '')
assert(count_newlines == 19999, 'line cap must be exactly 20000')
vim.cmd('enew!')
assert(vim.wait(1000, function() return received[#received][2] == false end))
{STOP_BUFFER}
assert(not pcall(vim.api.nvim_get_autocmds, {{group='GtuimMarkdownPreview'}}), 'close must remove watcher')
local count = #received
vim.api.nvim_exec_autocmds('TextChanged', {{}})
vim.wait(250)
assert(#received == count)
io.stdout:write('GTUIM_MARKDOWN_LIVE_OK\n')
vim.cmd('qa!')
"#
        );
        let output = std::process::Command::new("nvim")
            .args([
                "--headless",
                "-n",
                "-u",
                "NONE",
                "-i",
                "NONE",
                "-c",
                &check,
                "-c",
                "qa!",
            ])
            .output()
            .expect("Neovim is required for the live preview integration test");
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("GTUIM_MARKDOWN_LIVE_OK"),
            "Neovim preview integration failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[::core::prelude::v1::test]
    fn preserves_unicode_and_inert_html() {
        let blocks = parse("**hé🦀**\n\n<script>alert('no')</script>");
        assert_eq!(blocks[0].text, "hé🦀");
        assert_eq!(blocks[0].highlights[0].0, 0.."hé🦀".len());
        assert!(blocks.iter().any(|b| b.text.contains("<script>")));
    }
    #[::core::prelude::v1::test]
    fn preview_tracks_live_payload_and_ignores_closed_updates() {
        let mut state = MarkdownPreview::default();
        let payload = vec![
            Value::from("unsaved.md"),
            Value::from(true),
            Value::from("# Unsaved"),
            Value::from(false),
        ];
        state.update(&payload);
        assert!(state.blocks.is_empty());
        state.open = true;
        state.update(&payload);
        assert_eq!(state.blocks[0].text, "Unsaved");
        state.update(&[
            Value::from("main.rs"),
            Value::from(false),
            Value::from(""),
            Value::from(false),
        ]);
        assert!(!state.valid);
        assert!(state.blocks.is_empty());
    }
}
