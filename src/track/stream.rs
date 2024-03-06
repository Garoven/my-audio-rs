use std::thread::JoinHandle;

pub type Stream = Box<dyn std::io::Read + Send + Sync>;

pub(super) struct TrackStream {
    url: std::sync::Arc<str>,
    buf: Vec<u8>,
    pos: u64,
    content_length: u64,
    stream: Stream,
    reconnect: Option<JoinHandle<Option<Stream>>>,
}

impl TrackStream {
    pub async fn new(format: &rusty_ytdl::VideoFormat) -> Option<Self> {
        let response = ureq::get(&format.url).call().map_err(|e| dbg!(e)).ok()?;

        Some(Self {
            url: format.url.clone().into(),
            buf: Vec::new(),
            pos: 0,
            content_length: response
                .header("Content-Length")
                .and_then(|s| s.parse().ok())?,
            stream: response.into_reader(),
            reconnect: None,
        })
    }

    fn reconnect(&mut self) -> JoinHandle<Option<Stream>> {
        let url = self.url.clone();
        let pos = self.pos;
        let content_length = self.content_length;

        std::thread::spawn(move || -> Option<Stream> {
            let request = ureq::get(url.as_ref())
                .set("Range", &format!("bytes={}-", pos.min(content_length - 1)))
                .call()
                .map_err(|e| dbg!(e))
                .ok()?;

            Some(request.into_reader())
        })
    }
}

impl std::io::Seek for TrackStream {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match pos {
            std::io::SeekFrom::End(offset) => {
                self.pos = (self.content_length as i64 + offset).unsigned_abs();
            }
            std::io::SeekFrom::Start(offset) => {
                self.pos = offset;
            }
            std::io::SeekFrom::Current(offset) => {
                self.pos = (self.pos as i64 + offset).unsigned_abs()
            }
        }

        // Thread can't panic so first unwrap is safe
        // TODO remove second unwrap somehow
        self.stream = self.reconnect().join().unwrap().unwrap();
        self.buf.clear();

        Ok(self.pos)
    }
}

impl std::io::Read for TrackStream {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut tmp = [0u8; 16384];
        let len = match self.stream.read(&mut tmp) {
            Ok(len) => len,
            Err(_) => match self.reconnect {
                Some(ref handle) if handle.is_finished() => {
                    // Handle is finished so its safe to unwrap
                    let handle = self.reconnect.take().unwrap();
                    if let Ok(Some(stream)) = handle.join() {
                        self.stream = stream;
                        self.stream.read(&mut tmp)?
                    } else {
                        0
                    }
                }
                None => {
                    self.reconnect = Some(self.reconnect());

                    0
                }
                _ => 0,
            },
        };

        self.pos += len as u64;
        self.buf.extend_from_slice(&tmp[0..len]);
        let len = self.stream.read(&mut tmp).unwrap_or(0);
        self.buf.extend_from_slice(&tmp[0..len]);
        self.pos += len as u64;
        let result = (&self.buf[0..12288.min(self.buf.len())]).read(buf)?;
        self.buf.drain(0..result);

        Ok(result)
    }
}

impl symphonia::core::io::MediaSource for TrackStream {
    fn byte_len(&self) -> Option<u64> {
        Some(self.content_length)
    }

    fn is_seekable(&self) -> bool {
        true
    }
}
