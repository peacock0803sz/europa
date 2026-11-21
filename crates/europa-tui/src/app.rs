//! Startup and event loop.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::mpsc::{self, TryRecvError};

use europa_core::{ImageProtocolChoice, RenderOptions};
use ratatui::crossterm::event::{self, Event};

use crate::exit::{EX_DATAERR, EX_IOERR, EX_NOINPUT};
use crate::image_view::{clear_screen, ImageRenderer};
use crate::viewer::{self, Effect, Input, Viewer};
use crate::viewer_view::{status_name, view};

/// Message from background threads.
enum Msg {
    /// Terminal event.
    Event(Event),
    /// Signal number.
    Signal(i32),
    /// Input thread failed.
    InputFailed(std::io::Error),
}

/// Run the viewer for `args`, returning the exit code.
pub(crate) fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let path = match crate::cli::parse(args) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("europa: {}: {error}", path.display());
            return ExitCode::from(EX_NOINPUT);
        }
    };
    let notebook = match europa_core::parse(&bytes) {
        Ok(notebook) => notebook,
        Err(error) => {
            eprintln!("europa: {}: {error}", path.display());
            return ExitCode::from(EX_DATAERR);
        }
    };
    if !std::io::stdin().is_terminal() {
        eprintln!("europa: standard input is not a terminal");
        return ExitCode::from(EX_IOERR);
    }
    if !std::io::stdout().is_terminal() {
        eprintln!("europa: standard output is not a terminal");
        return ExitCode::from(EX_IOERR);
    }
    let mut signals = match signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ]) {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("europa: cannot handle signals: {error}");
            return ExitCode::from(EX_IOERR);
        }
    };
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(_) => {
            ratatui::restore();
            return ExitCode::from(EX_IOERR);
        }
    };
    let mut renderer = ImageRenderer::new(ImageProtocolChoice::Auto);
    let size = match terminal.size() {
        Ok(size) => size,
        Err(_) => {
            ratatui::restore();
            return ExitCode::from(EX_IOERR);
        }
    };
    let mut options =
        RenderOptions::new(std::num::NonZeroU16::new(size.width.max(1)).expect("non-zero"));
    options.background = renderer.background();
    options.cell_pixels = renderer.cell_pixels();
    let language = notebook.source_language();
    let file_name = status_name(&path);
    let mut viewer = {
        let mut opener = viewer::NotebookRenderer {
            notebook: &notebook,
            language,
            images: &mut renderer,
        };
        Viewer::open(
            file_name,
            notebook.cells.len(),
            size.width,
            size.height,
            options,
            &mut opener,
        )
    };
    let (sender, receiver) = mpsc::channel::<Msg>();
    let input_sender = sender.clone();
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(event) => {
                if input_sender.send(Msg::Event(event)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = input_sender.send(Msg::InputFailed(error));
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for signal in signals.forever() {
            if sender.send(Msg::Signal(signal)).is_err() {
                break;
            }
        }
    });
    let mut failures = 0;
    let code = loop {
        let first = match receiver.recv() {
            Ok(message) => message,
            Err(_) => break ExitCode::from(EX_IOERR),
        };
        let mut pending = vec![first];
        loop {
            match receiver.try_recv() {
                Ok(message) => pending.push(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        let mut quit = None;
        for message in pending {
            match message {
                Msg::Event(Event::Key(key)) => {
                    let Some(mapped) = crate::keymap::key_from(key) else {
                        continue;
                    };
                    let mut cell_renderer = viewer::NotebookRenderer {
                        notebook: &notebook,
                        language,
                        images: &mut renderer,
                    };
                    match viewer::update(&mut viewer, Input::Key(mapped), &mut cell_renderer) {
                        Effect::None => {}
                        Effect::Quit => {
                            quit = Some(ExitCode::SUCCESS);
                            break;
                        }
                    }
                }
                Msg::Event(Event::Resize(columns, rows)) => {
                    renderer.on_resize();
                    let input = Input::Resize {
                        columns,
                        rows,
                        cell_pixels: renderer.cell_pixels(),
                    };
                    let mut cell_renderer = viewer::NotebookRenderer {
                        notebook: &notebook,
                        language,
                        images: &mut renderer,
                    };
                    match viewer::update(&mut viewer, input, &mut cell_renderer) {
                        Effect::None => {}
                        Effect::Quit => {
                            quit = Some(ExitCode::SUCCESS);
                            break;
                        }
                    }
                }
                Msg::Event(_) => {}
                Msg::Signal(number) => {
                    quit = Some(ExitCode::from((128 + number) as u8));
                    break;
                }
                Msg::InputFailed(error) => {
                    let _ = error;
                    quit = Some(ExitCode::from(EX_IOERR));
                    break;
                }
            }
        }
        if let Some(code) = quit {
            break code;
        }
        match draw_frame(&mut terminal, &viewer, &notebook, &mut renderer) {
            Ok(()) => failures = 0,
            Err(()) => {
                failures += 1;
                if failures >= 2 {
                    break ExitCode::from(EX_IOERR);
                }
            }
        }
    };
    ratatui::restore();
    code
}

fn draw_frame(
    terminal: &mut ratatui::DefaultTerminal,
    viewer: &Viewer,
    notebook: &europa_core::Notebook,
    renderer: &mut ImageRenderer,
) -> Result<(), ()> {
    let area = ratatui::layout::Rect::new(0, 0, viewer.columns, viewer.rows);
    let mut scratch = ratatui::buffer::Buffer::empty(area);
    let placements = view(viewer, notebook, &mut scratch, renderer);
    if renderer.placements_changed(&placements) {
        clear_screen(terminal).map_err(|_| ())?;
    }
    match terminal.draw(|frame| {
        view(viewer, notebook, frame.buffer_mut(), renderer);
    }) {
        Ok(_) => Ok(()),
        Err(_) => {
            renderer.fall_back_visible();
            clear_screen(terminal).map_err(|_| ())?;
            terminal
                .draw(|frame| {
                    view(viewer, notebook, frame.buffer_mut(), renderer);
                })
                .map_err(|_| ())?;
            Ok(())
        }
    }
}
