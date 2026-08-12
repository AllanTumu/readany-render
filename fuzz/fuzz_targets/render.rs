#![no_main]
use libfuzzer_sys::fuzz_target;
use readany_render::{Options, render};

fuzz_target!(|data: &[u8]| {
    for filename in ["input.xlsx", "input.ods", "input.docx", "input.odt", "input.rtf", "input.pptx", "input.odp", "input.csv", "input.png"] {
        let _ = render(data, &Options { filename: Some(filename), ..Options::default() });
    }
});

