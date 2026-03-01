mod app;
mod drawn;
mod ui;
mod state;

use std::io;
use app::App;

fn main() -> Result<(), io::Error> {
    let mut application = App::new();
    application.run()
}
