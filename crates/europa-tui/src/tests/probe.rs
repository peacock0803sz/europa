use std::ffi::OsString;
use std::path::PathBuf;

use europa_core::{Cell, ImageProtocolChoice};
use ratatui_image::picker::ProtocolType;

use crate::probe::{parse_args, run_bench, sample_notebook, write_sample, ProbeCommand};

fn parse(args: &[&str]) -> Result<ProbeCommand, String> {
    parse_args(
        args.iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter(),
    )
}

fn view(path: &str, choice: ImageProtocolChoice, panic: bool) -> ProbeCommand {
    ProbeCommand::View {
        notebook: PathBuf::from(path),
        choice,
        panic,
    }
}

#[test]
fn a_notebook_alone_is_shown_with_an_automatic_protocol() {
    assert_eq!(
        parse(&["book.ipynb"]),
        Ok(view("book.ipynb", ImageProtocolChoice::Auto, false))
    );
}

#[test]
fn every_protocol_name_is_read() {
    for (name, choice) in [
        ("auto", ImageProtocolChoice::Auto),
        ("kitty", ImageProtocolChoice::Kitty),
        ("sixel", ImageProtocolChoice::Sixel),
        ("iterm2", ImageProtocolChoice::Iterm2),
        ("halfblock", ImageProtocolChoice::Halfblock),
    ] {
        assert_eq!(
            parse(&["book.ipynb", "--protocol", name]),
            Ok(view("book.ipynb", choice, false))
        );
    }
}

#[test]
fn options_can_come_before_the_notebook_and_in_any_order() {
    assert_eq!(
        parse(&["--panic", "--protocol", "sixel", "book.ipynb"]),
        Ok(view("book.ipynb", ImageProtocolChoice::Sixel, true))
    );
}

#[test]
fn the_other_commands_are_read() {
    assert_eq!(
        parse(&["--write-sample", "out.ipynb"]),
        Ok(ProbeCommand::WriteSample(PathBuf::from("out.ipynb")))
    );
    assert_eq!(parse(&["--bench"]), Ok(ProbeCommand::Bench));
    assert_eq!(
        parse(&["--select-only", "out.txt"]),
        Ok(ProbeCommand::SelectOnly(PathBuf::from("out.txt")))
    );
}

#[test]
fn wrong_arguments_are_refused() {
    for args in [
        vec![],
        vec!["--nope"],
        vec!["book.ipynb", "--protocol"],
        vec!["book.ipynb", "--protocol", "chafa"],
        vec!["--write-sample"],
        vec!["--select-only"],
        vec!["a.ipynb", "b.ipynb"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}

fn sample_outputs() -> Vec<europa_core::Output> {
    let notebook = europa_core::parse(sample_notebook().as_bytes()).expect("the sample loads");
    let Some(Cell::Code(code)) = notebook.cells.first() else {
        panic!("no code cell");
    };
    code.outputs.clone()
}

#[test]
fn the_sample_has_four_images_and_the_text_of_the_broken_one() {
    let outputs = sample_outputs();
    let options = europa_core::RenderOptions::new(std::num::NonZeroU16::new(80).expect("non-zero"));
    let cell = europa_render::render_outputs(&outputs, &options);
    assert_eq!(cell.images.len(), 4);
    let text: String = cell
        .lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect();
    assert!(text.contains("could not be read"), "{text}");
}

#[test]
fn the_sample_is_the_same_every_time() {
    assert_eq!(sample_notebook(), sample_notebook());
}

#[test]
fn the_sample_has_an_image_taller_than_a_window() {
    let outputs = sample_outputs();
    let options = europa_core::RenderOptions::new(std::num::NonZeroU16::new(80).expect("non-zero"));
    let cell = europa_render::render_outputs(&outputs, &options);
    assert!(cell.images.iter().any(|slot| slot.height > 60));
}

#[test]
fn the_sample_is_written_and_read_back() {
    let dir = std::env::temp_dir().join(format!("europa-probe-sample-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("directory");
    let path = dir.join("sample.ipynb");
    write_sample(&path).expect("writes");
    assert_eq!(
        std::fs::read_to_string(&path).expect("reads"),
        sample_notebook()
    );
    std::fs::remove_dir_all(&dir).expect("removes");
}

#[test]
fn the_bench_gives_one_time_for_each_protocol() {
    let results = run_bench(40, 30);
    let protocols: Vec<ProtocolType> = results.iter().map(|(protocol, _)| *protocol).collect();
    assert_eq!(protocols.len(), 4);
    for protocol in [
        ProtocolType::Halfblocks,
        ProtocolType::Kitty,
        ProtocolType::Sixel,
        ProtocolType::Iterm2,
    ] {
        assert_eq!(
            protocols.iter().filter(|found| **found == protocol).count(),
            1
        );
    }
}
