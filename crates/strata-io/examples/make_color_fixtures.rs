#[path = "support/fixtures.rs"]
mod fixtures;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .ok_or("usage: make_color_fixtures <new-output-directory>")?;
    if args.next().is_some() {
        return Err("expected one output directory".into());
    }
    fixtures::generate(std::path::Path::new(&output))?;
    println!(
        "Generated native Display P3 swatches, exact RGBA16 ramp, alpha edge, and expected.json in {}",
        std::path::Path::new(&output).display()
    );
    Ok(())
}
