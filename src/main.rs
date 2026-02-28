mod app;
mod drawn;


use std::io;
use app::App;

fn main() -> Result<(), io::Error> {
    let mut application = App::new();
    application.run()
}
