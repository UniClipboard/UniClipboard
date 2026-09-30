mod adapters;
mod app;
mod platform;
mod ui;

fn main() -> anyhow::Result<()> {
    app::run()
}
