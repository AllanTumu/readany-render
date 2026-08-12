#![no_main]
use libfuzzer_sys::fuzz_target;
use readany_render::{FontSource, Options, OwnedFont, render};

fuzz_target!(|data: &[u8]| {
    let fonts = [OwnedFont { family: "fuzz".into(), bytes: data.to_vec() }];
    let _ = render(b"cell\nvalue", &Options { filename: Some("fuzz.csv"), fonts: FontSource::Borrowed(&fonts), ..Options::default() });
});

