use std::sync::{atomic, Arc, Mutex};

use symphonia::core::formats::FormatReader;

use crate::Track;

pub struct TrackSource {
    decoder: Arc<Mutex<opus::Decoder>>,
    reader: Arc<Mutex<symphonia::default::formats::MkvReader>>,
    current_time: Arc<atomic::AtomicU64>,
    sample_buf: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    duration: Option<std::time::Duration>,
}

impl TrackSource {
    pub(super) async fn new(track: &super::Track) -> Option<(Self, TrackSourceHandle)> {
        let format = &track.format;
        let channels = format.audio_channels.unwrap_or(2) as u16;
        let sample_rate = format
            .audio_sample_rate
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(48000);

        let duration = Some(std::time::Duration::from_secs(track.duration));

        let decoder = opus::Decoder::new(
            sample_rate,
            match channels {
                1 => opus::Channels::Mono,
                _ => opus::Channels::Stereo,
            },
        )
        .ok()?;

        // Check if the link is still valid
        // If not try to get new one
        let stream = match super::TrackStream::new(format).await {
            Some(stream) => stream,
            None => {
                let new_track = Track::new(track.id.to_string()).await?;
                super::TrackStream::new(&new_track.format).await?
            }
        };

        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(stream), Default::default());
        let reader = symphonia::default::formats::MkvReader::try_new(mss, &Default::default())
            .map_err(|e| dbg!(e))
            .ok()?;

        let decoder = Arc::new(Mutex::new(decoder));
        let reader = Arc::new(Mutex::new(reader));
        let current_time = Arc::new(atomic::AtomicU64::default());

        let mut source = TrackSource {
            decoder: decoder.clone(),
            reader: reader.clone(),
            current_time: current_time.clone(),
            sample_buf: Vec::new(),
            channels,
            sample_rate,
            duration,
        };

        source.decode();

        let handle = TrackSourceHandle {
            reader,
            decoder,
            current_time,
            metadata: Arc::new(Metadata {
                title: track.title.clone(),
                author: track.author.clone(),
                thumbnails: track.thumbnails.clone(),
                duration: track.duration,
            }),
        };

        Some((source, handle))
    }

    pub(super) fn decode(&mut self) -> Option<()> {
        let Ok(packet) = self.reader.lock().unwrap().next_packet() else {
            return None;
        };

        self.current_time
            .store(packet.ts, atomic::Ordering::Relaxed);

        let decoder = &mut *self.decoder.lock().unwrap();
        let packet_size = decoder.get_nb_samples(packet.buf()).unwrap_or(1500);
        let actual_size = packet_size * self.channels as usize;
        let mut tmp = vec![0.0; actual_size];

        decoder
            .decode_float(packet.buf(), &mut tmp, false)
            .map(|_| ())
            .ok()?;

        self.sample_buf.append(&mut tmp);

        Some(())
    }
}

impl Iterator for TrackSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.sample_buf.len() <= self.sample_buf.capacity() / 2 {
            self.decode()?;
        }

        Some(self.sample_buf.remove(0))
    }
}

impl rodio::Source for TrackSource {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.sample_buf.len())
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.duration
    }
}

#[derive(Clone)]
pub struct TrackSourceHandle {
    decoder: Arc<Mutex<opus::Decoder>>,
    reader: Arc<Mutex<symphonia::default::formats::MkvReader>>,
    current_time: Arc<atomic::AtomicU64>,
    metadata: Arc<Metadata>,
}

impl TrackSourceHandle {
    pub(crate) fn seek(&self, sec: u64, frac: f64) {
        self.reader
            .lock()
            .unwrap()
            .seek(
                symphonia::core::formats::SeekMode::Coarse,
                symphonia::core::formats::SeekTo::Time {
                    time: symphonia::core::units::Time { seconds: sec, frac },
                    track_id: None,
                },
            )
            .unwrap();

        self.decoder.lock().unwrap().reset_state().unwrap();
    }

    pub fn current_time(&self) -> u64 {
        self.current_time.load(atomic::Ordering::Relaxed) / 1000
    }

    pub fn metadata(&self) -> Arc<Metadata> {
        self.metadata.clone()
    }
}

pub struct Metadata {
    pub title: Arc<str>,
    pub author: Arc<str>,
    pub thumbnails: Arc<[rusty_ytdl::Thumbnail]>,
    pub duration: u64,
}
