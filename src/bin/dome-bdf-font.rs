//! Resolve a BDF for native clients which do not link the C++ appearance hook.
#[allow(dead_code)]
#[path = "dome-compositor/qt.rs"]
mod qt;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--qt") { args.remove(0); std::env::set_var("DOME_BDF_QT_ALIAS", "1"); }
    if args.first().is_some_and(|a| a == "--list") {
        println!("{}", serde_json::to_string(&qt::bitmap_fonts())?);
        return Ok(());
    }
    let mut appearance = dome::appearance::Appearance::load();
    if let Some(font) = args.first() { appearance.font = font.clone(); }
    if let Some(size) = args.get(1) { appearance.font_size = size.parse()?; }
    if let Some(scale) = args.get(2) { appearance.font_scale = scale.parse()?; }
    if args.is_empty() { qt::normalize_appearance(&mut appearance); }
    appearance.validate()?;
    println!("{}", qt::resolved_font(&appearance)?);
    Ok(())
}
