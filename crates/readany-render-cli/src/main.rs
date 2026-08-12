#![forbid(unsafe_code)]

use readany_render::{Options, SvgOptions, rasterise, render, to_svg};
use std::io::Write;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: readany-render <input> [--json|--svg <directory>]")?;
    let mode = args.next().unwrap_or_else(|| "--json".into());
    let output = args.next();
    let bytes = std::fs::read(&input)?;
    let rendered = render(
        &bytes,
        &Options {
            filename: Some(&input),
            ..Options::default()
        },
    )?;
    if mode == "--json" {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &rendered)?;
        std::io::stdout().lock().write_all(b"\n")?;
    } else if mode == "--svg" || mode == "--png" {
        let directory = output.ok_or("--svg and --png require an output directory")?;
        std::fs::create_dir_all(&directory)?;
        for (index, page) in rendered.pages.iter().enumerate() {
            if mode == "--svg" {
                let svg = to_svg(page, &SvgOptions::default())?;
                std::fs::write(format!("{directory}/page-{}.svg", index + 1), svg)?;
            } else {
                let png = rasterise(page, 1.0)?.encode_png()?;
                std::fs::write(format!("{directory}/page-{}.png", index + 1), png)?;
            }
        }
    } else {
        return Err(format!("unknown mode {mode}").into());
    }
    Ok(())
}
