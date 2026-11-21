//! Display lines of one cell: border, source, separator and outputs.

use europa_core::{
    Cell, DecodedImage, RenderOptions, RenderedCell, SourceLanguage, Style, StyledLine, StyledSpan,
};

/// Render one cell as border, source, separator and output lines.
pub(crate) fn cell_display(
    cell: &Cell,
    number: usize,
    language: SourceLanguage,
    options: &RenderOptions,
    sink: &mut dyn FnMut(u64, DecodedImage),
) -> RenderedCell {
    let width = usize::from(options.width.get());
    let (kind, source_lines) = match cell {
        Cell::Code(code) => {
            let count = match code.execution_count {
                Some(n) => n.to_string(),
                None => " ".to_owned(),
            };
            let kind = format!("code [{count}]");
            let lines = europa_render::render_source(code.source.text(), language, options);
            (kind, lines)
        }
        Cell::Markdown(markdown) => {
            let lines = europa_render::render_markdown(markdown.source.text(), options);
            ("markdown".to_owned(), lines)
        }
        Cell::Raw(raw) => {
            let lines =
                europa_render::render_source(raw.source.text(), SourceLanguage::Plain, options);
            ("raw".to_owned(), lines)
        }
        Cell::Unknown(unknown) => {
            let name = match unknown.value.get("cell_type") {
                Some(europa_core::JsonValue::String(text)) => text.clone(),
                _ => String::new(),
            };
            let converted: String =
                europa_render::render_source(&name, SourceLanguage::Plain, options)
                    .iter()
                    .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
                    .collect();
            let notice = format!("[unknown cell type: {converted}]");
            let cut: String = notice.chars().take(width.max(1)).collect();
            let lines = vec![StyledLine {
                spans: vec![StyledSpan {
                    text: cut,
                    style: Style {
                        dim: true,
                        ..Style::default()
                    },
                }],
                fill: None,
            }];
            ("?".to_owned(), lines)
        }
    };
    let mut lines = Vec::new();
    lines.push(border_line(number, &kind, width));
    lines.extend(source_lines);
    let mut images = Vec::new();
    if let Cell::Code(code) = cell {
        let rendered = europa_render::render_outputs_with_images(&code.outputs, options, sink);
        if !rendered.lines.is_empty() {
            lines.push(separator_line(width));
            let offset = lines.len();
            lines.extend(rendered.lines);
            images = rendered
                .images
                .into_iter()
                .map(|mut slot| {
                    slot.line += offset;
                    slot
                })
                .collect();
        }
    }
    RenderedCell { lines, images }
}

fn border_line(number: usize, kind: &str, width: usize) -> StyledLine {
    let head = format!("== {number} {kind} ");
    let text = if head.chars().count() >= width {
        head.chars().take(width).collect()
    } else {
        let mut text = head;
        text.extend(std::iter::repeat_n('=', width - text.chars().count()));
        text
    };
    StyledLine {
        spans: vec![StyledSpan {
            text,
            style: Style {
                bold: true,
                ..Style::default()
            },
        }],
        fill: None,
    }
}

fn separator_line(width: usize) -> StyledLine {
    let head = "-- out ";
    let text = if head.chars().count() >= width {
        head.chars().take(width).collect()
    } else {
        let mut text = head.to_owned();
        text.extend(std::iter::repeat_n('-', width - text.chars().count()));
        text
    };
    StyledLine {
        spans: vec![StyledSpan {
            text,
            style: Style {
                dim: true,
                ..Style::default()
            },
        }],
        fill: None,
    }
}
