#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = readany_render::render(data, &readany_render::Options { filename: Some("input.rtf"), ..Default::default() });
});
