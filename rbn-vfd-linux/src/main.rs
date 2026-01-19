mod config;
mod models;
mod services;

fn main() {
    let config = config::Config::load();
    let store = services::SpotStore::new(config.min_snr, config.max_age_minutes);
    println!("SpotStore created with {} spots", store.count());
}
