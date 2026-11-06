mod common;

use europa_core::{DecodedImage, ImageMime, Output, RenderedCell};
use europa_render::{decode_image, render_outputs, render_outputs_with_images};

fn bundle(entries: &[(&str, Vec<u8>)]) -> String {
    let fields: Vec<String> = entries
        .iter()
        .map(|(mime, bytes)| format!(r#""{mime}":"{}""#, common::b64(bytes)))
        .collect();
    format!("{{{}}}", fields.join(","))
}

fn image_output(mime: &str, bytes: Vec<u8>) -> Output {
    common::display(&bundle(&[(mime, bytes)]))
}

fn draw(outputs: &[Output], width: u16) -> (RenderedCell, Vec<(u64, DecodedImage)>) {
    let mut sunk = Vec::new();
    let cell = render_outputs_with_images(outputs, &common::opts(width), &mut |source, image| {
        sunk.push((source, image));
    });
    (cell, sunk)
}

#[test]
fn a_png_makes_one_slot_and_hides_the_plain_text() {
    let output = common::display(&format!(
        r#"{{"image/png":"{}","text/plain":"<Figure>"}}"#,
        common::b64(&common::png(640, 480))
    ));
    let (cell, sunk) = draw(&[output], 80);
    assert_eq!(cell.images.len(), 1);
    let slot = cell.images[0];
    assert_eq!((slot.line, slot.width, slot.height), (0, 64, 24));
    assert_eq!((slot.output, slot.mime), (0, ImageMime::Png));
    assert_eq!(cell.lines.len(), 24);
    assert!(common::texts(&cell).iter().all(String::is_empty));
    assert_eq!(sunk.len(), 1);
    assert_eq!(sunk[0].0, slot.source);
}

#[test]
fn the_sink_gets_the_exact_colors_of_a_png() {
    let (_, sunk) = draw(&[image_output("image/png", common::png(40, 30))], 80);
    let image = &sunk[0].1;
    for (x, y, expected) in [
        (0, 0, [0, 0, 0]),
        (39, 0, [39, 0, 39]),
        (5, 20, [5, 20, 25]),
        (39, 29, [39, 29, 68]),
    ] {
        assert_eq!(common::rgb_at(image, x, y), expected, "at {x},{y}");
    }
}

#[test]
fn the_sink_gets_the_colors_of_a_jpeg_within_a_tolerance() {
    let (_, sunk) = draw(&[image_output("image/jpeg", common::jpeg(64, 64))], 80);
    let image = &sunk[0].1;
    // Far from the corners, so that the gradient holds and red differs from
    // blue by much more than the tolerance.
    for (x, y, expected) in [(8, 56, [8, 56, 64]), (8, 32, [8, 32, 40])] {
        let actual = common::rgb_at(image, x, y);
        for (got, want) in actual.into_iter().zip(expected) {
            assert!(
                got.abs_diff(want) <= 12,
                "at {x},{y}: {actual:?} is not near {expected:?}"
            );
        }
    }
}

#[test]
fn a_partial_alpha_is_mixed_with_white() {
    let alpha = 128;
    let over_white = |channel: u32| {
        u8::try_from((channel * alpha + 255 * (255 - alpha) + 127) / 255).expect("a byte")
    };
    let png = common::png_rgba(40, 30, u8::try_from(alpha).expect("a byte"));
    let (_, sunk) = draw(&[image_output("image/png", png)], 80);
    let image = &sunk[0].1;
    for (x, y) in [(0, 0), (39, 0), (0, 29), (20, 10), (39, 29)] {
        let expected = [over_white(x), over_white(y), over_white(0)];
        assert_eq!(common::rgb_at(image, x, y), expected, "at {x},{y}");
    }
    // The same value worked out by hand, in case the formula above is wrong too.
    assert_eq!(common::rgb_at(image, 39, 29), [147, 142, 127]);
}

#[test]
fn png_wins_over_jpeg_and_over_rich_text() {
    let png = common::b64(&common::png(10, 10));
    let jpeg = common::b64(&common::jpeg(10, 10));
    let data = format!(
        r#"{{"image/jpeg":"{jpeg}","image/png":"{png}","text/html":"<b>x</b>","text/markdown":"x","application/json":{{"a":1}},"image/svg+xml":"<svg/>"}}"#
    );
    let (cell, _) = draw(&[common::display(&data)], 80);
    assert_eq!(cell.images.len(), 1);
    assert_eq!(cell.images[0].mime, ImageMime::Png);
}

#[test]
fn a_jpeg_alone_in_an_execute_result_is_a_jpeg_slot() {
    let output = common::result(&bundle(&[("image/jpeg", common::jpeg(20, 20))]));
    let (cell, _) = draw(&[output], 80);
    assert_eq!(cell.images.len(), 1);
    assert_eq!(cell.images[0].mime, ImageMime::Jpeg);
}

#[test]
fn a_broken_png_gives_way_to_a_good_jpeg_or_to_the_text() {
    let jpeg = common::b64(&common::jpeg(10, 10));
    let data = format!(r#"{{"image/png":"AAAA","image/jpeg":"{jpeg}"}}"#);
    let (cell, _) = draw(&[common::display(&data)], 80);
    assert_eq!(cell.images[0].mime, ImageMime::Jpeg);
    let (cell, _) = draw(
        &[common::display(
            r#"{"image/png":"AAAA","text/plain":"fallback"}"#,
        )],
        80,
    );
    assert!(cell.images.is_empty());
    assert_eq!(common::texts(&cell), ["fallback"]);
}

#[test]
fn the_format_comes_from_the_content_not_the_mime() {
    let (cell, _) = draw(&[image_output("image/jpeg", common::png(10, 10))], 80);
    assert_eq!(cell.images.len(), 1);
    assert_eq!(cell.images[0].mime, ImageMime::Jpeg);
}

#[test]
fn a_list_of_strings_and_stray_whitespace_give_the_same_source() {
    let plain = draw(&[image_output("image/png", common::png(10, 10))], 80).0;
    let text = common::b64(&common::png(10, 10));
    let (head, tail) = text.split_at(text.len() / 2);
    let list = format!(r#"{{"image/png":["{head}\n","{tail}"]}}"#);
    let spaced = format!(r#"{{"image/png":"{head} \n {tail}"}}"#);
    for data in [list, spaced] {
        let cell = draw(&[common::display(&data)], 80).0;
        assert_eq!(cell.images[0].source, plain.images[0].source);
    }
}

#[test]
fn a_number_or_an_object_is_not_an_image() {
    for data in [r#"{"image/png":5}"#, r#"{"image/png":{"a":1}}"#] {
        let (cell, _) = draw(&[common::display(data)], 80);
        assert!(cell.images.is_empty());
    }
}

#[test]
fn streams_and_errors_are_drawn_as_before() {
    let outputs = [common::stdout("a\n"), common::error("E", "v", &["tb"])];
    let (with, sunk) = draw(&outputs, 20);
    assert_eq!(with, render_outputs(&outputs, &common::opts(20)));
    assert!(sunk.is_empty());
}

#[test]
fn metadata_sets_the_size_of_the_slot() {
    let data = format!(
        r#"{{"image/png":"{}"}}"#,
        common::b64(&common::png(1280, 960))
    );
    let outputs = common::display_with(&data, r#"{"image/png":{"width":640,"height":480}}"#);
    let (cell, _) = draw(&outputs, 80);
    assert_eq!((cell.images[0].width, cell.images[0].height), (64, 24));
}

#[test]
fn the_orientation_of_a_jpeg_turns_the_slot() {
    let bytes = common::jpeg_with_orientation(40, 30, 6);
    let (cell, _) = draw(&[image_output("image/jpeg", bytes)], 80);
    assert_eq!((cell.images[0].width, cell.images[0].height), (3, 2));
}

#[test]
fn an_animated_png_is_its_first_frame() {
    let (cell, sunk) = draw(&[image_output("image/png", common::apng(30, 20))], 80);
    assert_eq!((cell.images[0].width, cell.images[0].height), (3, 1));
    assert_eq!((sunk[0].1.width, sunk[0].1.height), (30, 20));
}

#[test]
fn text_image_text_keeps_the_order() {
    let outputs = [
        common::stdout("before\n"),
        image_output("image/png", common::png(100, 60)),
        common::stdout("after\n"),
    ];
    let (cell, _) = draw(&outputs, 80);
    let texts = common::texts(&cell);
    assert_eq!(cell.images[0].line, 1);
    assert_eq!((cell.images[0].width, cell.images[0].height), (10, 3));
    assert_eq!(texts, ["before", "", "", "", "after"]);
}

#[test]
fn two_slots_do_not_overlap() {
    let outputs = [
        image_output("image/png", common::png(100, 60)),
        image_output("image/jpeg", common::jpeg(100, 60)),
    ];
    let (cell, _) = draw(&outputs, 80);
    let [first, second] = [cell.images[0], cell.images[1]];
    assert_eq!(second.line, first.line + usize::from(first.height));
    assert_eq!((first.output, second.output), (0, 1));
}

#[test]
fn a_slot_counts_outputs_rather_than_merged_streams() {
    let outputs = [
        common::stdout("a\n"),
        common::stdout("b\n"),
        image_output("image/png", common::png(100, 60)),
    ];
    let (cell, _) = draw(&outputs, 80);
    assert_eq!(cell.images[0].output, 2);
}

#[test]
fn equal_arguments_give_equal_results_whatever_the_sink_does() {
    let outputs = [image_output("image/png", common::png(100, 60))];
    let (first, _) = draw(&outputs, 80);
    let second = render_outputs_with_images(&outputs, &common::opts(80), &mut |_, _| {});
    assert_eq!(first, second);
    assert_eq!(first, render_outputs(&outputs, &common::opts(80)));
}

#[test]
fn a_slot_on_the_border_of_the_omitted_part_goes_whole() {
    let outputs = [
        common::stdout("top\n"),
        image_output("image/png", common::png(100, 400)),
        common::stdout("bottom\n"),
    ];
    let mut sunk = 0;
    let cell = render_outputs_with_images(&outputs, &common::limited(80, 7), &mut |_, _| {
        sunk += 1;
    });
    assert_eq!(
        common::texts(&cell),
        ["top", "... 7 lines hidden ...", "bottom"]
    );
    assert!(cell.images.is_empty());
    assert_eq!(sunk, 1);
}

#[test]
fn an_image_taller_than_the_limit_allows_is_left_out() {
    let mut outputs = vec![image_output("image/png", common::png(100, 3000))];
    outputs.push(common::stdout(&"x\n".repeat(60)));
    let (cell, _) = draw(&outputs, 80);
    assert!(cell.images.is_empty());
    assert!(cell.lines.len() <= 200);
    assert_eq!(common::texts(&cell)[0], "... 150 lines hidden ...");
}

#[test]
fn slots_that_fit_either_side_stay_where_they_belong() {
    let mut outputs = vec![image_output("image/png", common::png(100, 60))];
    outputs.push(common::stdout(&"x\n".repeat(300)));
    outputs.push(image_output("image/png", common::png(100, 60)));
    let (cell, _) = draw(&outputs, 80);
    assert_eq!(cell.lines.len(), 200);
    assert_eq!(cell.images.len(), 2);
    assert_eq!(cell.images[0].line, 0);
    assert_eq!(cell.images[1].line, 99 + 1 + (303 - 206));
}

#[test]
fn decode_image_returns_what_the_sink_got() {
    let outputs = [image_output("image/png", common::png(30, 20))];
    let (cell, sunk) = draw(&outputs, 80);
    let decoded = decode_image(&outputs, &cell.images[0]).expect("decodes");
    assert_eq!(decoded, sunk[0].1);
}

#[test]
fn decode_image_is_none_when_the_outputs_changed() {
    let outputs = [image_output("image/png", common::png(30, 20))];
    let (cell, _) = draw(&outputs, 80);
    let slot = cell.images[0];
    let other = [image_output("image/png", common::png(31, 20))];
    assert!(decode_image(&other, &slot).is_none());
    assert!(decode_image(&[], &slot).is_none());
    assert!(decode_image(&[common::stdout("x")], &slot).is_none());
}

fn notice_of(data: &str) -> Vec<String> {
    let (cell, _) = draw(&[common::display(data)], 80);
    assert!(cell.images.is_empty());
    common::texts(&cell)
}

#[test]
fn broken_data_is_named_with_its_reason() {
    let png = common::png(10, 10);
    let jpeg = common::jpeg(10, 10);
    let cases = [
        r#"{"image/png":"!!!"}"#.to_owned(),
        format!(
            r#"{{"image/png":"{}"}}"#,
            common::b64(&png[..png.len() / 2])
        ),
        format!(r#"{{"image/png":"{}"}}"#, common::b64(b"neither")),
        r#"{"image/png":5}"#.to_owned(),
    ];
    for data in cases {
        assert_eq!(notice_of(&data), ["[image/png: broken]"], "{data}");
    }
    let half = common::b64(&jpeg[..jpeg.len() / 2]);
    assert_eq!(
        notice_of(&format!(r#"{{"image/jpeg":"{half}"}}"#)),
        ["[image/jpeg: broken]"]
    );
}

#[test]
fn a_jpeg_cut_in_its_scan_is_named_broken() {
    let cut = common::b64(&common::jpeg_cut_in_scan(256, 256));
    assert_eq!(
        notice_of(&format!(r#"{{"image/jpeg":"{cut}"}}"#)),
        ["[image/jpeg: broken]"]
    );
}

#[test]
fn a_jpeg_with_bytes_after_its_end_is_still_an_image() {
    let mut jpeg = common::jpeg(10, 10);
    jpeg.extend_from_slice(b"padding");
    let (cell, _) = draw(&[image_output("image/jpeg", jpeg)], 80);
    assert_eq!(cell.images.len(), 1);
}

#[test]
fn too_large_images_are_named_with_their_reason() {
    for (mime, bytes) in [
        ("image/png", common::png_with_size(8001, 8001)),
        ("image/png", common::png_with_size(60000, 60000)),
        ("image/jpeg", common::jpeg_with_size(8001, 8001)),
    ] {
        let data = format!(r#"{{"{mime}":"{}"}}"#, common::b64(&bytes));
        assert_eq!(notice_of(&data), [format!("[{mime}: too large]")]);
    }
}

#[test]
fn a_too_large_image_gives_way_to_the_next_entry() {
    let big = common::b64(&common::png_with_size(8001, 8001));
    let data = format!(r#"{{"image/png":"{big}","text/plain":"words"}}"#);
    assert_eq!(notice_of(&data), ["words"]);
    let jpeg = common::b64(&common::jpeg(10, 10));
    let data = format!(r#"{{"image/png":"{big}","image/jpeg":"{jpeg}"}}"#);
    let (cell, _) = draw(&[common::display(&data)], 80);
    assert_eq!(cell.images[0].mime, ImageMime::Jpeg);
}

#[test]
fn only_png_and_jpeg_get_a_reason() {
    let data = r#"{"image/png":"!!!","image/gif":"R0lGOD"}"#;
    assert_eq!(notice_of(data), ["[image/gif, image/png: broken]"]);
}

#[test]
fn a_broken_output_does_not_disturb_the_others() {
    let good = [common::stdout("before\n"), common::plain("after")];
    let broken = common::display(r#"{"image/png":"!!!"}"#);
    let mixed = [good[0].clone(), broken, good[1].clone()];
    let alone = common::texts(&render_outputs(&good, &common::opts(80)));
    let texts = common::texts(&render_outputs(&mixed, &common::opts(80)));
    let without_notice: Vec<_> = texts
        .into_iter()
        .filter(|line| !line.starts_with("[image/png"))
        .collect();
    assert_eq!(without_notice, alone);
}

#[test]
fn rendering_images_leaves_the_saved_notebook_unchanged() {
    let png = common::b64(&common::png(30, 20));
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"cell_type":"code","id":"c1","metadata":{{}},"source":"","outputs":[{{"output_type":"display_data","data":{{"image/png":"{png}","text/plain":"<Figure>"}},"metadata":{{"image/png":{{"width":1e2}}}}}}],"execution_count":null}}]}}"#
    );
    let notebook = europa_core::parse(text.as_bytes()).expect("loads");
    let before = europa_core::serialize(&notebook);
    let outputs = common::cell_outputs(&notebook);
    let _ = render_outputs_with_images(outputs, &common::opts(40), &mut |_, _| {});
    let after = europa_core::serialize(&notebook);
    assert_eq!(before, after);
}
