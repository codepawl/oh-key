//! Times the engine on realistic typing: `cargo run --release -p oh-key-engine --example bench`.

use std::hint::black_box;
use std::time::Instant;

use oh_key_engine::{Config, Engine, InputMethod, KeyResult};

const KEY_COUNT: usize = 10_000_000;

const TELEX_TEXT: &str = "Tooi laf nguwowif Vieetj Nam, ddang gox tieengs Vieetj bawfng muwowjt chuwowng trinhf \
    nhanh vaf khoong looxi. Hoaf bifnh, thuyr thuur, khuyeenx khichs, quyeets ddinhj. \
    The quick brown fox jumps over the lazy dog; text, window, coffee, Facebook. ";

const VNI_TEXT: &str = "To6i la2 ngu7o7i2 Vie6t5 Nam, d9ang go4 tie6ng1 Vie6t5 ba82ng mu7o75t chu7o7ng tri2nh \
    nhanh va2 kho6ng lo6i4. Hoa2 bi2nh, thuy3 thu3, khuye6n1 khich1, quye6t1 d9i5nh. ";

fn measure(label: &str, input_method: InputMethod, text: &str) {
    let keys: Vec<char> = text.chars().collect();
    let mut engine = Engine::new(Config {
        input_method,
        ..Config::default()
    });
    let mut committed_bytes = 0usize;
    let started = Instant::now();
    for key_index in 0..KEY_COUNT {
        let key = keys[key_index % keys.len()];
        match black_box(engine.process_key(black_box(key))) {
            KeyResult::Commit(text) => committed_bytes += text.len(),
            KeyResult::Composing | KeyResult::PassThrough => {}
        }
    }
    let elapsed = started.elapsed();
    let nanoseconds_per_key = elapsed.as_nanos() as f64 / KEY_COUNT as f64;
    println!(
        "{label}: {KEY_COUNT} keys in {elapsed:.2?} = {nanoseconds_per_key:.1} ns/key ({committed_bytes} bytes committed)"
    );
}

fn main() {
    measure("telex", InputMethod::Telex, TELEX_TEXT);
    measure("vni", InputMethod::Vni, VNI_TEXT);
}
