//! The program that shows a notebook with images in a terminal, to check each
//! protocol by eye, and the measurements that go with it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use europa_core::{CellPixels, ImageProtocolChoice, RenderOptions};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui_image::picker::ProtocolType;

use crate::cell_view::{draw_cell, visible_placements};
use crate::image_cache::DEFAULT_LIMIT;
use crate::image_protocol::ColorDepth;
use crate::image_view::{clear_screen, ImageRenderer, Placement, RatatuiEncoder};

/// What the program was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeCommand {
    /// Show a notebook.
    View {
        /// The notebook to show.
        notebook: PathBuf,
        /// How the protocol is chosen.
        choice: ImageProtocolChoice,
        /// Panic after the first frame, to see that the terminal is restored.
        panic: bool,
    },
    /// Write the notebook of the sample to a file.
    WriteSample(PathBuf),
    /// Measure how long each protocol takes for a large image.
    Bench,
    /// Choose a protocol, and write what was chosen and how long it took.
    SelectOnly(PathBuf),
}

/// Reads the arguments, without touching the terminal or any file.
pub(crate) fn parse_args(args: impl Iterator<Item = OsString>) -> Result<ProbeCommand, String> {
    let mut args = args;
    let mut notebook: Option<PathBuf> = None;
    let mut choice = ImageProtocolChoice::Auto;
    let mut panic = false;
    let mut command: Option<ProbeCommand> = None;
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        match text.as_str() {
            "--bench" => command = Some(ProbeCommand::Bench),
            "--panic" => panic = true,
            "--protocol" => {
                let value = args.next().ok_or("--protocol needs a value")?;
                choice = match value.to_string_lossy().as_ref() {
                    "auto" => ImageProtocolChoice::Auto,
                    "kitty" => ImageProtocolChoice::Kitty,
                    "sixel" => ImageProtocolChoice::Sixel,
                    "iterm2" => ImageProtocolChoice::Iterm2,
                    "halfblock" => ImageProtocolChoice::Halfblock,
                    other => return Err(format!("unknown protocol {other:?}")),
                };
            }
            "--write-sample" => {
                let path = args.next().ok_or("--write-sample needs a path")?;
                command = Some(ProbeCommand::WriteSample(PathBuf::from(path)));
            }
            "--select-only" => {
                let path = args.next().ok_or("--select-only needs a path")?;
                command = Some(ProbeCommand::SelectOnly(PathBuf::from(path)));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            _ => {
                if notebook.replace(PathBuf::from(arg)).is_some() {
                    return Err("only one notebook can be shown".to_owned());
                }
            }
        }
    }
    match (command, notebook) {
        (Some(command), None) => Ok(command),
        (Some(_), Some(_)) => Err("a notebook cannot be combined with that option".to_owned()),
        (None, Some(notebook)) => Ok(ProbeCommand::View {
            notebook,
            choice,
            panic,
        }),
        (None, None) => Err("a notebook, or one of the other commands, is needed".to_owned()),
    }
}

/// The text printed when the arguments are wrong.
pub(crate) const USAGE: &str = "usage:
  image_probe <notebook.ipynb> [--protocol auto|kitty|sixel|iterm2|halfblock] [--panic]
  image_probe --write-sample <path>
  image_probe --bench
  image_probe --select-only <path>";

fn encode_png(image: impl Into<image::DynamicImage>) -> Vec<u8> {
    let mut bytes = Vec::new();
    image
        .into()
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("a PNG encodes to memory");
    bytes
}

/// A picture of bands, a grid and a diagonal, so that a crop or a change of
/// the aspect ratio can be seen.
fn figure(width: u32, height: u32) -> Vec<u8> {
    const BANDS: [[u8; 3]; 6] = [
        [220, 60, 60],
        [230, 160, 50],
        [230, 220, 70],
        [70, 180, 90],
        [60, 120, 220],
        [150, 80, 200],
    ];
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        let band = (x * 6 / width.max(1)) as usize;
        let on_grid = x % 40 == 0 || y % 40 == 0;
        let on_diagonal = (u64::from(x) * u64::from(height))
            .abs_diff(u64::from(y) * u64::from(width))
            < u64::from(width.max(height)) * 2;
        if on_diagonal || on_grid {
            image::Rgb([20, 20, 20])
        } else if y < height / 4 {
            image::Rgb(BANDS[band])
        } else {
            image::Rgb([240, 240, 240])
        }
    });
    encode_png(image)
}

fn jpeg_picture(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        let inside = x > width / 4 && x < width * 3 / 4 && y > height / 4 && y < height * 3 / 4;
        if inside {
            image::Rgb([250, 250, 250])
        } else {
            image::Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, 140])
        }
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .expect("a JPEG encodes to memory");
    bytes
}

/// A disc that is half transparent, on a transparent ground, which shows as
/// white where nothing is drawn.
fn translucent(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        let dx = f64::from(x) - f64::from(width) / 2.0;
        let dy = f64::from(y) - f64::from(height) / 2.0;
        let radius = f64::from(width.min(height)) / 2.5;
        if dx * dx + dy * dy < radius * radius {
            image::Rgba([200, 30, 30, 128])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    encode_png(image)
}

/// A tall picture whose color changes every 100 pixels, to scroll through.
fn tall(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        let step = (y / 100) as u8;
        if y % 100 < 4 {
            image::Rgb([20, 20, 20])
        } else if x < 24 {
            image::Rgb([step * 15, 255 - step * 15, 120])
        } else {
            image::Rgb([60 + step * 12, 90 + step * 5, 200 - step * 10])
        }
    });
    encode_png(image)
}

fn base64(bytes: &[u8]) -> String {
    base64_simd::STANDARD.encode_to_string(bytes)
}

/// The JSON of a notebook with one code cell, whose outputs show what the
/// drawing has to get right. It is the same on every call.
pub(crate) fn sample_notebook() -> String {
    let stream =
        |text: &str| format!(r#"{{"output_type":"stream","name":"stdout","text":"{text}\n"}}"#);
    let figure_output = format!(
        r#"{{"output_type":"display_data","data":{{"image/png":"{}","text/plain":"<Figure size 640x480 with 1 Axes>"}},"metadata":{{}}}}"#,
        base64(&figure(640, 480))
    );
    let jpeg_output = format!(
        r#"{{"output_type":"execute_result","data":{{"image/jpeg":"{}"}},"metadata":{{}},"execution_count":1}}"#,
        base64(&jpeg_picture(480, 360))
    );
    let png_output = |bytes: &[u8]| {
        format!(
            r#"{{"output_type":"display_data","data":{{"image/png":"{}"}},"metadata":{{}}}}"#,
            base64(bytes)
        )
    };
    let broken = r#"{"output_type":"display_data","data":{"image/png":"not an image","text/plain":"this image could not be read"},"metadata":{}}"#;
    let outputs = [
        stream("text before the figure"),
        figure_output,
        stream("text between the images"),
        jpeg_output,
        png_output(&translucent(320, 240)),
        png_output(&tall(400, 1600)),
        broken.to_owned(),
        stream("text after the images"),
    ];
    format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"sample","metadata":{{}},"source":"show_images()","execution_count":null,"outputs":[{}]}}]}}
"#,
        outputs.join(",")
    )
}

/// Writes [`sample_notebook`] to `path`.
pub(crate) fn write_sample(path: &Path) -> io::Result<()> {
    std::fs::write(path, sample_notebook())
}

/// A PNG of noise from a fixed formula, so that every run measures the same
/// image and nothing but `image` is needed to make it.
fn noise_png(width: u32, height: u32) -> Vec<u8> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let image = image::RgbImage::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let [a, b, c, ..] = state.to_le_bytes();
        image::Rgb([a, b, c])
    });
    encode_png(image)
}

/// How long each protocol takes, from decoding a noisy PNG of the given size
/// to the data for the terminal, at 80 columns and cells of 10 by 20 pixels.
/// Nothing is written to the terminal.
pub(crate) fn run_bench(width: u32, height: u32) -> Vec<(ProtocolType, Duration)> {
    let png = noise_png(width, height);
    let json = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c","metadata":{{}},"source":"","execution_count":null,"outputs":[{{"output_type":"display_data","data":{{"image/png":"{}"}},"metadata":{{}}}}]}}]}}"#,
        base64(&png)
    );
    let notebook = europa_core::parse(json.as_bytes()).expect("the bench notebook loads");
    let Some(europa_core::Cell::Code(code)) = notebook.cells.first() else {
        panic!("no code cell");
    };
    let columns = std::num::NonZeroU16::new(80).expect("80 is not zero");
    [
        ProtocolType::Halfblocks,
        ProtocolType::Kitty,
        ProtocolType::Sixel,
        ProtocolType::Iterm2,
    ]
    .into_iter()
    .map(|protocol| {
        let mut renderer = ImageRenderer::with_parts(
            protocol,
            CellPixels::DEFAULT,
            ColorDepth::TrueColor,
            DEFAULT_LIMIT,
            Box::new(RatatuiEncoder),
        );
        let options = RenderOptions::new(columns);
        let started = Instant::now();
        let cell = europa_render::render_outputs_with_images(
            &code.outputs,
            &options,
            &mut |source, image| renderer.insert(source, image),
        );
        let slot = cell.images[0];
        let area = Rect::new(0, 0, slot.width, slot.height);
        let mut buffer = Buffer::empty(area);
        renderer.draw(&mut buffer, area, &code.outputs, &slot, 0);
        (protocol, started.elapsed())
    })
    .collect()
}

/// What was rendered for one code cell.
struct Laid {
    cell: europa_core::RenderedCell,
    outputs: Vec<europa_core::Output>,
}

/// Renders the outputs of every code cell for `width` columns, and hands the
/// pixels of the images to the renderer.
fn lay_out(
    cells: &[Vec<europa_core::Output>],
    width: u16,
    renderer: &mut ImageRenderer,
) -> Vec<Laid> {
    let options = RenderOptions {
        cell_pixels: renderer.cell_pixels(),
        ..RenderOptions::new(std::num::NonZeroU16::new(width.max(1)).expect("at least one"))
    };
    cells
        .iter()
        .map(|outputs| Laid {
            cell: europa_render::render_outputs_with_images(
                outputs,
                &options,
                &mut |source, image| {
                    renderer.insert(source, image);
                },
            ),
            outputs: outputs.clone(),
        })
        .collect()
}

/// The part of each cell that shows in `area` when the notebook is scrolled
/// to the line `scroll`: the cell, where it goes and the line it starts at.
fn visible_cells(laid: &[Laid], area: Rect, scroll: usize) -> Vec<(usize, Rect, usize)> {
    let mut parts = Vec::new();
    let mut offset = 0i64;
    let scroll = i64::try_from(scroll).unwrap_or(i64::MAX);
    for (index, part) in laid.iter().enumerate() {
        let top = offset - scroll;
        let lines = i64::try_from(part.cell.lines.len()).unwrap_or(i64::MAX);
        offset += lines;
        let first = top.max(0);
        let last = (top + lines).min(i64::from(area.height));
        if first >= last {
            continue;
        }
        let (Ok(y), Ok(height), Ok(cell_scroll)) = (
            u16::try_from(first),
            u16::try_from(last - first),
            usize::try_from((-top).max(0)),
        ) else {
            continue;
        };
        parts.push((
            index,
            Rect::new(area.x, area.y + y, area.width, height),
            cell_scroll,
        ));
    }
    parts
}

fn total_lines(laid: &[Laid]) -> usize {
    laid.iter().map(|part| part.cell.lines.len()).sum()
}

/// Shows the notebook until `q` is pressed.
fn view(path: &Path, choice: ImageProtocolChoice, panic_after_first: bool) -> io::Result<ExitCode> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

    let bytes = std::fs::read(path)?;
    let notebook = europa_core::parse(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let cells: Vec<Vec<europa_core::Output>> = notebook
        .cells
        .iter()
        .filter_map(|cell| match cell {
            europa_core::Cell::Code(code) => Some(code.outputs.clone()),
            _ => None,
        })
        .collect();

    let mut terminal = ratatui::init();
    // After `init`, and before any event is read, because the answer of the
    // terminal to the query comes through the input.
    let mut renderer = ImageRenderer::new(choice);
    let mut laid = Vec::new();
    let mut laid_width = 0;
    let mut scroll = 0usize;
    let mut failures = 0;
    let mut first_frame = true;
    let result = loop {
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height.saturating_sub(1));
        if laid_width != size.width {
            laid = lay_out(&cells, size.width, &mut renderer);
            laid_width = size.width;
        }
        let most = total_lines(&laid).saturating_sub(usize::from(area.height));
        scroll = scroll.min(most);

        let parts = visible_cells(&laid, area, scroll);
        let placements: Vec<Placement> = parts
            .iter()
            .flat_map(|(index, part_area, cell_scroll)| {
                visible_placements(*part_area, &laid[*index].cell, *cell_scroll)
            })
            .collect();
        let mut written = if renderer.placements_changed(&placements) {
            clear_screen(&mut terminal).is_ok()
        } else {
            true
        };
        let status = format!(
            " {:?}  cell {}x{} px  line {scroll}/{most}  j k scroll  q quit",
            renderer.protocol(),
            renderer.cell_pixels().width,
            renderer.cell_pixels().height,
        );
        if written {
            written = terminal
                .draw(|frame| {
                    for (index, part_area, cell_scroll) in &parts {
                        let part = &laid[*index];
                        draw_cell(
                            frame.buffer_mut(),
                            *part_area,
                            &part.cell,
                            &part.outputs,
                            *cell_scroll,
                            &mut renderer,
                        );
                    }
                    let bar = Rect::new(0, size.height.saturating_sub(1), size.width, 1);
                    frame.render_widget(ratatui::widgets::Paragraph::new(status), bar);
                })
                .is_ok();
        }
        if !written {
            failures += 1;
            if failures >= 2 {
                // Writing failed twice in a row, with half-blocks the second
                // time, so the terminal is taken to be gone.
                break Ok(ExitCode::from(1));
            }
            renderer.fall_back_visible();
            let _ = clear_screen(&mut terminal);
            continue;
        }
        failures = 0;
        if first_frame && panic_after_first {
            panic!("the probe was asked to panic after the first frame");
        }
        first_frame = false;

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => break Ok(ExitCode::SUCCESS),
                KeyCode::Char('j') | KeyCode::Down => scroll += 1,
                KeyCode::Char('k') | KeyCode::Up => scroll = scroll.saturating_sub(1),
                _ => {}
            },
            // A new size of a cell changes the slots, so the cells are
            // rendered again, which a new width does through the check above.
            Event::Resize(..) if renderer.on_resize() => laid_width = 0,
            _ => {}
        }
    };
    ratatui::restore();
    result
}

/// Chooses a protocol the way the application does, and writes what was
/// chosen and how long it took to `path`.
fn select_only(path: &Path) -> io::Result<()> {
    use std::io::IsTerminal;

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let started = Instant::now();
    if interactive {
        ratatui::init();
    }
    let renderer = ImageRenderer::new(ImageProtocolChoice::Auto);
    if interactive {
        ratatui::restore();
    }
    let cell = renderer.cell_pixels();
    std::fs::write(
        path,
        format!(
            "protocol: {:?}\ncell_pixels: {}x{}\nelapsed_ms: {}\n",
            renderer.protocol(),
            cell.width,
            cell.height,
            started.elapsed().as_millis()
        ),
    )
}

fn finished(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

/// Runs what the arguments ask for.
pub(crate) fn run(command: ProbeCommand) -> ExitCode {
    match command {
        ProbeCommand::WriteSample(path) => finished(write_sample(&path)),
        ProbeCommand::SelectOnly(path) => finished(select_only(&path)),
        ProbeCommand::Bench => {
            for (protocol, elapsed) in run_bench(4000, 3000) {
                println!("{protocol:?}: {} ms", elapsed.as_millis());
            }
            ExitCode::SUCCESS
        }
        ProbeCommand::View {
            notebook,
            choice,
            panic,
        } => match view(&notebook, choice, panic) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        },
    }
}
