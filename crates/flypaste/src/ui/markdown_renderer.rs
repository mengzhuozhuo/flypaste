use std::collections::HashMap;
use gpui::{div, prelude::*, ElementId, Entity, IntoElement, RenderOnce, Styled, Window, App, StyledText, HighlightStyle};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ui::prelude::*;
use crate::ui::animated_gif::AnimatedGifView;

#[derive(IntoElement)]
pub struct MarkdownRenderer {
    id: ElementId,
    content: String,
    gif_views: HashMap<String, Entity<AnimatedGifView>>,
}

impl MarkdownRenderer {
    pub fn new(id: impl Into<ElementId>, content: impl Into<String>) -> Self {
        let content = content.into();
        // Preprocess custom icon syntax `![[path]]` into invisible unicode markers `\u{E000}path\u{E001}` 
        // to prevent `pulldown_cmark` from splitting the text segment.
        let content = content.replace("![[", "\u{E000}").replace("]]", "\u{E001}");

        Self {
            id: id.into(),
            content,
            gif_views: HashMap::new(),
        }
    }

    pub fn with_gif_views(mut self, gif_views: HashMap<String, Entity<AnimatedGifView>>) -> Self {
        self.gif_views = gif_views;
        self
    }
}

fn push_text_chunks(text: &str, style: &HighlightStyle, mut container: gpui::Div) -> gpui::Div {
    let mut last_idx = 0;
    for (i, c) in text.char_indices() {
        if c == ' ' {
            let chunk = &text[last_idx..=i];
            container = container.child(div().child(StyledText::new(chunk.to_string()).with_highlights(vec![(0..chunk.len(), style.clone())])));
            last_idx = i + c.len_utf8();
        } else if !c.is_ascii() {
            if last_idx < i {
                let chunk = &text[last_idx..i];
                container = container.child(div().child(StyledText::new(chunk.to_string()).with_highlights(vec![(0..chunk.len(), style.clone())])));
            }
            let end = i + c.len_utf8();
            let chunk = &text[i..end];
            container = container.child(div().child(StyledText::new(chunk.to_string()).with_highlights(vec![(0..chunk.len(), style.clone())])));
            last_idx = end;
        }
    }
    if last_idx < text.len() {
        let chunk = &text[last_idx..];
        container = container.child(div().child(StyledText::new(chunk.to_string()).with_highlights(vec![(0..chunk.len(), style.clone())])));
    }
    container
}

impl RenderOnce for MarkdownRenderer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let parser = Parser::new(&self.content);
        
        let mut blocks = Vec::new();
        let mut current_runs: Vec<(String, HighlightStyle)> = Vec::new();
        
        let mut in_heading = false;
        let mut heading_level = 1;
        let mut list_depth: u32 = 0;
        let mut list_item_depth: u32 = 0;
        let mut in_blockquote = false;
        let mut in_image = false;
        
        let mut bold_start = false;
        let mut code_start = false;

        macro_rules! flush_block {
            ($is_heading:expr, $level:expr, $is_list_item:expr, $is_blockquote:expr) => {
                if !current_runs.is_empty() {
                    let has_icon = current_runs.iter().any(|(text, _)| text.contains('\u{E000}'));
                    
                    if has_icon {
                        // Advanced rendering ONLY for paragraphs that contain SVGs. 
                        // It chunks characters to allow precise inline wrapping around SVG elements.
                        let mut flex_container = div().flex().flex_wrap().gap_0().items_center().w_full().min_w_0();
                        
                        for (text, style) in &current_runs {
                            let mut remaining = text.as_str();
                            while let Some(start) = remaining.find('\u{E000}') {
                                if start > 0 {
                                    let text_before = &remaining[..start];
                                    flex_container = push_text_chunks(text_before, style, flex_container);
                                }
                                remaining = &remaining[start + 3..];
                                if let Some(end) = remaining.find('\u{E001}') {
                                    let path = &remaining[..end];
                                    let clean_url = path.trim_start_matches("./").trim_start_matches("/");
                                    let asset_path = format!("docs/{}", clean_url);
                                    
                                    if crate::assets::Assets::get(&asset_path).is_some() {
                                        let svg_element = gpui::svg()
                                            .path(asset_path)
                                            .size(gpui::px(16.0))
                                            .text_color(cx.theme().colors().text);
                                            
                                        flex_container = flex_container.child(div().mx_1().mt_1().child(svg_element).flex_shrink_0());
                                    }
                                    remaining = &remaining[end + 3..];
                                }
                            }
                            if !remaining.is_empty() {
                                flex_container = push_text_chunks(remaining, style, flex_container);
                            }
                        }

                        if $is_list_item {
                            let margin = gpui::px(16.0 * list_depth as f32);
                            let mut list_row = div().ml(margin).mb_1().flex().flex_row().gap_2().items_start().w_full().min_w_0();
                            list_row = list_row.child(div().child("•").text_color(gpui::rgba(0x888888FF)).mt_px());
                            list_row = list_row.child(flex_container);
                            blocks.push(list_row.into_any_element());
                        } else {
                            if $is_blockquote {
                                flex_container = flex_container.mb_2().pl_3().border_l_4().border_color(gpui::rgba(0x888888FF)).text_color(gpui::rgba(0x888888FF));
                            } else {
                                flex_container = flex_container.mb_2();
                            }
                            blocks.push(flex_container.into_any_element());
                        }
                    } else {
                        // Fast, native rendering for standard text blocks (99% of paragraphs)
                        let mut full_text = String::new();
                        let mut highlights = Vec::new();
                        for (text, style) in &current_runs {
                            let start = full_text.len();
                            full_text.push_str(text);
                            if style.font_weight.is_some() || style.background_color.is_some() {
                                highlights.push((start..full_text.len(), style.clone()));
                            }
                        }
                        
                        let styled_text = StyledText::new(full_text).with_highlights(highlights);
                        let mut container = div().w_full().min_w_0();
                        
                        if $is_heading {
                            match $level {
                                1 => container = container.text_xl().font_weight(gpui::FontWeight::BOLD).mt_4().mb_2(),
                                2 => container = container.text_lg().font_weight(gpui::FontWeight::BOLD).mt_3().mb_2(),
                                _ => container = container.font_weight(gpui::FontWeight::BOLD).mt_2().mb_1(),
                            }
                            container = container.child(styled_text);
                        } else if $is_blockquote {
                            container = container.mb_2().pl_3().border_l_4().border_color(gpui::rgba(0x888888FF)).text_color(gpui::rgba(0x888888FF));
                            container = container.child(styled_text);
                        } else if $is_list_item {
                            let margin = gpui::px(16.0 * list_depth as f32);
                            container = container.ml(margin).mb_1().flex().flex_row().gap_2().items_start();
                            container = container.child(div().child("•").text_color(gpui::rgba(0x888888FF)).mt_px());
                            container = container.child(div().flex_1().min_w_0().child(styled_text));
                        } else {
                            container = container.mb_2();
                            container = container.child(styled_text);
                        }

                        blocks.push(container.into_any_element());
                    }
                    
                    current_runs.clear();
                }
            }
        }

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        in_heading = true;
                        heading_level = level as u32;
                    }
                    Tag::List(_) => { 
                        if !current_runs.is_empty() {
                            flush_block!(false, 0, list_item_depth > 0, in_blockquote);
                        }
                        list_depth += 1; 
                    }
                    Tag::BlockQuote(_) => { in_blockquote = true; }
                    Tag::Item => { list_item_depth += 1; }
                    Tag::Strong => { bold_start = true; }
                    Tag::CodeBlock(_) => { code_start = true; }
                    Tag::Image { dest_url, title, .. } => {
                        in_image = true;
                        let url = dest_url.to_string();
                        let clean_url = url.trim_start_matches("./").trim_start_matches("/");
                        let asset_path = format!("docs/{}", clean_url);
                        
                        if clean_url.ends_with(".gif") {
                            if let Some(gif_view) = self.gif_views.get(&asset_path) {
                                flush_block!(false, 0, false, false);
                                blocks.push(gif_view.clone().into_any_element());
                                continue;
                            }
                        }

                        if let Some(asset) = crate::assets::Assets::get(&asset_path) {
                            let format = if clean_url.ends_with(".png") {
                                gpui::ImageFormat::Png
                            } else if clean_url.ends_with(".jpg") || clean_url.ends_with(".jpeg") {
                                gpui::ImageFormat::Jpeg
                            } else if clean_url.ends_with(".gif") {
                                gpui::ImageFormat::Gif
                            } else if clean_url.ends_with(".webp") {
                                gpui::ImageFormat::Webp
                            } else {
                                gpui::ImageFormat::Png
                            };
                            
                            let image = gpui::Image::from_bytes(format, asset.data.into_owned());
                            let image_source = gpui::ImageSource::Image(std::sync::Arc::new(image));
                            let image_element = gpui::img(image_source)
                                .w_full()
                                .max_h(gpui::px(400.0))
                                .object_fit(gpui::ObjectFit::Contain)
                                .mt_2()
                                .mb_4();
                            
                            flush_block!(false, 0, false, false);
                            blocks.push(image_element.into_any_element());
                        } else {
                            let placeholder = div()
                                .child(format!("[图片: {}]", if title.is_empty() { clean_url } else { &title }))
                                .text_color(gpui::rgba(0x888888FF))
                                .mb_2();
                            flush_block!(false, 0, false, false);
                            blocks.push(placeholder.into_any_element());
                        }
                    }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Heading(_) => {
                        flush_block!(true, heading_level, false, false);
                        in_heading = false;
                    }
                    TagEnd::Paragraph => {
                        if list_item_depth == 0 {
                            flush_block!(false, 0, false, in_blockquote);
                        } else if !current_runs.is_empty() {
                            // Separate paragraphs within list items so they don't merge words
                            current_runs.push((" ".to_string(), HighlightStyle::default()));
                        }
                    }
                    TagEnd::List(_) => { list_depth = list_depth.saturating_sub(1); }
                    TagEnd::BlockQuote(_) => { in_blockquote = false; }
                    TagEnd::Item => {
                        flush_block!(false, 0, list_item_depth > 0, in_blockquote);
                        list_item_depth = list_item_depth.saturating_sub(1);
                    }
                    TagEnd::Strong => { bold_start = false; }
                    TagEnd::CodeBlock => { code_start = false; }
                    TagEnd::Image => { in_image = false; }
                    _ => {}
                },
                Event::Text(text) => {
                    if !in_image {
                        let mut style = HighlightStyle::default();
                        if bold_start { style.font_weight = Some(gpui::FontWeight::BOLD); }
                        if code_start { style.background_color = Some(gpui::rgba(0x80808033).into()); }
                        current_runs.push((text.to_string(), style));
                    }
                }
                Event::Code(text) => {
                    if !in_image {
                        let mut style = HighlightStyle::default();
                        style.background_color = Some(gpui::rgba(0x80808033).into());
                        current_runs.push((text.to_string(), style));
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    current_runs.push((" ".to_string(), HighlightStyle::default()));
                }
                _ => {}
            }
        }

        flush_block!(in_heading, heading_level, list_item_depth > 0, in_blockquote);

        div()
            .id(self.id.clone())
            .min_w_0()
            .w_full()
            .flex()
            .flex_col()
            .text_sm()
            .children(blocks)
    }
}
