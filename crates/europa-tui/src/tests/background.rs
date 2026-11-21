//! Background interpretation.

use europa_core::Background;
use ratatui_image::picker::{Capability, ProtocolType};

use crate::image_protocol::{background_from, probe_from};

fn answer(response: &str) -> Option<(u8, u8, u8)> {
    let mut parser = ratatui_image::picker::cap_parser::Parser::new();
    let mut last = None;
    for c in response.chars() {
        for reply in parser.push(c) {
            if let ratatui_image::picker::cap_parser::Response::Background(r, g, b) = reply {
                last = Some((r, g, b));
            }
        }
    }
    last
}

#[test]
fn table_answers_map_to_light_or_dark() {
    for (response, expected) in [
        ("\u{1b}]11;rgb:ffff/ffff/ffff\u{7}", Background::Light),
        ("\u{1b}]11;rgb:0000/0000/0000\u{1b}\\", Background::Dark),
        ("\u{1b}]11;rgb:8080/8080/8080\u{7}", Background::Light),
        ("\u{1b}]11;rgb:7f7f/7f7f/7f7f\u{7}", Background::Dark),
        ("\u{1b}]11;rgb:fdfd/f6f6/e3e3\u{7}", Background::Light),
        ("\u{1b}]11;rgb:0000/2b2b/3636\u{7}", Background::Dark),
        ("\u{1b}]11;nonsense\u{7}", Background::Dark),
    ] {
        assert_eq!(background_from(answer(response)), expected, "{response:?}");
    }
    assert_eq!(background_from(None), Background::Dark);
}

#[test]
fn probe_keeps_the_reported_background() {
    assert_eq!(
        probe_from(ProtocolType::Kitty, &[Capability::Background(1, 2, 3)]).background,
        Some((1, 2, 3))
    );
    assert_eq!(probe_from(ProtocolType::Kitty, &[]).background, None);
}
