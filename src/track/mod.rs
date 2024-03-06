mod source;
use std::sync::Arc;

use source::TrackSource;
pub use source::TrackSourceHandle;

mod stream;
use stream::TrackStream;

mod queue;
pub use queue::{TracksQueue, TracksQueueHandle};

#[derive(Debug, Clone)]
pub struct Track {
    format: Arc<rusty_ytdl::VideoFormat>,
    pub id: Arc<str>,
    pub title: Arc<str>,
    pub author: Arc<str>,
    pub thumbnails: Arc<[rusty_ytdl::Thumbnail]>,
    pub duration: u64,
}

impl Track {
    pub async fn new(id: impl AsRef<str>) -> Option<Self> {
        let options = rusty_ytdl::VideoOptions {
            quality: rusty_ytdl::VideoQuality::HighestAudio,
            filter: rusty_ytdl::VideoSearchOptions::Audio,
            ..Default::default()
        };
        let video = rusty_ytdl::Video::new_with_options(id.as_ref(), options)
            .map_err(|e| dbg!(e))
            .ok()?;
        let info = video.get_basic_info().await.map_err(|e| dbg!(e)).ok()?;
        let format = info
            .formats
            .iter()
            .find(|info| info.codecs == Some(String::from("opus")))?;

        Some(Self {
            id: info.video_details.video_id.into(),
            format: format.clone().into(),
            title: info.video_details.title.into(),
            author: info
                .video_details
                .author
                .map_or_else(|| "???".to_string(), |author| author.name)
                .into(),
            thumbnails: info.video_details.thumbnails.into(),
            duration: info.video_details.length_seconds.parse().unwrap_or(0),
        })
    }

    pub async fn start(&self) -> Option<(TrackSource, TrackSourceHandle)> {
        TrackSource::new(self).await
    }
}
