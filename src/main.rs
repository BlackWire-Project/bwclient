mod app;
mod crypto;
mod relay;
mod state;
mod storage;
mod sync;
mod ui;

use anyhow::Result;
use app::App;

fn main() -> Result<()> {
    let mut application = App::bootstrap()?;
    application.run()
}
