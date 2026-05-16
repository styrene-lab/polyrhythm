fn main() {
    eprintln!(
        "td50-drumgizmo-hihat-mapper-rs is a non-live prototype. \
         The tested Rust mapping core exists, but ALSA sequencer I/O is not wired yet. \
         Keep using td50-drumgizmo-hihat-mapper for live DRS runs."
    );
    std::process::exit(2);
}
