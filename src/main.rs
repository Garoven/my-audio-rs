use std::{sync::Arc, time::Duration};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let rt = Arc::new(rt);
    let manager = audio::AudioManager::new(rt.clone());

    manager.send(audio::Request::Queue(audio::Queue::Add(
        "https://www.youtube.com/watch?v=_eudL0xra3A".to_string(),
    )));

    manager.send(audio::Request::Queue(audio::Queue::Add(
        "https://www.youtube.com/watch?v=y7FBy4eIxig".to_string(),
    )));

    loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(track) = manager.current_track() else {
            manager.send(audio::Request::Play(audio::Play::Current, true));
            continue;
        };

        println!("{}/{}", track.current_time(), track.metadata().duration);
    }
}
