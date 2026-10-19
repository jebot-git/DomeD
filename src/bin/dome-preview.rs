use dome::{service, Scene};
use std::sync::{Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut state = Scene::demo();
    dome::load_geometry(&mut state).map_err(|e| e.to_string())?;
    let scene = Arc::new(Mutex::new(state));
    let port = std::env::var("DOME_PREVIEW_PORT")
        .unwrap_or_else(|_| "49640".into())
        .parse()?;
    let server = service::serve(scene, port)?;
    eprintln!("Dome layout laboratory: http://127.0.0.1:{port} (synthetic surfaces)");
    server.join().map_err(|_| "preview thread stopped")?;
    Ok(())
}
